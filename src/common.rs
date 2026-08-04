//! Shared constants, components, and helpers used across the game's plugins.

use bevy::prelude::*;

// ----- Playfield -----------------------------------------------------------
// The window is a retro 4:3 800x600; the camera magnifies the world 2x, so it
// shows a 400x300 world slice. The level is 512 wide (wider than the view), so
// the camera follows the ship horizontally (see `parallax::auto_scroll`) to reach
// the map's left/right edges, while auto-scrolling vertically.
pub const WIDTH: f32 = 800.0;
pub const HEIGHT: f32 = 600.0;
/// How much the camera magnifies the world (2x = everything drawn twice as big).
pub const VIEW_SCALE: f32 = 2.0;
/// Half-extents of the visible view in *world* units (window / 2 / zoom).
pub const X_BOUND: f32 = WIDTH / (2.0 * VIEW_SCALE);
pub const Y_BOUND: f32 = HEIGHT / (2.0 * VIEW_SCALE);

// ----- Levels -----
// The whole game is ONE continuous vertical map, stitched from 256x256 segments
// (MAP_COLS per row). It is the levels concatenated bottom-to-top: each level is a
// band of `SPACE_ROWS` random `space_N.tmx` segments (its intro/transition corridor)
// with `ORIGINAL_ROWS` of `chunk_N.tmx` above it (the level's terrain, boss at the
// top). The ship flies up through it without rewinding — beating a boss just lets
// the camera scroll on into the next level's space corridor.
/// Side length of one segment in pixels (16 tiles * 16 px).
pub const CHUNK_PX: f32 = 256.0;
pub const MAP_COLS: u32 = 2;
/// Rows of chunk terrain per level (chunk_1..10 -> 5 rows of 2 cols).
pub const ORIGINAL_ROWS: u32 = 5;
/// Rows of the random space corridor below each level's chunks.
pub const SPACE_ROWS: u32 = 8;
/// How many levels are concatenated (chunk_1..10 = level 1, chunk_11..20 = level
/// 2, ... chunk_41..50 = level 5).
pub const LEVELS: u32 = 5;
/// Rows in one level's band: its space corridor + its chunk terrain.
pub const LEVEL_ROWS: u32 = SPACE_ROWS + ORIGINAL_ROWS;
pub const MAP_ROWS: u32 = LEVELS * LEVEL_ROWS;
/// Full stitched map size in pixels.
pub const MAP_WIDTH: f32 = MAP_COLS as f32 * CHUNK_PX;
pub const MAP_HEIGHT: f32 = MAP_ROWS as f32 * CHUNK_PX;
/// Half the map per axis, i.e. the world coordinate of the map edges (±MAP_HALF).
pub const MAP_HALF: Vec2 = Vec2::new(MAP_WIDTH / 2.0, MAP_HEIGHT / 2.0);

/// World y of the bottom edge of level `l`'s band (0-indexed) — the start of its
/// space corridor.
pub fn level_bottom_y(l: u32) -> f32 {
    -MAP_HALF.y + (l * LEVEL_ROWS) as f32 * CHUNK_PX
}
/// World y where level `l`'s chunk terrain begins (top of its space corridor).
pub fn level_chunk_bottom_y(l: u32) -> f32 {
    level_bottom_y(l) + SPACE_ROWS as f32 * CHUNK_PX
}
/// World y of the top edge of level `l`'s chunk terrain (where its boss sits).
pub fn level_chunk_top_y(l: u32) -> f32 {
    level_bottom_y(l) + LEVEL_ROWS as f32 * CHUNK_PX
}

/// Where the ship spawns for a fresh game (bottom-center of level 1).
pub const PLAYER_START: Vec2 = Vec2::new(0.0, -MAP_HEIGHT / 2.0 + 60.0);

/// World Y the scrolling camera starts at for a fresh game — the map's bottom edge.
pub const CAMERA_START_Y: f32 = -(MAP_HEIGHT / 2.0 - Y_BOUND);

// ---------------------------------------------------------------------------
// Shared components
// ---------------------------------------------------------------------------

/// Straight-line velocity in world units per second.
#[derive(Component)]
pub struct Velocity(pub Vec2);

/// Axis-aligned collision box (full width/height).
#[derive(Component)]
pub struct Collider(pub Vec2);

/// Ship hit boxes are shrunk to this fraction of the sprite size, since the art
/// has transparent padding — otherwise craft "touch" (and explode) while there's
/// still a visible gap between them.
pub const HITBOX_SCALE: f32 = 0.6;

/// Simple AABB overlap test between two centered boxes.
pub fn overlaps(a_pos: Vec2, a_size: Vec2, b_pos: Vec2, b_size: Vec2) -> bool {
    (a_pos.x - b_pos.x).abs() < (a_size.x + b_size.x) / 2.0
        && (a_pos.y - b_pos.y).abs() < (a_size.y + b_size.y) / 2.0
}

// ---------------------------------------------------------------------------
// Shared bundle functions (see design.md "Bundle Functions" pattern)
// ---------------------------------------------------------------------------

/// A one-shot sound effect that despawns itself when finished. Spawn with
/// `commands.spawn(sfx(handle))`.
pub fn sfx(sound: Handle<AudioSource>) -> impl Bundle {
    (AudioPlayer::new(sound), PlaybackSettings::DESPAWN)
}
