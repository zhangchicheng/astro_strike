//! Constant auto-scroll (classic vertical-shmup camera) + parallax.
//!
//! The camera scrolls straight up at a fixed speed, independent of the ship
//! (so projectiles keep a constant on-screen speed no matter how the ship
//! moves — see the design note in git history). The ship "rides" the scroll so
//! it holds its screen position when idle; `player::player_movement` then lets
//! it move freely within the viewport.
//!
//! As the camera scrolls, each Tiled layer is shifted by
//! `camera_position * (1 - factor)`: factor `1.0` scrolls at full speed (terrain);
//! factor `< 1.0` drifts slowly (distant celestial bodies).

use bevy::prelude::*;

use crate::common::{MAP_HALF, X_BOUND, Y_BOUND, level_chunk_bottom_y, level_chunk_top_y};
use crate::level::Level;
use crate::player::Player;
use crate::state::{Phase, live};

/// Auto-scroll speed (world px/s) during the intro cruise and during combat.
const INTRO_SCROLL_SPEED: f32 = 40.0;
const COMBAT_SCROLL_SPEED: f32 = 70.0;

/// How quickly the camera eases toward the ship's x each second (higher = snappier).
const X_FOLLOW_RATE: f32 = 6.0;

#[derive(Component)]
pub struct ParallaxLayer {
    pub factor: Vec2,
    /// The layer's resting world position (e.g. a chunk's center). The parallax
    /// offset is added to this, so positioned layers stay put at factor 1.0.
    pub base: Vec2,
}

pub(super) fn plugin(app: &mut App) {
    // Scroll first, then place the parallax layers for the camera's new position.
    // Runs whenever the world is live — dialogue doesn't freeze the scroll, but
    // game over does (instantly).
    app.add_systems(Update, (auto_scroll, apply_parallax).chain().run_if(live));
}

/// Scroll the camera up at a constant speed and carry the ship with it. During the
/// intro it stops just below the current level's chunks (keeps the backdrop
/// "space"); in combat it stops at the top of the level's chunks (the boss). The
/// caps are per-level, so beating a boss lets the camera scroll on into the next
/// level's space corridor without ever rewinding.
fn auto_scroll(
    time: Res<Time>,
    phase: Res<State<Phase>>,
    level: Res<Level>,
    mut camera: Query<&mut Transform, (With<Camera2d>, Without<Player>)>,
    mut player: Query<&mut Transform, With<Player>>,
) {
    let Ok(mut camera) = camera.single_mut() else {
        return;
    };
    let l = level.0.saturating_sub(1);
    let (speed, cap) = match phase.get() {
        // Cap the intro just below this level's space/chunk boundary.
        Phase::Intro => (INTRO_SCROLL_SPEED, level_chunk_bottom_y(l) - Y_BOUND),
        // Combat and dialogue: scroll toward this level's boss. (Dialogue happens
        // either at the boss — already at the cap — or mid-level via the taunt.)
        _ => (COMBAT_SCROLL_SPEED, level_chunk_top_y(l) - Y_BOUND),
    };

    let old = camera.translation.y;
    let new = (old + speed * time.delta_secs()).min(cap);
    camera.translation.y = new;
    let delta = new - old;

    if let Ok(mut player) = player.single_mut() {
        // Carry the ship along so it keeps its on-screen position when idle.
        player.translation.y += delta;

        // Horizontal follow: the level (512) is wider than the view (400), so ease
        // the camera toward the ship's x, clamped so the view never leaves the map.
        // This lets the ship reach the map's left/right edges (the camera only
        // scrolls sideways — it never follows vertically, which is what kept
        // projectiles at a constant on-screen speed).
        let x_limit = (MAP_HALF.x - X_BOUND).max(0.0);
        let target_x = player.translation.x.clamp(-x_limit, x_limit);
        let t = (X_FOLLOW_RATE * time.delta_secs()).min(1.0);
        camera.translation.x += (target_x - camera.translation.x) * t;
    }
}

/// Offset each layer relative to the camera according to its parallax factor.
fn apply_parallax(
    camera: Query<&Transform, With<Camera2d>>,
    mut layers: Query<(&ParallaxLayer, &mut Transform), Without<Camera2d>>,
) {
    let Ok(camera) = camera.single() else {
        return;
    };
    let cam = camera.translation.truncate();
    for (layer, mut transform) in &mut layers {
        let pos = layer.base + cam * (Vec2::ONE - layer.factor);
        transform.translation.x = pos.x;
        transform.translation.y = pos.y;
    }
}
