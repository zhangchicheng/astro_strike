//! Level progression across the single continuous map (see `common.rs`).
//!
//! The whole game is one tall vertical scroll — level 1's space corridor + chunks,
//! then level 2's, etc. — so advancing a level is NOT a jump: the camera just keeps
//! scrolling up into the next level's space corridor (the caps in `parallax.rs` are
//! per-level; `boss::boss_death` bumps `Level` and re-enters `Intro` so the next
//! level's briefing plays over that corridor). Only a *fresh game* (leaving the
//! title) or a *death retry* rewinds — to level 1's / the current level's start.

use std::ops::Range;

use bevy::prelude::*;

use crate::combat::{GameProgress, MAX_HEALTH};
use crate::common::{CAMERA_START_Y, LEVELS};
use crate::player::{Player, Weapon};
use crate::state::Phase;

/// The current level (1-based). Drives the boss position, the scroll caps, the
/// intro dialogue, and the [`Difficulty`] knobs. (The level *count* lives in
/// `common::LEVELS` since the map size depends on it.)
#[derive(Resource)]
pub struct Level(pub u32);

impl Default for Level {
    fn default() -> Self {
        Level(1)
    }
}

/// The tuning knobs that scale with the level. ALL difficulty tuning lives in
/// [`Level::difficulty`], which interpolates each knob linearly from its level-1
/// baseline to its final-level maximum.
pub struct Difficulty {
    /// Seconds between enemy aircraft spawns.
    pub enemy_spawn_interval: f32,
    /// Enemy aircraft descent speed (world px/s).
    pub enemy_speed: f32,
    /// A new plane's delay before its first shot / between later shots.
    pub enemy_first_shot: Range<f32>,
    pub enemy_refire: Range<f32>,
    /// Speed of enemy plane and tank shots (world px/s).
    pub enemy_bullet_speed: f32,
    /// Seconds between UFO swarms, UFOs per swarm, and their flight speed.
    pub ufo_swarm_interval: f32,
    pub ufo_swarm_size: u32,
    pub ufo_speed: f32,
    /// Seconds between tank shots.
    pub tank_fire_interval: f32,
    /// The boss's hit points and the rest between its beam bursts.
    pub boss_health: u32,
    pub boss_beam_cooldown: f32,
}

impl Level {
    /// How far into the campaign this level is: 0.0 on level 1, 1.0 on the last.
    fn progress(&self) -> f32 {
        self.0.saturating_sub(1) as f32 / LEVELS.saturating_sub(1).max(1) as f32
    }

    /// The difficulty knobs for this level (level 1 = the game's original tuning).
    pub fn difficulty(&self) -> Difficulty {
        let t = self.progress();
        // Lerp from the level-1 baseline to the final-level maximum.
        let f = move |base: f32, max: f32| base + (max - base) * t;
        Difficulty {
            enemy_spawn_interval: f(0.75, 0.45),
            enemy_speed: f(100.0, 130.0),
            enemy_first_shot: f(0.4, 0.25)..f(1.0, 0.65),
            enemy_refire: f(1.2, 0.8)..f(2.0, 1.4),
            enemy_bullet_speed: f(300.0, 380.0),
            ufo_swarm_interval: f(8.0, 4.5),
            ufo_swarm_size: f(5.0, 8.0).round() as u32,
            ufo_speed: f(135.0, 165.0),
            tank_fire_interval: f(1.6, 1.0),
            boss_health: f(40.0, 70.0).round() as u32,
            boss_beam_cooldown: f(1.5, 0.9),
        }
    }
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Level>()
        .add_systems(OnExit(Phase::Title), start_new_game)
        .add_systems(OnEnter(Phase::Intro), refill_ship);
}

/// A fresh game (leaving the title): back to level 1 with a cleared score, camera
/// and ship rewound to the very bottom. The ship is despawned so `spawn_player`
/// makes a fresh one at the start; later levels keep flying the same ship.
fn start_new_game(
    mut commands: Commands,
    mut level: ResMut<Level>,
    mut progress: ResMut<GameProgress>,
    mut weapon: ResMut<Weapon>,
    mut camera: Query<&mut Transform, (With<Camera2d>, Without<Player>)>,
    players: Query<Entity, With<Player>>,
) {
    level.0 = 1;
    progress.score = 0;
    *weapon = Weapon::Single;
    for entity in &players {
        commands.entity(entity).try_despawn();
    }
    if let Ok(mut transform) = camera.single_mut() {
        transform.translation.x = 0.0;
        transform.translation.y = CAMERA_START_Y;
    }
}

/// At every level start, top up health and clear game-over (score carries over).
///
/// Leftover combat entities are deliberately NOT despawned here: movement and
/// off-screen cleanup keep running outside combat (see `combat`/`ufo`), so craft
/// from the previous fight fly off and expire naturally instead of vanishing.
fn refill_ship(mut progress: ResMut<GameProgress>) {
    progress.health = MAX_HEALTH;
    progress.game_over = false;
}
