//! Whole-frame palette grading.
//!
//! One fullscreen post-process (`shaders/palette_grade.wgsl`) gives the entire
//! composition — map, ships, bullets, pickups, HUD, dialogue — a unified color
//! scheme per level. Instead of pre-recoloring assets per level, the shader
//! substitutes the game's known art colors in the finished frame: the default
//! primaries plus the four palettes from `palettes.ron` map to the current
//! level's pair (see [`crate::recolor::level_scheme`]), and the art/text whites
//! map to the level's white accent. On game over everything drains to gray; on
//! the title screen the targets equal the sources, so the frame passes through
//! untouched.
//!
//! Scheme changes are not hard cuts: the shader carries the OLD and NEW target
//! palettes plus a progress value, and each pixel picks between them via a
//! 4x4 Bayer threshold — the classic ordered-dither dissolve. Over
//! [`TRANSITION_SECS`] the new scheme sprinkles in as a densifying checkerboard
//! (band crossings, death-gray, restarts, and title transitions all dissolve).
//!
//! The pass runs *after* the UI pass (so menus, HUD, and dialogue are graded
//! too) and before the final upscale. The scheme follows the level band the
//! camera is over, so the world changes color as the map scrolls into the next
//! level.

use bevy::asset::uuid_handle;
use bevy::core_pipeline::fullscreen_material::{FullscreenMaterial, FullscreenMaterialPlugin};
use bevy::core_pipeline::schedule::Core2d;
use bevy::core_pipeline::upscaling::upscaling;
use bevy::ecs::schedule::{ScheduleConfigs, ScheduleLabel};
use bevy::ecs::system::BoxedSystem;
use bevy::prelude::*;
use bevy::render::extract_component::ExtractComponent;
use bevy::render::render_resource::ShaderType;
use bevy::shader::{Shader, ShaderRef};
use bevy::ui_render::ui_pass;
use bevy::window::PrimaryWindow;

use crate::combat::GameProgress;
use crate::common::{CHUNK_PX, HEIGHT, LEVEL_ROWS, LEVELS, MAP_HALF, VIEW_SCALE};
use crate::dialogue::SpeakerText;
use crate::hud::{GameOverText, RewardText};
use crate::recolor::{GAME_OVER_PAIR, Palettes, Shade, WHITE, level_scheme};
use crate::state::Phase;

/// Capacity of the substitution table (12 entries are used; padded for headroom).
const MAX_ENTRIES: usize = 16;

/// Table entries actually in use: 5 palette pairs + the two whites. The count
/// is CONSTANT — schemes that don't remap an entry map it to itself — so the
/// old/new target palettes always align index-for-index during a dissolve.
const ENTRY_COUNT: u32 = 12;

/// How long a scheme change takes to dissolve across the screen.
const TRANSITION_SECS: f32 = 0.8;

/// UI text renders in pure white, unlike the art's `#F8F8F8` white.
const TEXT_WHITE: Shade = [255, 255, 255];

/// The accent color of the speaker names / money counter on the title screen
/// (and the base their per-level tint replaces).
const UI_YELLOW: Color = Color::srgb(1.0, 0.85, 0.3);

/// The grading shader, embedded in the binary and registered under this handle
/// at startup. Loading it from the assets folder instead (`ShaderRef::Path`)
/// arrives asynchronously: for the first seconds the pass silently skips —
/// invisible on the title screen (identity targets) but showing the game world
/// in raw default colors until the pipeline popped in.
const SHADER_HANDLE: Handle<Shader> = uuid_handle!("7b9a1c4e-5d2f-4a8b-9c3e-1f6a8d2b4e70");

/// The shader's substitution table: [`ENTRY_COUNT`] exact source colors and TWO
/// target palettes (the outgoing and incoming schemes), all in LINEAR color
/// space (the render target's working space). `progress` is the Bayer-dissolve
/// position (1.0 = fully on `to_new`), and `cell` is the dither cell size in
/// physical pixels (one game art pixel, so the pattern is resolution-stable).
/// Lives on the 2D camera; extracted to the render world every frame.
#[derive(Component, ExtractComponent, Clone, Copy, ShaderType)]
pub struct PaletteGrade {
    from: [Vec4; MAX_ENTRIES],
    to_old: [Vec4; MAX_ENTRIES],
    to_new: [Vec4; MAX_ENTRIES],
    count: u32,
    progress: f32,
    cell: f32,
}

