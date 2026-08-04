//! CRT display look for the whole game.
//!
//! One fullscreen post-process (`shaders/crt.wgsl`) draws the finished frame
//! as if on a curved picture tube: barrel distortion, scanlines, an RGB
//! aperture grille, vignette, chromatic fringing, and a faint flicker. The
//! pass runs after [`crate::palette_grade`] (so it sees the graded colors) and
//! before the final upscale to the window. All tuning lives in the constants
//! below; the per-frame uniform only carries what changes (time, resolution).
//!
//! The render-world plumbing mirrors Bevy's `FullscreenMaterialPlugin` rather
//! than using it: in Bevy 0.19 that plugin tracks its pipeline in an UNTYPED
//! `FullscreenMaterialPipelineId` component, so a second fullscreen material
//! on the same camera (this one, next to `PaletteGrade`) overwrites the
//! first's pipeline id and the passes draw with mismatched bind groups. Here
//! every per-view component is CRT-specific, so the two passes can coexist.

use bevy::asset::uuid_handle;
use bevy::core_pipeline::FullscreenShader;
use bevy::core_pipeline::fullscreen_material::fullscreen_material_system;
use bevy::core_pipeline::schedule::Core2d;
use bevy::core_pipeline::upscaling::upscaling;
use bevy::ecs::error::BevyError;
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::extract_component::{
    ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
    UniformComponentPlugin,
};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer};
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
    CachedRenderPipelineId, Canonical, ColorTargetState, ColorWrites, FragmentState, Operations,
    PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline,
    RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
    ShaderType, Specializer, SpecializerKey, TextureFormat, TextureSampleType, TextureView,
    TextureViewId, Variants,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::Shader;
use bevy::window::PrimaryWindow;

use crate::common::{HEIGHT, VIEW_SCALE};
use crate::palette_grade::PaletteGrade;

/// How strongly the tube face bulges (0 = flat).
const CURVATURE: f32 = 0.2;

/// How dark the groove between scanlines gets (fraction of full darkness).
const SCANLINE: f32 = 0.7;

/// How much the aperture grille dims a stripe's two foreign channels.
const MASK: f32 = 0.10;

/// Vignette exponent: higher darkens the corners more.
const VIGNETTE: f32 = 0.12;

/// Chromatic aberration offset at the screen edge, in UV units.
const ABERRATION: f32 = 0.0015;

/// Amplitude of the mains-hum brightness flicker.
const FLICKER: f32 = 0.01;

/// The CRT shader, embedded in the binary and registered under this handle at
/// startup (same reasoning as `palette_grade`: a path-loaded shader arrives
/// asynchronously and the pass would silently skip for the first seconds).
const SHADER_HANDLE: Handle<Shader> = uuid_handle!("3e8d5f2a-91c7-4b6e-8a4d-7c2f9e1b5a83");

/// The shader's uniform. Lives on the 2D camera; extracted to the render world
/// every frame. Field order must match the WGSL struct.
#[derive(Component, ExtractComponent, Clone, Copy, ShaderType)]
pub struct Crt {
    /// Render target size in physical pixels.
    resolution: Vec2,
    /// One game art pixel in physical pixels (scanline pitch).
    cell: f32,
    /// Seconds since startup, for the flicker.
    time: f32,
    curvature: f32,
    scanline: f32,
    mask: f32,
    vignette: f32,
    aberration: f32,
    flicker: f32,
}

impl Default for Crt {
    fn default() -> Self {
        Self {
            resolution: Vec2::ONE,
            cell: VIEW_SCALE,
            time: 0.0,
            curvature: CURVATURE,
            scanline: SCANLINE,
            mask: MASK,
            vignette: VIGNETTE,
            aberration: ABERRATION,
            flicker: FLICKER,
        }
    }
}

pub(super) fn plugin(app: &mut App) {
    // Compiled into the binary; edits to the .wgsl need a rebuild.
    let _ = app.world_mut().resource_mut::<Assets<Shader>>().insert(
        SHADER_HANDLE.id(),
        Shader::from_wgsl(include_str!("../assets/shaders/crt.wgsl"), "shaders/crt.wgsl"),
    );
    app.add_plugins((
        ExtractComponentPlugin::<Crt>::default(),
        UniformComponentPlugin::<Crt>::default(),
    ))
    .add_systems(Update, drive_crt);

    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app
        .add_systems(RenderStartup, init_pipeline)
        .add_systems(
            Render,
            (
                prepare_crt_pipelines.in_set(RenderSystems::Prepare),
                prepare_bind_groups.in_set(RenderSystems::PrepareBindGroups),
            ),
        )
        .add_systems(
            Core2d,
            // After the palette grade (the tube shows the graded frame) and
            // before the final upscale to the window.
            crt_pass
                .after(fullscreen_material_system::<PaletteGrade>)
                .before(upscaling),
        );
}

/// Keeps the camera's uniform current: window size (the render target's), the
/// physical size of one art pixel, and the running clock.
fn drive_crt(
    mut commands: Commands,
    time: Res<Time>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut camera: Query<(Entity, Option<&mut Crt>), With<Camera2d>>,
) {
    let Ok((entity, crt)) = camera.single_mut() else {
        return;
    };
    let Ok(window) = window.single() else {
        return;
    };
    let next = Crt {
        resolution: Vec2::new(window.physical_width() as f32, window.physical_height() as f32),
        cell: (window.physical_height() as f32 / (HEIGHT / VIEW_SCALE)).max(1.0),
        time: time.elapsed_secs(),
        ..default()
    };
    match crt {
        Some(mut crt) => *crt = next,
        None => {
            commands.entity(entity).insert(next);
        }
    }
}

