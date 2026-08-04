//! The player's ship: spawning, movement, weapon selection, and firing.

use bevy::prelude::*;

use crate::assets::{BulletArt, GameAssets};
use crate::combat::{BULLET_SPEED, GameProgress, PlayerBullet};
use crate::common::{Collider, HITBOX_SCALE, PLAYER_START, Velocity, X_BOUND, Y_BOUND, sfx};
use crate::menu::SelectedFighter;
use crate::state::{Phase, live};

/// How much the chosen ship's native pixels are magnified when drawn in-game
/// (e.g. spaceship_0 at 24x24 native -> 33x33; sizes come from the PNG headers).
const PLAYER_DRAW_SCALE: f32 = 1.375;
const PLAYER_SPEED: f32 = 420.0;
const PLAYER_FIRE_COOLDOWN: f32 = 0.18;

/// Invincibility pickup: how long the ship is immune to all damage.
const INVINCIBLE_SECS: f32 = 5.0;
/// While invincible the ship blinks at this rate (toggling `Visibility` — a
/// color tint would knock the ship's pixels off the palette-grade table).
const INVINCIBLE_BLINK_HZ: f32 = 5.0;



#[derive(Component)]
pub struct Player;

/// Temporary invincibility from a pickup: the ship takes no damage from enemy
/// bullets, rams, or boss beams while the timer runs (it blinks meanwhile).
#[derive(Component)]
pub struct Invincible(Timer);

impl Invincible {
    pub fn new() -> Self {
        Self(Timer::from_seconds(INVINCIBLE_SECS, TimerMode::Once))
    }
}

#[derive(Resource)]
struct FireCooldown(Timer);

#[derive(Resource, Clone, Copy, PartialEq, Eq)]
pub enum Weapon {
    Single,
    Double,
    Triple,
}

pub(super) fn plugin(app: &mut App) {
    app.insert_resource(Weapon::Single)
        .insert_resource(FireCooldown(Timer::from_seconds(
            PLAYER_FIRE_COOLDOWN,
            TimerMode::Once,
        )))
        // Spawn the chosen fighter when the intro (and thus the level) begins.
        .add_systems(OnEnter(Phase::Intro), spawn_player)
        .add_systems(
            Update,
            (
                (player_movement, player_fire).run_if(in_state(Phase::Combat)),
                // Runs under `live` (not just Combat): the blink toggles
                // Visibility, so if it stopped on a Hidden frame when a boss
                // kill enters Intro (or a dialogue opens), the ship would stay
                // invisible until combat resumes.
                tick_invincible.run_if(live),
            ),
        );
}

