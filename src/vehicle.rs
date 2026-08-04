//! Moving ground vehicles: tanks and trucks.
//!
//! * **Tanks** are single (animated) sprites that patrol back and forth along the
//!   road, flipping to face their heading ([`Patrol`]). A tank turns back when it
//!   reaches a road [`RoadBlocks`] marker on its row; a side with no block falls
//!   back to its `Range` object property (or [`DEFAULT_PATROL_HALF_RANGE`]). It
//!   also fires a spinning shell down at the player (see `tank_fire`).
//! * **Trucks** are assembled from several tiles under one parent (see
//!   `tilemap::spawn_trucks`) and simply drive one way; when they reach a block (or
//!   the end of their run) they vanish and restart from the beginning ([`Convoy`]).
//!
//! `tilemap.rs` tags each vehicle with the appropriate component at map load and
//! collects the `Block=true` tiles into [`RoadBlocks`].

use std::time::Duration;

use bevy::prelude::*;

use crate::animation::SpriteSheetAnimation;
use crate::assets::GameAssets;
use crate::combat::{EnemyBullet, GameProgress};
use crate::level::Level;
use crate::common::{Collider, Velocity, X_BOUND, Y_BOUND};
use crate::state::{Phase, live};

/// Fallback patrol half-range (world px) for tanks whose map object has no `Range`.
pub const DEFAULT_PATROL_HALF_RANGE: f32 = 48.0;
/// Tank patrol speed (world px/s) — a slow, deliberate crawl.
const PATROL_SPEED: f32 = 26.0;

/// Trucks cruise forward at this speed and loop after driving this far.
const CONVOY_SPEED: f32 = 30.0;
const CONVOY_SPAN: f32 = 200.0;

/// Tanks shoot down at the player (who approaches from below). How often each
/// tank fires, the shell's drawn size / (smaller) hit box, and how far below the
/// tank's center it emerges (its barrel).
const TANK_FIRE_INTERVAL: f32 = 1.6;
const TANK_BULLET_DISPLAY: Vec2 = Vec2::splat(14.0);
const TANK_BULLET_COLLIDER: Vec2 = Vec2::splat(9.0);
const TANK_MUZZLE_DOWN: f32 = 8.0;

/// One map tile (world px). A vehicle stops flush against a block one tile from
/// the block's center (each is 16px wide, so their edges meet).
const TILE: f32 = 16.0;
/// A block counts as "on this vehicle's road" when their centers are within this
/// vertical distance (blocks and road vehicles share the same row, so this only
/// needs to be under one tile to exclude neighbouring rows).
const BLOCK_ROW_TOL: f32 = 8.0;

/// World positions of the road "Block" markers (tiles tagged `Block=true` in the
/// TMX). Vehicles turn back / reset when they reach one on their road. Filled by
/// `tilemap::spawn_map` at startup.
#[derive(Resource, Default)]
pub struct RoadBlocks(pub Vec<Vec2>);

/// A tank that patrols back and forth within `half_range` px of `home_x`.
#[derive(Component)]
pub struct Patrol {
    home_x: f32,
    /// How far (world px) it strays from `home_x` before turning around.
    half_range: f32,
    /// Current heading: `+1.0` right, `-1.0` left.
    dir: f32,
}

impl Patrol {
    /// A tank patrolling `half_range` px around `home_x`, starting toward `dir`.
    pub fn new(home_x: f32, half_range: f32, dir: f32) -> Self {
        Self {
            home_x,
            half_range,
            dir,
        }
    }
}

/// A tank's gun: a repeating cooldown between shots (fired straight down).
#[derive(Component)]
pub struct TankGun {
    timer: Timer,
}

impl TankGun {
    /// A tank gun on the shared fire cooldown, offset by `phase` (0..1 of the
    /// cycle) so tanks placed at the same time don't all fire in lockstep.
    pub fn new(phase: f32) -> Self {
        let mut timer = Timer::from_seconds(TANK_FIRE_INTERVAL, TimerMode::Repeating);
        timer.tick(Duration::from_secs_f32(TANK_FIRE_INTERVAL * phase.clamp(0.0, 1.0)));
        Self { timer }
    }
}

/// A truck that drives right from `start_x`, looping back to it once it has
/// travelled [`CONVOY_SPAN`] (or sooner, if it reaches a block). The art already
/// faces right, so it never flips.
#[derive(Component)]
pub struct Convoy {
    start_x: f32,
    /// Half the assembled truck's width (world px) — its front edge is this far
    /// ahead of `start_x`, used to detect when the front reaches a block.
    half_width: f32,
}

impl Convoy {
    pub fn new(start_x: f32, half_width: f32) -> Self {
        Self {
            start_x,
            half_width,
        }
    }
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<RoadBlocks>()
        // Vehicles keep driving through level transitions and dialogue (they're
        // scenery in motion), freezing only on game over; the tank guns are
        // limited to combat.
        .add_systems(Update, (patrol_tanks, drive_convoys).run_if(live))
        .add_systems(Update, tank_fire.run_if(in_state(Phase::Combat)));
}