// ---------------------------------------------------------------------------
// Render world (mirrors bevy_core_pipeline::fullscreen_material, typed to Crt)
// ---------------------------------------------------------------------------

#[derive(Resource)]
struct CrtPipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    variants: Variants<RenderPipeline, CrtSpecializer>,
}

struct CrtSpecializer;

#[derive(PartialEq, Eq, Hash, Clone, Copy, SpecializerKey)]
struct CrtPipelineKey {
    target_format: TextureFormat,
}

impl Specializer<RenderPipeline> for CrtSpecializer {
    type Key = CrtPipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        let fragment = descriptor.fragment_mut()?;
        fragment.set_target(
            0,
            ColorTargetState {
                format: key.target_format,
                blend: None,
                write_mask: ColorWrites::ALL,
            },
        );
        Ok(key)
    }
}

fn init_pipeline(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    fullscreen_shader: Res<FullscreenShader>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "crt_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<Crt>(true),
            ),
        ),
    );
    let sampler = render_device.create_sampler(&SamplerDescriptor::default());
    let desc = RenderPipelineDescriptor {
        label: Some("crt_pipeline".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen_shader.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: SHADER_HANDLE,
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::Rgba8UnormSrgb,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    };
    commands.insert_resource(CrtPipeline {
        layout,
        sampler,
        variants: Variants::new(CrtSpecializer, desc),
    });
}

#[derive(Component)]
struct CrtPipelineId(CachedRenderPipelineId);

fn prepare_crt_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<CrtPipeline>,
    views: Query<(Entity, &ExtractedView, Option<&Crt>), With<ExtractedCamera>>,
) -> Result<(), BevyError> {
    for (entity, view, crt) in &views {
        if crt.is_none() {
            commands.entity(entity).remove::<CrtPipelineId>();
            continue;
        }
        let pipeline_id = pipeline.variants.specialize(
            &pipeline_cache,
            CrtPipelineKey { target_format: view.target_format },
        )?;
        commands.entity(entity).insert(CrtPipelineId(pipeline_id));
    }
    Ok(())
}

/// Bind groups for both main textures: post-processing ping-pongs between
/// them, so either can be the source on a given frame.
#[derive(Component)]
struct CrtBindGroup {
    a: (TextureViewId, BindGroup),
    b: (TextureViewId, BindGroup),
}

fn prepare_bind_groups(
    mut commands: Commands,
    mut views: Query<(Entity, &ViewTarget, Option<&mut CrtBindGroup>, Option<&Crt>)>,
    pipeline: Option<Res<CrtPipeline>>,
    pipeline_cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<Crt>>,
    render_device: Res<RenderDevice>,
) {
    let Some(pipeline) = pipeline else {
        return;
    };
    let Some(settings_binding) = uniforms.uniforms().binding() else {
        return;
    };

    for (entity, view_target, mut bind_groups, crt) in &mut views {
        if crt.is_none() {
            commands.entity(entity).remove::<CrtBindGroup>();
            continue;
        }

        let create_bind_group = |texture: &TextureView| {
            (
                texture.id(),
                render_device.create_bind_group(
                    "crt_bind_group",
                    &pipeline_cache.get_bind_group_layout(&pipeline.layout),
                    &BindGroupEntries::sequential((
                        texture,
                        &pipeline.sampler,
                        settings_binding.clone(),
                    )),
                ),
            )
        };

        let main = view_target.main_texture_view();
        let other = view_target.main_texture_other_view();
        if let Some(bind_groups) = &mut bind_groups {
            if bind_groups.a.0 != main.id() {
                bind_groups.a = create_bind_group(main);
            }
            if bind_groups.b.0 != other.id() {
                bind_groups.b = create_bind_group(other);
            }
        } else {
            commands.entity(entity).insert(CrtBindGroup {
                a: create_bind_group(main),
                b: create_bind_group(other),
            });
        }
    }
}

fn crt_pass(
    view: ViewQuery<(
        &ViewTarget,
        &DynamicUniformIndex<Crt>,
        &CrtBindGroup,
        &CrtPipelineId,
    )>,
    pipeline_cache: Res<PipelineCache>,
    mut ctx: RenderContext,
) {
    let (view_target, settings_index, bind_groups, pipeline_id) = view.into_inner();

    let Some(pipeline) = pipeline_cache.get_render_pipeline(pipeline_id.0) else {
        return;
    };

    let post_process = view_target.post_process_write();
    let (_, bind_group) = if bind_groups.a.0 == post_process.source.id() {
        &bind_groups.a
    } else {
        &bind_groups.b
    };

    let pass_descriptor = RenderPassDescriptor {
        label: Some("crt_pass"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: post_process.destination,
            depth_slice: None,
            resolve_target: None,
            ops: Operations::default(),
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    };

    let mut render_pass = ctx.command_encoder().begin_render_pass(&pass_descriptor);
    render_pass.set_pipeline(pipeline);
    render_pass.set_bind_group(0, bind_group, &[settings_index.index()]);
    render_pass.draw(0..3, 0..1);
}