/// Blink the ship while invincible; restore it and expire when the timer ends.
fn tick_invincible(
    mut commands: Commands,
    time: Res<Time>,
    mut shielded: Query<(Entity, &mut Invincible, &mut Visibility)>,
) {
    for (entity, mut shield, mut visibility) in &mut shielded {
        if shield.0.tick(time.delta()).is_finished() {
            commands.entity(entity).remove::<Invincible>();
            *visibility = Visibility::Visible;
        } else {
            let phase = (shield.0.elapsed_secs() * INVINCIBLE_BLINK_HZ * 2.0) as u32;
            *visibility = if phase.is_multiple_of(2) {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
    }
}

fn spawn_player(
    mut commands: Commands,
    assets: Res<GameAssets>,
    selected: Res<SelectedFighter>,
    existing: Query<(), With<Player>>,
) {
    // The ship flies the whole continuous map, so keep it across level starts;
    // only spawn one when there isn't one yet (a fresh game clears it first).
    if !existing.is_empty() {
        return;
    }
    let fighter = assets.fighter(selected.0);
    let size = fighter.size * PLAYER_DRAW_SCALE;
    commands
        .spawn(player(fighter.image.clone(), size, PLAYER_START))
        .with_children(|ship| {
            // Animated thruster flame at the tail (the ship faces up).
            ship.spawn(crate::enemy::thruster_flame(
                &fighter.flame,
                PLAYER_DRAW_SCALE,
                size.y,
                false,
            ));
        });
}

/// Bundle for the player's ship. Drawn at `size`, but the collider is shrunk so
/// contact matches the visible craft rather than the sprite's padded box.
fn player(image: Handle<Image>, size: Vec2, pos: Vec2) -> impl Bundle {
    (
        Sprite {
            image,
            custom_size: Some(size),
            ..default()
        },
        Transform::from_xyz(pos.x, pos.y, 0.0),
        Player,
        Collider(size * HITBOX_SCALE),
    )
}

/// Bundle for a player bullet (the `projectile_2` energy bolt, drawn at native
/// size pointing up) travelling at `velocity`.
fn missile(art: &BulletArt, pos: Vec2, velocity: Vec2) -> impl Bundle {
    (
        Sprite {
            image: art.image.clone(),
            custom_size: Some(art.size),
            ..default()
        },
        Transform::from_xyz(pos.x, pos.y, 1.0),
        PlayerBullet,
        Velocity(velocity),
        Collider(art.collider),
    )
}

fn player_movement(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    progress: Res<GameProgress>,
    camera: Query<&Transform, (With<Camera2d>, Without<Player>)>,
    mut query: Query<(&mut Transform, &Sprite), With<Player>>,
) {
    if progress.game_over {
        return;
    }
    let Ok((mut transform, sprite)) = query.single_mut() else {
        return;
    };
    let speed = PLAYER_SPEED;

    let mut dir = Vec2::ZERO;
    if keys.any_pressed([KeyCode::ArrowLeft, KeyCode::KeyA]) {
        dir.x -= 1.0;
    }
    if keys.any_pressed([KeyCode::ArrowRight, KeyCode::KeyD]) {
        dir.x += 1.0;
    }
    if keys.any_pressed([KeyCode::ArrowUp, KeyCode::KeyW]) {
        dir.y += 1.0;
    }
    if keys.any_pressed([KeyCode::ArrowDown, KeyCode::KeyS]) {
        dir.y -= 1.0;
    }

    if dir != Vec2::ZERO {
        let delta = dir.normalize() * speed * time.delta_secs();
        transform.translation.x += delta.x;
        transform.translation.y += delta.y;
    }

    // Keep the ship inside the on-screen viewport (the camera scrolls up and
    // follows the ship horizontally, so clamp x/y around the camera's position).
    // Clamp by the *visual* size, not the (smaller) collider, so it stays fully
    // on-screen.
    let visual = sprite.custom_size.unwrap_or(Vec2::ZERO);
    let cam = camera
        .single()
        .map(|c| c.translation.truncate())
        .unwrap_or(Vec2::ZERO);
    let hx = X_BOUND - visual.x / 2.0;
    let hy = Y_BOUND - visual.y / 2.0;
    transform.translation.x = transform.translation.x.clamp(cam.x - hx, cam.x + hx);
    transform.translation.y = transform.translation.y.clamp(cam.y - hy, cam.y + hy);
}

#[allow(clippy::too_many_arguments)]
fn player_fire(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    weapon: Res<Weapon>,
    progress: Res<GameProgress>,
    assets: Res<GameAssets>,
    mut cooldown: ResMut<FireCooldown>,
    query: Query<(&Transform, &Sprite), With<Player>>,
) {
    if progress.game_over {
        return;
    }
    cooldown.0.tick(time.delta());

    if !keys.pressed(KeyCode::Space) || !cooldown.0.is_finished() {
        return;
    }
    let Ok((transform, sprite)) = query.single() else {
        return;
    };
    cooldown.0.reset();

    // Fire from just past the visual nose of the ship: the bullet's bottom
    // edge starts at the ship sprite's top edge (adjacent, not overlapping —
    // both sprites are drawn centered on their origins).
    let half_height = sprite.custom_size.map(|s| s.y / 2.0).unwrap_or(0.0);
    let origin = transform.translation.truncate()
        + Vec2::new(0.0, half_height + assets.player_bullet.size.y / 2.0);

    // Each weapon fires a different spread of bullet velocities.
    let velocities: &[Vec2] = match *weapon {
        Weapon::Single => &[Vec2::new(0.0, BULLET_SPEED)],
        Weapon::Double => &[
            Vec2::new(-0.15 * BULLET_SPEED, BULLET_SPEED),
            Vec2::new(0.15 * BULLET_SPEED, BULLET_SPEED),
        ],
        Weapon::Triple => &[
            Vec2::new(-0.35 * BULLET_SPEED, 0.94 * BULLET_SPEED),
            Vec2::new(0.0, BULLET_SPEED),
            Vec2::new(0.35 * BULLET_SPEED, 0.94 * BULLET_SPEED),
        ],
    };

    for &v in velocities {
        commands.spawn(missile(&assets.player_bullet, origin, v));
    }

    // Fire sound (one shot per volley; despawns when finished).
    commands.spawn(sfx(assets.laser.clone()));
}
