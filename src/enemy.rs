//! Enemies: periodic spawning, downward movement, and occasional return fire.

use bevy::prelude::*;
use rand::Rng;

use crate::assets::{BulletArt, GameAssets};
use crate::combat::{EnemyBullet, GameProgress};
use crate::common::{Collider, HITBOX_SCALE, Velocity, X_BOUND, Y_BOUND};
use crate::level::Level;
use crate::player::Player;
use crate::state::Phase;

// Enemy ships and their bullets are drawn at their art's native size (the
// camera applies the zoom, like every other world sprite). Descent speed
// scales with the level — see `Difficulty::enemy_speed`.
/// Initial spawn cadence; from then on the timer follows the current level's
/// [`Difficulty`] (spawn rate, gun timing, and bullet speed all scale with it).
const ENEMY_SPAWN_INTERVAL: f32 = 0.75;
/// Widest slant of an aimed shot from straight down (radians, ~30 deg per side).
const MAX_AIM_ANGLE: f32 = 0.52;

#[derive(Component)]
pub struct Enemy;

/// An aircraft's gun: the projectile art its ship model fires (a fixed trait of
/// the model — see `assets::ENEMY_SHIPS`) and the countdown to its next shot.
#[derive(Component)]
struct Gun {
    bullet: BulletArt,
    timer: Timer,
}

#[derive(Resource)]
struct EnemySpawnTimer(Timer);

pub(super) fn plugin(app: &mut App) {
    app.insert_resource(EnemySpawnTimer(Timer::from_seconds(
        ENEMY_SPAWN_INTERVAL,
        TimerMode::Repeating,
    )))
    .add_systems(
        Update,
        (spawn_enemies, enemy_fire).run_if(in_state(Phase::Combat)),
    );
}

fn spawn_enemies(
    mut commands: Commands,
    time: Res<Time>,
    progress: Res<GameProgress>,
    assets: Res<GameAssets>,
    level: Res<Level>,
    mut timer: ResMut<EnemySpawnTimer>,
    camera: Query<&Transform, With<Camera2d>>,
) {
    if progress.game_over {
        return;
    }
    timer.0.tick(time.delta());
    if !timer.0.just_finished() {
        return;
    }
    let difficulty = level.difficulty();
    // Later levels spawn faster; the new duration takes effect from this cycle on.
    timer
        .0
        .set_duration(std::time::Duration::from_secs_f32(
            difficulty.enemy_spawn_interval,
        ));
    let Ok(camera) = camera.single() else {
        return;
    };
    if assets.enemy_ships.is_empty() {
        return;
    }
    let cam = camera.translation.truncate();

    // Pick a random enemy ship, drawn at its native size.
    let mut rng = rand::thread_rng();
    let ship = assets.enemy_ships[rng.gen_range(0..assets.enemy_ships.len())].clone();
    let size = ship.size;

    // Enter from just above the top of the current view.
    let x = cam.x + rng.gen_range(-X_BOUND + size.x..X_BOUND - size.x);
    let y = cam.y + Y_BOUND + size.y;
    let drift = rng.gen_range(-40.0..40.0);

    // Each aircraft is armed with its model's bullet style, and a gun timer
    // that guarantees a first shot shortly after it enters the view.
    let gun = Gun {
        bullet: ship.bullet.clone(),
        timer: Timer::from_seconds(
            rng.gen_range(difficulty.enemy_first_shot),
            TimerMode::Once,
        ),
    };

    commands
        .spawn((
            enemy(
                ship.image,
                size,
                Vec2::new(x, y),
                Vec2::new(drift, -difficulty.enemy_speed),
            ),
            gun,
        ))
        .with_children(|craft| {
            // Thruster flame at the tail. The craft is flipped to face down, so
            // its tail is on top and the flame flips with it.
            craft.spawn(thruster_flame(&ship.flame, 1.0, size.y, true));
        });
}

/// Bundle for a ship's animated thruster flame (a child of the ship), scaled
/// like its ship and aligned to the tail: it starts at the ship sprite's rear
/// edge (`ship_draw_height` is the ship's on-screen height). `flipped` matches
/// ships drawn facing down, whose tail is on top.
pub fn thruster_flame(
    flame: &crate::assets::FlameArt,
    scale: f32,
    ship_draw_height: f32,
    flipped: bool,
) -> impl Bundle {
    let animation = crate::animation::SpriteSheetAnimation::new(flame.frames(), true, false);
    let rect = animation.first_rect();
    let size = flame.size * scale;
    // Both sprites are drawn centered on their origins: the tail edge sits half
    // a ship from the parent, the flame's center half a flame further.
    let offset = (ship_draw_height + size.y) / 2.0;
    (
        Sprite {
            image: flame.image.clone(),
            custom_size: Some(size),
            rect,
            flip_y: flipped,
            ..default()
        },
        Transform::from_xyz(0.0, if flipped { offset } else { -offset }, -0.1),
        animation,
    )
}

