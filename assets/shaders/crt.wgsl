// CRT display post-process (see src/crt.rs).
//
// Runs as the last fullscreen pass before the upscale, after palette grading,
// so the whole finished frame — world, HUD, menus — shows through a curved
// tube face: barrel distortion with a black bezel outside the glass, one
// scanline groove per game art pixel row, a vertical RGB aperture grille,
// corner vignette, edge chromatic fringing, and a faint brightness flicker.
//
// `textureSampleLevel` (not `textureSample`) everywhere: the bezel early-out
// puts the samples in non-uniform control flow, where implicit-gradient
// sampling is invalid WGSL.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var texture_sampler: sampler;

struct Crt {
    resolution: vec2<f32>,
    cell: f32,
    time: f32,
    curvature: f32,
    scanline: f32,
    mask: f32,
    vignette: f32,
    aberration: f32,
    flicker: f32,
}

@group(0) @binding(2) var<uniform> crt: Crt;

const TAU: f32 = 6.28318530718;

// Barrel-distorts a 0..1 UV as if the image were projected onto the bulging
// face of a picture tube: each axis bows out by the square of how far along
// the OTHER axis the point sits.
fn curve(uv: vec2<f32>) -> vec2<f32> {
    let c = uv * 2.0 - 1.0;
    let bow = abs(c.yx) * crt.curvature;
    let warped = c + c * bow * bow;
    return warped * 0.5 + 0.5;
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let uv = curve(in.uv);

    // Off the curved glass: the monitor's bezel.
    if any(uv < vec2(0.0)) || any(uv > vec2(1.0)) {
        return vec4(0.0, 0.0, 0.0, 1.0);
    }

    // Chromatic aberration: the electron guns converge imperfectly, fringing
    // red outward and blue inward, worse toward the edges.
    let shift = (uv - 0.5) * crt.aberration;
    let r = textureSampleLevel(screen_texture, texture_sampler, uv + shift, 0.0).r;
    let g = textureSampleLevel(screen_texture, texture_sampler, uv, 0.0).g;
    let b = textureSampleLevel(screen_texture, texture_sampler, uv - shift, 0.0).b;
    var color = vec3(r, g, b);

    // Scanlines: one groove per TWO game art pixel rows (tied to art rows, not
    // physical pixels, so the look survives hidpi and resizes; the 2-row pitch
    // keeps the lines chunky enough to read at the game's 2x art scale).
    let art_row = uv.y * crt.resolution.y / (2.0 * max(crt.cell, 1.0));
    color *= 1.0 - crt.scanline * (0.5 - 0.5 * cos(art_row * TAU));

    // Aperture grille: vertical RGB phosphor stripes at a 3-physical-pixel
    // pitch, fixed to the glass (pre-warp screen position, like real stripes).
    let stripe = u32(in.position.x) % 3u;
    var grille = vec3(1.0 - crt.mask);
    grille[stripe] = 1.0;
    color *= grille;

    // Give back roughly the light the grooves and stripes soaked up.
    color *= 1.0 + 0.5 * crt.scanline + 0.66 * crt.mask;

    // Corner vignette (the term is 1 at center, falling toward the edges) and
    // a faint mains-hum brightness flicker.
    let vig = 16.0 * uv.x * uv.y * (1.0 - uv.x) * (1.0 - uv.y);
    color *= pow(vig, crt.vignette);
    color *= 1.0 + crt.flicker * sin(crt.time * 120.0);

    // A couple of physical pixels of soft falloff where the glass meets the
    // bezel, so the curved boundary doesn't alias.
    let bd = min(uv, vec2(1.0) - uv);
    color *= smoothstep(0.0, 2.0 / crt.resolution.y, min(bd.x, bd.y));

    return vec4(color, 1.0);
}