impl Default for PaletteGrade {
    fn default() -> Self {
        Self {
            from: [Vec4::ZERO; MAX_ENTRIES],
            to_old: [Vec4::ZERO; MAX_ENTRIES],
            to_new: [Vec4::ZERO; MAX_ENTRIES],
            count: 0,
            progress: 1.0,
            cell: 1.0,
        }
    }
}

impl FullscreenMaterial for PaletteGrade {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(SHADER_HANDLE)
    }

    fn schedule() -> impl ScheduleLabel + Clone {
        Core2d
    }

    /// After the UI pass, so the HUD/dialogue/menus follow the scheme too;
    /// before the final upscale to the window.
    fn schedule_configs(system: ScheduleConfigs<BoxedSystem>) -> ScheduleConfigs<BoxedSystem> {
        system.after(ui_pass).before(upscaling)
    }
}

/// The source palettes ([`Palettes::DEFAULT`]): every pair an asset may have
/// been recolored to at load time, i.e. every color family the shader must remap.
#[derive(Resource)]
struct SourcePalettes(Palettes);

/// The dissolve in flight (or settled): the outgoing and incoming target
/// palettes and how far between them the screen is.
struct Transition {
    to_old: [Vec4; MAX_ENTRIES],
    to_new: [Vec4; MAX_ENTRIES],
    progress: f32,
}

pub(super) fn plugin(app: &mut App) {
    // Compiled into the binary; edits to the .wgsl need a rebuild. Inserting
    // under a fresh uuid handle cannot collide with an existing asset.
    let _ = app.world_mut().resource_mut::<Assets<Shader>>().insert(
        SHADER_HANDLE.id(),
        Shader::from_wgsl(
            include_str!("../assets/shaders/palette_grade.wgsl"),
            "shaders/palette_grade.wgsl",
        ),
    );
    app.add_plugins(FullscreenMaterialPlugin::<PaletteGrade>::default())
        .insert_resource(SourcePalettes(Palettes::DEFAULT))
        .add_systems(Update, drive_palette);
}

/// Converts an sRGB art shade to the linear-space value the shader compares
/// sampled frame pixels against.
fn lin(shade: Shade) -> Vec4 {
    let c = Color::srgb_u8(shade[0], shade[1], shade[2]).to_linear();
    Vec4::new(c.red, c.green, c.blue, 1.0)
}

/// The constant source side of the table: the five palette pairs, then the two
/// whites. Target arrays built by [`target_colors`] use the same indexing.
fn source_colors(p: &Palettes) -> [Vec4; MAX_ENTRIES] {
    let mut out = [Vec4::ZERO; MAX_ENTRIES];
    for (i, pair) in [p.default, p.red, p.green, p.yellow, p.blue].iter().enumerate() {
        out[2 * i] = lin(pair[0]);
        out[2 * i + 1] = lin(pair[1]);
    }
    out[10] = lin(WHITE);
    out[11] = lin(TEXT_WHITE);
    out
}

/// Target colors for one scheme: every dark shade to `dark`, every light shade
/// to `light`, the whites to the scheme's accent (or themselves if it has none
/// — the count never changes, so dissolves stay aligned).
fn target_colors(dark: Shade, light: Shade, white: Option<Shade>) -> [Vec4; MAX_ENTRIES] {
    let mut out = [Vec4::ZERO; MAX_ENTRIES];
    for i in 0..5 {
        out[2 * i] = lin(dark);
        out[2 * i + 1] = lin(light);
    }
    out[10] = lin(white.unwrap_or(WHITE));
    out[11] = lin(white.unwrap_or(TEXT_WHITE));
    out
}