/// Bundle for an enemy ship descending at `velocity`. The collider is shrunk
/// below the sprite size so contact matches the visible craft, not its padding.
fn enemy(image: Handle<Image>, size: Vec2, pos: Vec2, velocity: Vec2) -> impl Bundle {
    (
        Sprite {
            image,
            custom_size: Some(size),
            flip_y: true, // ship art points up; enemies face down toward the player
            ..default()
        },
        Transform::from_xyz(pos.x, pos.y, 5.0),
        Enemy,
        Velocity(velocity),
        Collider(size * HITBOX_SCALE),
    )
}

/// Bundle for an enemy bullet fired from `pos` along `velocity` (aimed at the
/// player when it left the muzzle), in the firing ship's own style. The sprite
/// is NOT rotated toward its travel direction — it stays drawn pointing
/// straight down while the trajectory slants toward the player.
fn enemy_bullet(art: &BulletArt, pos: Vec2, velocity: Vec2) -> impl Bundle {
    (
        Sprite {
            image: art.image.clone(),
            custom_size: Some(art.size),
            flip_y: true, // the art points up (trail below); drawn firing downward
            ..default()
        },
        Transform::from_xyz(pos.x, pos.y, 5.0),
        EnemyBullet,
        Velocity(velocity),
        Collider(art.collider),
    )
}

#[allow(clippy::too_many_arguments)]
fn enemy_fire(
    mut commands: Commands,
    time: Res<Time>,
    progress: Res<GameProgress>,
    level: Res<Level>,
    camera: Query<&Transform, (With<Camera2d>, Without<Enemy>)>,
    player: Query<&Transform, (With<Player>, Without<Enemy>)>,
    // `Gun` is only on armed aircraft, so UFOs (which ram, never shoot) fall out
    // of this query naturally.
    mut query: Query<(&Transform, &Sprite, &mut Gun), With<Enemy>>,
) {
    if progress.game_over {
        return;
    }
    let Ok(camera) = camera.single() else {
        return;
    };
    let cam = camera.translation.truncate();
    let player_pos = player.single().ok().map(|t| t.translation.truncate());
    let difficulty = level.difficulty();
    let mut rng = rand::thread_rng();
    for (transform, sprite, mut gun) in &mut query {
        if !gun.timer.tick(time.delta()).is_finished() {
            continue;
        }
        // Hold fire until the plane is actually in view (it spawns just above it);
        // the timer stays finished, so it shoots the moment it enters.
        let pos = transform.translation.truncate();
        if (pos.x - cam.x).abs() > X_BOUND || (pos.y - cam.y).abs() > Y_BOUND {
            continue;
        }
        // Fire from the visual bottom of the ship, in this plane's own style,
        // aimed at where the player is right now (a snapshot, not homing — the
        // shot can be dodged). No player -> fall back to straight down.
        let half_height = sprite.custom_size.map(|s| s.y / 2.0).unwrap_or(0.0);
        let origin = pos - Vec2::new(0.0, half_height);
        let dir = player_pos
            .map(|p| (p - origin).normalize_or_zero())
            .filter(|d| *d != Vec2::ZERO)
            .unwrap_or(Vec2::NEG_Y);
        // Keep the shot inside a forward cone: a nose gun can lead the target a
        // little, but firing sideways (or backwards, with the player alongside
        // or above the plane) looked unnatural.
        let slant = Vec2::NEG_Y
            .angle_to(dir)
            .clamp(-MAX_AIM_ANGLE, MAX_AIM_ANGLE);
        let dir = Vec2::from_angle(slant - std::f32::consts::FRAC_PI_2);
        commands.spawn(enemy_bullet(
            &gun.bullet,
            origin,
            dir * difficulty.enemy_bullet_speed,
        ));
        // Rearm on this plane's own cadence.
        let refire = rng.gen_range(difficulty.enemy_refire.clone());
        gun.timer.set_duration(std::time::Duration::from_secs_f32(refire));
        gun.timer.reset();
    }
}
