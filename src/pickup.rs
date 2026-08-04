//! Collectible pickups dropped by defeated enemies.
//!
//! `pickups.png` holds four 16x16 icons, each mapped to a [`PickupKind`] with a
//! distinct effect (color-coded via the palette recolor in `assets.rs`):
//!   * **Invincible** (boots, yellow) — 5s of immunity to all damage.
//!   * **Repair** (orb, green) — restores some health.
//!   * **Weapon** (arrow, red) — upgrades the weapon one step.
//!   * **Bomb** (teardrop, blue) — destroys every enemy on screen.
//!
//! When the player shoots an enemy down there's a chance one drops
//! (`spawn_drops`, reacting to `combat::EnemyKilled`). Pickups drift down with the
//! scrolling world (they carry a [`Velocity`], so `combat`'s `apply_velocity` moves
//! them and `despawn_offscreen` cleans them up) and are collected on contact.

use bevy::prelude::*;
use rand::Rng;

use crate::animation::ExplosionMessage;
use crate::assets::GameAssets;
use crate::combat::{EnemyKilled, GameProgress, KILL_REWARD, MAX_HEALTH};
use crate::common::{Collider, Velocity, overlaps, sfx};
use crate::enemy::Enemy;
use crate::player::{Invincible, Player, Weapon};
use crate::state::Phase;

/// Chance that shooting down an enemy drops a pickup.
const DROP_CHANCE: f64 = 0.4;
/// Health restored by a repair pickup (capped at [`MAX_HEALTH`]).
const REPAIR_AMOUNT: u32 = 4;
/// Pickup sprite size (native px) and downward drift speed (world px/s).
const PICKUP_SIZE: Vec2 = Vec2::splat(16.0);
const PICKUP_FALL_SPEED: f32 = 40.0;

/// The four pickup kinds. Order matches the frames in `pickups.png` and the
/// `PICKUP_COLORS` theme colors in `assets.rs`.
#[derive(Clone, Copy)]
pub enum PickupKind {
    Invincible,
    Repair,
    Weapon,
    Bomb,
}

impl PickupKind {
    const ALL: [PickupKind; 4] = [
        PickupKind::Invincible,
        PickupKind::Repair,
        PickupKind::Weapon,
        PickupKind::Bomb,
    ];

    fn index(self) -> usize {
        match self {
            PickupKind::Invincible => 0,
            PickupKind::Repair => 1,
            PickupKind::Weapon => 2,
            PickupKind::Bomb => 3,
        }
    }

    /// Source rect of this pickup's 16x16 frame within its (recolored) strip.
    fn frame(self) -> Rect {
        let x = self.index() as f32 * PICKUP_SIZE.x;
        Rect {
            min: Vec2::new(x, 0.0),
            max: Vec2::new(x + PICKUP_SIZE.x, PICKUP_SIZE.y),
        }
    }
}

/// Fired when a bomb pickup detonates. The planes' wipe happens right in
/// `collect_pickups`; the boss listens separately and takes ONE point of damage
/// (never the full wipe — the boss fight must stay a fight).
#[derive(Message)]
pub struct BombBlast;

#[derive(Component)]
pub struct Pickup {
    kind: PickupKind,
}

pub(super) fn plugin(app: &mut App) {
    app.add_message::<BombBlast>().add_systems(
        Update,
        (spawn_drops, collect_pickups).run_if(in_state(Phase::Combat)),
    );
}

/// Occasionally drop a random pickup where an enemy was shot down.
fn spawn_drops(
    mut commands: Commands,
    assets: Res<GameAssets>,
    mut kills: MessageReader<EnemyKilled>,
) {
    let mut rng = rand::thread_rng();
    for kill in kills.read() {
        if rng.gen_bool(DROP_CHANCE) {
            let kind = PickupKind::ALL[rng.gen_range(0..PickupKind::ALL.len())];
            commands.spawn(pickup(&assets, kind, kill.position));
        }
    }
}

/// Bundle for a pickup drifting downward from `pos`.
fn pickup(assets: &GameAssets, kind: PickupKind, pos: Vec2) -> impl Bundle {
    (
        Sprite {
            image: assets.pickups[kind.index()].clone(),
            custom_size: Some(PICKUP_SIZE),
            rect: Some(kind.frame()),
            ..default()
        },
        Transform::from_xyz(pos.x, pos.y, 4.0),
        Pickup { kind },
        Velocity(Vec2::new(0.0, -PICKUP_FALL_SPEED)),
        Collider(PICKUP_SIZE),
    )
}

/// Collect pickups the player overlaps, applying each kind's effect.
#[allow(clippy::too_many_arguments)]
fn collect_pickups(
    mut commands: Commands,
    assets: Res<GameAssets>,
    mut progress: ResMut<GameProgress>,
    mut weapon: ResMut<Weapon>,
    mut explosions: MessageWriter<ExplosionMessage>,
    mut blasts: MessageWriter<BombBlast>,
    player: Query<(Entity, &Transform, &Collider), With<Player>>,
    pickups: Query<(Entity, &Transform, &Collider, &Pickup)>,
    enemies: Query<(Entity, &Transform, Has<crate::ufo::Ufo>), With<Enemy>>,
) {
    if progress.game_over {
        return;
    }
    let Ok((player_entity, pt, pc)) = player.single() else {
        return;
    };
    let ppos = pt.translation.truncate();

    for (entity, tf, col, pick) in &pickups {
        if !overlaps(ppos, pc.0, tf.translation.truncate(), col.0) {
            continue;
        }
        commands.entity(entity).try_despawn();
        commands.spawn(sfx(assets.pickup_sfx.clone()));

        match pick.kind {
            PickupKind::Invincible => {
                commands.entity(player_entity).insert(Invincible::new());
            }
            PickupKind::Repair => {
                progress.health = (progress.health + REPAIR_AMOUNT).min(MAX_HEALTH);
            }
            PickupKind::Weapon => {
                *weapon = match *weapon {
                    Weapon::Single => Weapon::Double,
                    _ => Weapon::Triple,
                };
            }
            PickupKind::Bomb => {
                // The boss reacts separately (one point of damage, see boss.rs).
                blasts.write(BombBlast);
                // Wipe every enemy currently on screen (each explodes and scores).
                for (enemy, et, is_ufo) in &enemies {
                    commands.entity(enemy).try_despawn();
                    progress.score += KILL_REWARD;
                    let position = et.translation.truncate();
                    explosions.write(if is_ufo {
                        ExplosionMessage::ufo(position)
                    } else {
                        ExplosionMessage::at(position)
                    });
                }
            }
        }
    }
}
