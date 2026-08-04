//! Projectile movement, collisions, score/lives, and restart.

use bevy::prelude::*;

use crate::animation::ExplosionMessage;
use crate::ufo::Ufo;
use crate::assets::GameAssets;
use crate::common::{
    Collider, Velocity, X_BOUND, Y_BOUND, overlaps, sfx,
};
use crate::enemy::Enemy;
use crate::player::{Invincible, Player};
use crate::state::{Phase, live};

// ----- Player bullets ------------------------------------------------------
pub const BULLET_SPEED: f32 = 300.0;

// (Enemy/tank bullet speed now scales with the level — see `Level::difficulty`.)

/// Total health = 3 end/cap segments + 10 tiled `healthbar_repeat` segments.
pub const MAX_HEALTH: u32 = 13;

/// Money awarded for destroying an enemy aircraft.
pub const KILL_REWARD: u32 = 100;

/// A projectile fired by the player.
#[derive(Component)]
pub struct PlayerBullet;

/// A projectile fired by an enemy.
#[derive(Component)]
pub struct EnemyBullet;

/// Emitted when the player *shoots down* an enemy (not on ram/collision), at its
/// world position. `pickup` listens for these to occasionally drop a pickup.
#[derive(Message)]
pub struct EnemyKilled {
    pub position: Vec2,
}

#[derive(Resource)]
pub struct GameProgress {
    pub score: u32,
    pub health: u32,
    pub game_over: bool,
}

impl Default for GameProgress {
    fn default() -> Self {
        Self {
            score: 0,
            health: MAX_HEALTH,
            game_over: false,
        }
    }
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<GameProgress>()
        .add_message::<EnemyKilled>()
        // Movement and off-screen cleanup keep running outside combat (intro
        // cruise, dialogue): craft left over from a fight fly off and despawn
        // naturally instead of vanishing when a level ends. `live` freezes them
        // instantly on game over.
        .add_systems(Update, (apply_velocity, despawn_offscreen).run_if(live))
        .add_systems(
            Update,
            (bullet_hits_enemy, enemy_hits_player, enemy_bullet_hits_player)
                .run_if(in_state(Phase::Combat))
                .run_if(live),
        )
        // These two must stay active while the game-over screen is frozen.
        .add_systems(
            Update,
            (game_over_sound, return_to_title).run_if(in_state(Phase::Combat)),
        );
}

/// Plays the game-over jingle once when health runs out (resets on restart).
fn game_over_sound(
    mut commands: Commands,
    progress: Res<GameProgress>,
    assets: Res<GameAssets>,
    mut played: Local<bool>,
) {
    if progress.game_over {
        if !*played {
            commands.spawn(sfx(assets.game_over_sfx.clone()));
            *played = true;
        }
    } else {
        *played = false;
    }
}

fn apply_velocity(time: Res<Time>, mut query: Query<(&Velocity, &mut Transform)>) {
    let dt = time.delta_secs();
    for (velocity, mut transform) in &mut query {
        transform.translation.x += velocity.0.x * dt;
        transform.translation.y += velocity.0.y * dt;
    }
}

fn despawn_offscreen(
    mut commands: Commands,
    camera: Query<&Transform, With<Camera2d>>,
    query: Query<(Entity, &Transform), With<Velocity>>,
) {
    let Ok(camera) = camera.single() else {
        return;
    };
    let cam = camera.translation.truncate();
    let margin = 60.0;
    for (entity, transform) in &query {
        let p = transform.translation.truncate();
        if (p.x - cam.x).abs() > X_BOUND + margin || (p.y - cam.y).abs() > Y_BOUND + margin {
            commands.entity(entity).try_despawn();
        }
    }
}

fn bullet_hits_enemy(
    mut commands: Commands,
    mut progress: ResMut<GameProgress>,
    mut explosions: MessageWriter<ExplosionMessage>,
    mut kills: MessageWriter<EnemyKilled>,
    bullets: Query<(Entity, &Transform, &Collider), With<PlayerBullet>>,
    enemies: Query<(Entity, &Transform, &Collider, Has<Ufo>), With<Enemy>>,
) {
    for (bullet, bt, bc) in &bullets {
        for (enemy, et, ec, is_ufo) in &enemies {
            if overlaps(
                bt.translation.truncate(),
                bc.0,
                et.translation.truncate(),
                ec.0,
            ) {
                let position = et.translation.truncate();
                commands.entity(bullet).try_despawn();
                commands.entity(enemy).try_despawn();
                progress.score += KILL_REWARD;
                // UFOs go up in their own blast (`explosion_2.png`).
                explosions.write(if is_ufo {
                    ExplosionMessage::ufo(position)
                } else {
                    ExplosionMessage::at(position)
                });
                kills.write(EnemyKilled { position });
                break; // this bullet is gone; stop checking it against other enemies
            }
        }
    }
}

fn enemy_hits_player(
    mut commands: Commands,
    mut progress: ResMut<GameProgress>,
    mut explosions: MessageWriter<ExplosionMessage>,
    enemies: Query<(Entity, &Transform, &Collider, Has<Ufo>), With<Enemy>>,
    player: Query<(&Transform, &Collider, Has<Invincible>), With<Player>>,
) {
    let Ok((pt, pc, invincible)) = player.single() else {
        return;
    };
    for (enemy, et, ec, is_ufo) in &enemies {
        // Only a collision with the ship costs health; enemies that slip past the
        // bottom are cleaned up harmlessly by `despawn_offscreen`.
        if overlaps(
            pt.translation.truncate(),
            pc.0,
            et.translation.truncate(),
            ec.0,
        ) {
            let position = et.translation.truncate();
            explosions.write(if is_ufo {
                ExplosionMessage::ufo(position)
            } else {
                ExplosionMessage::at(position)
            });
            commands.entity(enemy).try_despawn();
            // Invincible: the rammed craft still explodes, but costs no health.
            if !invincible {
                progress.damage();
            }
        }
    }
}

fn enemy_bullet_hits_player(
    mut commands: Commands,
    mut progress: ResMut<GameProgress>,
    bullets: Query<(Entity, &Transform, &Collider), With<EnemyBullet>>,
    player: Query<(&Transform, &Collider, Has<Invincible>), With<Player>>,
) {
    let Ok((pt, pc, invincible)) = player.single() else {
        return;
    };
    // Invincible: enemy fire passes straight through the ship.
    if invincible {
        return;
    }
    for (bullet, bt, bc) in &bullets {
        if overlaps(
            pt.translation.truncate(),
            pc.0,
            bt.translation.truncate(),
            bc.0,
        ) {
            commands.entity(bullet).try_despawn();
            progress.damage();
        }
    }
}

impl GameProgress {
    /// Take one point of damage; triggers game over at zero health.
    pub fn damage(&mut self) {
        if self.game_over {
            return;
        }
        self.health = self.health.saturating_sub(1);
        if self.health == 0 {
            self.game_over = true;
        }
    }
}

/// After a game over, Space/Enter returns to the title screen. Starting a new
/// game from there resets everything via `level::start_new_game` (level 1,
/// cleared score/weapon, fresh ship, rewound camera).
fn return_to_title(
    keys: Res<ButtonInput<KeyCode>>,
    progress: Res<GameProgress>,
    mut next: ResMut<NextState<Phase>>,
) {
    if progress.game_over
        && (keys.just_pressed(KeyCode::Space) || keys.just_pressed(KeyCode::Enter))
    {
        next.set(Phase::Title);
    }
}