/// Bundle for a tank shell fired straight down. It reuses [`EnemyBullet`] so the
/// existing `combat::enemy_bullet_hits_player` handles the damage, and plays the
/// spinning `projectile_1` animation (direction-agnostic, so no flip needed).
fn tank_bullet(
    image: Handle<Image>,
    frames: Vec<(Rect, f32)>,
    pos: Vec2,
    speed: f32,
) -> impl Bundle {
    let animation = SpriteSheetAnimation::new(frames, true, false);
    let rect = animation.first_rect();
    (
        Sprite {
            image,
            custom_size: Some(TANK_BULLET_DISPLAY),
            rect,
            ..default()
        },
        Transform::from_xyz(pos.x, pos.y, 5.0),
        EnemyBullet,
        Velocity(Vec2::new(0.0, -speed)),
        Collider(TANK_BULLET_COLLIDER),
        animation,
    )
}

/// On its cooldown, each on-screen tank fires a shell straight down at the player.
fn tank_fire(
    mut commands: Commands,
    time: Res<Time>,
    progress: Res<GameProgress>,
    assets: Res<GameAssets>,
    level: Res<Level>,
    camera: Query<&Transform, With<Camera2d>>,
    mut tanks: Query<(&GlobalTransform, &mut TankGun)>,
) {
    if progress.game_over {
        return;
    }
    let Ok(camera) = camera.single() else {
        return;
    };
    let cam = camera.translation.truncate();
    let difficulty = level.difficulty();
    for (transform, mut gun) in &mut tanks {
        if !gun.timer.tick(time.delta()).just_finished() {
            continue;
        }
        // Tanks on later levels reload faster; effective from this cycle on.
        gun.timer.set_duration(Duration::from_secs_f32(
            difficulty.tank_fire_interval,
        ));
        // Only fire while on-screen, so tanks far down the map don't spray shots
        // that would just despawn off-view.
        let pos = transform.translation().truncate();
        if (pos.x - cam.x).abs() > X_BOUND || (pos.y - cam.y).abs() > Y_BOUND {
            continue;
        }
        let origin = pos - Vec2::new(0.0, TANK_MUZZLE_DOWN);
        commands.spawn(tank_bullet(
            assets.projectile.clone(),
            assets.projectile_frames.clone(),
            origin,
            difficulty.enemy_bullet_speed,
        ));
    }
}

fn patrol_tanks(
    time: Res<Time>,
    blocks: Res<RoadBlocks>,
    mut tanks: Query<(&mut Transform, &mut Sprite, &mut Patrol)>,
) {
    let dt = time.delta_secs();
    for (mut transform, mut sprite, mut tank) in &mut tanks {
        transform.translation.x += tank.dir * PATROL_SPEED * dt;

        // Patrol limits: a block on the road is the turn-back point (it overrides
        // the default range, in or out); a side with no block keeps `half_range`.
        let y = transform.translation.y;
        let mut min = tank.home_x - tank.half_range;
        let mut max = tank.home_x + tank.half_range;
        let (mut has_left, mut has_right) = (false, false);
        for b in &blocks.0 {
            if (b.y - y).abs() > BLOCK_ROW_TOL {
                continue;
            }
            if b.x < tank.home_x {
                // Nearest left block (largest x) wins; stop flush against it.
                let wall = b.x + TILE;
                min = if has_left { min.max(wall) } else { wall };
                has_left = true;
            } else if b.x > tank.home_x {
                // Nearest right block (smallest x) wins.
                let wall = b.x - TILE;
                max = if has_right { max.min(wall) } else { wall };
                has_right = true;
            }
        }

        // Reverse at either end of the patrol, clamping so it can't drift out.
        if transform.translation.x >= max {
            transform.translation.x = max;
            tank.dir = -1.0;
        } else if transform.translation.x <= min {
            transform.translation.x = min;
            tank.dir = 1.0;
        }

        // Face the way it's driving (art points right by default).
        sprite.flip_x = tank.dir < 0.0;
    }
}

fn drive_convoys(
    time: Res<Time>,
    blocks: Res<RoadBlocks>,
    mut trucks: Query<(&mut Transform, &Convoy)>,
) {
    let dt = time.delta_secs();
    for (mut transform, convoy) in &mut trucks {
        transform.translation.x += CONVOY_SPEED * dt;

        // A block ahead on this road makes the truck vanish and restart: reset
        // once the truck's front edge reaches the block's near (left) edge.
        let y = transform.translation.y;
        let front = transform.translation.x + convoy.half_width;
        let blocked = blocks.0.iter().any(|b| {
            (b.y - y).abs() < BLOCK_ROW_TOL && b.x > convoy.start_x && front >= b.x - TILE / 2.0
        });

        // Restart from the beginning at the end of the run (or when blocked).
        if blocked || transform.translation.x - convoy.start_x >= CONVOY_SPAN {
            transform.translation.x = convoy.start_x;
        }
    }
}
