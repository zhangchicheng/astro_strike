// Whole-frame palette grading with a Bayer dither-dissolve (see
// src/palette_grade.rs).
//
// Runs as a fullscreen pass after the UI has been drawn: every pixel whose
// color exactly matches one of the `from_colors` entries (the game's known art
// colors, in linear space) is replaced by the paired target color. Two target
// palettes ride along — the outgoing and incoming schemes — and each pixel
// picks between them by comparing `progress` against a 4x4 ordered-dither
// threshold, so a scheme change sweeps across the screen as a densifying
// checkerboard instead of a hard cut. All other colors pass through, keeping
// accents like the game-over banner intact.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var texture_sampler: sampler;

struct PaletteGrade {
    from_colors: array<vec4<f32>, 16>,
    to_old: array<vec4<f32>, 16>,
    to_new: array<vec4<f32>, 16>,
    count: u32,
    progress: f32,
    cell: f32,
}

@group(0) @binding(2) var<uniform> grade: PaletteGrade;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    var color = textureSample(screen_texture, texture_sampler, in.uv);

    // Classic 4x4 Bayer matrix; +0.5 centers the 16 thresholds inside (0, 1)
    // so progress 0 shows only the old palette and 1 only the new. `cell`
    // sizes the pattern to one game art pixel.
    var bayer = array<f32, 16>(
         0.0,  8.0,  2.0, 10.0,
        12.0,  4.0, 14.0,  6.0,
         3.0, 11.0,  1.0,  9.0,
        15.0,  7.0, 13.0,  5.0,
    );
    let cell = vec2<u32>(floor(in.position.xy / max(grade.cell, 1.0))) % vec2(4u, 4u);
    let threshold = (bayer[cell.y * 4u + cell.x] + 0.5) / 16.0;
    let take_new = grade.progress >= threshold;

    for (var i = 0u; i < grade.count; i++) {
        let d = color.rgb - grade.from_colors[i].rgb;
        // Exact-match substitution. The epsilon absorbs f32/sRGB rounding and
        // GPU decode drift (bright shades drift the most); the closest pair of
        // distinct art colors is ~0.01 apart squared, so 2e-4 cannot mismatch.
        if dot(d, d) < 2e-4 {
            if take_new {
                color = vec4(grade.to_new[i].rgb, color.a);
            } else {
                color = vec4(grade.to_old[i].rgb, color.a);
            }
            break;
        }
    }
    return color;
}