/// Rebuilds the camera's substitution table every frame: title = identity
/// targets, game over = gray, otherwise the scheme of the level band the
/// camera is over. Any change of target palette starts a Bayer dissolve.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn drive_palette(
    mut commands: Commands,
    time: Res<Time>,
    palettes: Res<SourcePalettes>,
    progress: Res<GameProgress>,
    phase: Res<State<Phase>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut camera: Query<(Entity, &Transform, Option<&mut PaletteGrade>), With<Camera2d>>,
    mut accent_texts: Query<
        &mut TextColor,
        (Or<(With<RewardText>, With<SpeakerText>)>, Without<GameOverText>),
    >,
    mut banner_text: Query<&mut TextColor, With<GameOverText>>,
    mut transition: Local<Option<Transition>>,
    mut was_title: Local<bool>,
) {
    let Ok((entity, transform, grade)) = camera.single_mut() else {
        return;
    };
    let p = &palettes.0;

    // The target palette this frame wants (the dissolve's destination).
    let on_title = *phase.get() == Phase::Title;
    let desired = if on_title {
        // Identity: everything shows its authored colors.
        source_colors(p)
    } else {
        let band = ((transform.translation.y + MAP_HALF.y) / (LEVEL_ROWS as f32 * CHUNK_PX))
            .floor()
            .clamp(0.0, (LEVELS - 1) as f32) as u32;
        let scheme = level_scheme(band + 1);
        // The GAME OVER banner wears the level's light shade — the one colored
        // thing against the gray death palette (it's hidden while alive).
        let banner = Color::srgb_u8(scheme.pair[1][0], scheme.pair[1][1], scheme.pair[1][2]);
        for mut color in &mut banner_text {
            if color.0 != banner {
                color.0 = banner;
            }
        }
        if progress.game_over {
            // The whites stay white, keeping text contrast against the gray.
            target_colors(GAME_OVER_PAIR[0], GAME_OVER_PAIR[1], None)
        } else {
            target_colors(scheme.pair[0], scheme.pair[1], scheme.white)
        }
    };

    // Advance (or start) the dissolve toward `desired`. The very first frame
    // settles instantly, so the game doesn't boot mid-transition.
    let tr = transition.get_or_insert_with(|| Transition {
        to_old: desired,
        to_new: desired,
        progress: 1.0,
    });
    if tr.to_new != desired {
        if on_title || *was_title {
            // Crossing the title boundary snaps instead of dissolving: the
            // menu covers the moment, and the game should open already in its
            // level colors rather than fading up from the default ones.
            tr.to_old = desired;
            tr.to_new = desired;
            tr.progress = 1.0;
        } else {
            // A change mid-dissolve snaps to the old destination and restarts.
            tr.to_old = tr.to_new;
            tr.to_new = desired;
            tr.progress = 0.0;
        }
    } else {
        tr.progress = (tr.progress + time.delta_secs() / TRANSITION_SECS).min(1.0);
    }
    *was_title = on_title;

    // One dither cell = one game art pixel: the world is drawn at VIEW_SCALE
    // and the window may be hidpi-scaled on top.
    let cell = window
        .single()
        .map(|w| w.physical_height() as f32 / (HEIGHT / VIEW_SCALE))
        .unwrap_or(VIEW_SCALE)
        .max(1.0);

    let next = PaletteGrade {
        from: source_colors(p),
        to_old: tr.to_old,
        to_new: tr.to_new,
        count: ENTRY_COUNT,
        progress: tr.progress,
        cell,
    };

    // Speaker names / money counter follow the scheme's light shade (index 1 =
    // the default pair's light target), blended along the dissolve so the text
    // keeps pace with the world. Set as the actual text color (not a shader
    // substitution) so the glyphs' antialiased edges blend from the right color
    // — a shader remap of the fill left a fringe of the original yellow.
    let accent = if on_title {
        UI_YELLOW
    } else {
        let v = tr.to_old[1].lerp(tr.to_new[1], tr.progress);
        Color::linear_rgb(v.x, v.y, v.z)
    };
    for mut color in &mut accent_texts {
        if color.0 != accent {
            color.0 = accent;
        }
    }

    match grade {
        Some(mut grade) => *grade = next,
        None => {
            commands.entity(entity).insert(next);
        }
    }
}
