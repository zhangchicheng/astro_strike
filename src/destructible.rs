//! Destructible buildings and units.
//!
//! Objects flagged `Destructible=true` in the TMX get a [`Destructible`] component
//! (and a `Collider`) when the map loads. Player fire damages them: each hit swaps
//! the sprite to the next entry in [`Destructible::stages`], and the final entry is
//! the destroyed look, at which point the object becomes an inert wreck.
//!
//! Most props stay intact until the last hit (their early stages repeat the intact
//! tile); `unit_2` instead has genuine intermediate frames, so it visibly crumbles
//! step by step. The per-object stage list is built in `tilemap.rs`.

use bevy::prelude::*;

use crate::animation::ExplosionMessage;
use crate::combat::PlayerBullet;
use crate::common::{Collider, overlaps};
use crate::state::Phase;

/// Number of player hits a destructible object takes before it's destroyed.
pub const DESTRUCTIBLE_HP: u32 = 3;

#[derive(Component)]
pub struct Destructible {
    /// Sprite rect to show after each successive hit; the last is the destroyed
    /// state. Its length is the object's hit points.
    pub stages: Vec<Rect>,
    /// How many hits have landed so far (indexes into `stages`).
    pub taken: usize,
    /// The undamaged sprite rect and hit-box size, so the object can be restored
    /// when the level (re)starts — the map itself is built once and never reloaded.
    pub intact: Rect,
    pub collider: Vec2,
}

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, damage_buildings.run_if(in_state(Phase::Combat)))
        .add_systems(OnEnter(Phase::Intro), reset_destructibles);
}

/// Repairs every destructible at level start: undo damage taken on a previous run
/// (the map is never rebuilt, so without this a retry/new game keeps the wrecks).
fn reset_destructibles(
    mut commands: Commands,
    mut buildings: Query<(Entity, &mut Sprite, &mut Destructible)>,
) {
    for (entity, mut sprite, mut destructible) in &mut buildings {
        if destructible.taken == 0 {
            continue;
        }
        destructible.taken = 0;
        sprite.rect = Some(destructible.intact);
        // Wrecks had their hit box removed; give it back.
        commands
            .entity(entity)
            .insert(Collider(destructible.collider));
    }
}

fn damage_buildings(
    mut commands: Commands,
    mut explosions: MessageWriter<ExplosionMessage>,
    bullets: Query<(Entity, &Transform, &Collider), With<PlayerBullet>>,
    // Buildings are children of a parallax layer, so use their GlobalTransform.
    mut buildings: Query<(
        Entity,
        &GlobalTransform,
        &Collider,
        &mut Sprite,
        &mut Destructible,
    )>,
) {
    for (bullet, bullet_tf, bullet_col) in &bullets {
        for (building, building_tf, building_col, mut sprite, mut destructible) in &mut buildings {
            let pos = building_tf.translation().truncate();
            if overlaps(
                bullet_tf.translation.truncate(),
                bullet_col.0,
                pos,
                building_col.0,
            ) {
                commands.entity(bullet).try_despawn();
                destructible.taken += 1;
                // Advance to this hit's damage frame (progressive for unit_2).
                if let Some(rect) = destructible.stages.get(destructible.taken - 1) {
                    sprite.rect = Some(*rect);
                }
                // The last stage is the destroyed look: become an inert wreck.
                // Only the hit box is removed — the `Destructible` stays so the
                // level-start reset can find and repair the wreck (it drops out of
                // this query anyway once the `Collider` is gone).
                if destructible.taken >= destructible.stages.len() {
                    commands.entity(building).remove::<Collider>();
                    explosions.write(ExplosionMessage::at(pos));
                }
                break; // this bullet is spent
            }
        }
    }
}
