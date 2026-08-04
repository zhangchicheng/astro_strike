//! UFO swarms: alien saucers that dive at the player as a chain.
//!
//! Unlike the straight-diving [`enemy::Enemy`] ships, a UFO swarm moves as a
//! leader-follower chain — the leader homes on the player (with a sine weave for a
//! curved approach) while each follower trails the one ahead along its path, so
//! the whole swarm snakes in. UFOs reuse the `Enemy` marker, so the existing
//! combat systems shoot them down and dock a life on contact; only their movement
//! (and their swarm spawning) is bespoke. They have no [`Velocity`] — `move_ufos`
//! owns their position and cleanup — since they enter from above the view (where
//! the shared `despawn_offscreen` would otherwise cull the trailing chain links).

use std::collections::HashMap;

use bevy::prelude::*;
use rand::Rng;

use crate::animation::SpriteSheetAnimation;
use crate::assets::{GameAssets, UfoArt};
use crate::combat::GameProgress;
use crate::common::{Collider, HITBOX_SCALE, X_BOUND, Y_BOUND};
use crate::enemy::Enemy;
use crate::level::Level;
use crate::player::Player;
use crate::state::{Phase, live};

// Cruise speed, swarm size, and swarm cadence scale with the level's
// `Difficulty` (see `level.rs`); the constants below are level-independent.
/// Desired spacing between chain links (world px).
const UFO_CHAIN_GAP: f32 = 24.0;
/// How fast a UFO's heading eases toward its target direction (fraction/second),
/// so it curves toward its goal rather than snapping — and a darting player can
/// out-turn it.
const UFO_TURN_RATE: f32 = 4.0;
/// The leader weaves toward the player: wiggle amplitude (radians) and frequency.
const UFO_WEAVE_AMP: f32 = 0.5;
const UFO_WEAVE_FREQ: f32 = 4.0;
/// On-screen size of a UFO.
const UFO_DISPLAY: f32 = 20.0;
/// Initial swarm cadence; from then on the timer follows the level `Difficulty`.
const UFO_SWARM_INTERVAL: f32 = 8.0;
/// Backstop so a UFO that keeps missing the player eventually gives up and clears.
const UFO_MAX_LIFETIME: f32 = 9.0;

/// A UFO that snakes toward the player as one link of a chain.
#[derive(Component)]
pub struct Ufo {
    /// The link ahead of this one (closer to the player); `None` for the leader,
    /// which homes on the player directly.
    ahead: Option<Entity>,
    /// Current unit heading, eased over time so turns curve rather than snap.
    heading: Vec2,
    /// Per-UFO phase offset for the weave, and running lifetime.
    weave_phase: f32,
    elapsed: f32,
}

#[derive(Resource)]
struct UfoSwarmTimer(Timer);

pub(super) fn plugin(app: &mut App) {
    app.insert_resource(UfoSwarmTimer(Timer::from_seconds(
        UFO_SWARM_INTERVAL,
        TimerMode::Repeating,
    )))
    // New swarms only appear during combat, but existing ones keep flying (and
    // clean themselves up) through level transitions and dialogue — they just
    // stop hunting the player (see `move_ufos`). On game over they freeze with
    // the rest of the world.
    .add_systems(Update, spawn_ufo_swarms.run_if(in_state(Phase::Combat)))
    .add_systems(Update, move_ufos.run_if(live));
}

/// Rotates `v` by `angle` radians.
fn rotate(v: Vec2, angle: f32) -> Vec2 {
    let (s, c) = angle.sin_cos();
    Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

/// Periodically spawns a swarm entering as a column from just above the view.
fn spawn_ufo_swarms(
    mut commands: Commands,
    time: Res<Time>,
    progress: Res<GameProgress>,
    assets: Res<GameAssets>,
    level: Res<Level>,
    mut timer: ResMut<UfoSwarmTimer>,
    camera: Query<&Transform, With<Camera2d>>,
) {
    if progress.game_over {
        return;
    }
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }
    let Ok(camera) = camera.single() else {
        return;
    };
    if assets.ufos.is_empty() {
        return;
    }
    let cam = camera.translation.truncate();
    let difficulty = level.difficulty();
    // Later levels send swarms more often; effective from this cycle on.
    timer
        .0
        .set_duration(std::time::Duration::from_secs_f32(
            difficulty.ufo_swarm_interval,
        ));

    // Each level has its own character: level 1 fields NO UFOs at all, and every
    // level after it unlocks one more saucer type (level 2 -> ufo_1, level 3 ->
    // ufo_1..2, ... level 5 -> all four). Swarms pick from the unlocked pool.
    let unlocked = (level.0.saturating_sub(1) as usize).min(assets.ufos.len());
    if unlocked == 0 {
        return;
    }

    let mut rng = rand::thread_rng();
    let art = &assets.ufos[rng.gen_range(0..unlocked)];
    // Enter as a vertical column at a random x, from just above the top of the view.
    let x = cam.x + rng.gen_range(-X_BOUND * 0.7..X_BOUND * 0.7);
    let top = cam.y + Y_BOUND + UFO_DISPLAY;

    // Leader lowest (enters first); each follower sits one gap higher and trails
    // the link below it. Later levels field longer chains.
    let mut ahead: Option<Entity> = None;
    for i in 0..difficulty.ufo_swarm_size {
        let pos = Vec2::new(x, top + i as f32 * UFO_CHAIN_GAP);
        ahead = Some(spawn_ufo(&mut commands, art, pos, ahead, i as f32 * 1.3));
    }
}

/// Spawns one UFO link and returns its entity.
fn spawn_ufo(
    commands: &mut Commands,
    art: &UfoArt,
    pos: Vec2,
    ahead: Option<Entity>,
    weave_phase: f32,
) -> Entity {
    let mut sprite = Sprite {
        image: art.image.clone(),
        custom_size: Some(Vec2::splat(UFO_DISPLAY)),
        ..default()
    };
    if let Some(frames) = &art.frames {
        sprite.rect = frames.first().map(|(rect, _)| *rect);
    }

    let mut entity = commands.spawn((
        sprite,
        Transform::from_xyz(pos.x, pos.y, 5.0),
        Enemy,
        Ufo {
            ahead,
            heading: Vec2::NEG_Y,
            weave_phase,
            elapsed: 0.0,
        },
        Collider(Vec2::splat(UFO_DISPLAY) * HITBOX_SCALE),
    ));
    if let Some(frames) = &art.frames {
        entity.insert(SpriteSheetAnimation::new(frames.clone(), true, false));
    }
    entity.id()
}

/// Steers each UFO: the leader homes on the player (weaving); followers trail the
/// link ahead of them, forming a snaking chain.
fn move_ufos(
    mut commands: Commands,
    time: Res<Time>,
    phase: Res<State<Phase>>,
    level: Res<Level>,
    player: Query<&Transform, (With<Player>, Without<Ufo>)>,
    mut ufos: Query<(Entity, &mut Transform, &mut Ufo), Without<Player>>,
) {
    // (On game over this system is frozen by the `live` gate — the swarm holds
    // still in the tableau; after a retry it disengages and expires naturally.)
    let dt = time.delta_secs();
    let speed = level.difficulty().ufo_speed;

    // Snapshot every UFO's (position, heading) up front so followers read a stable
    // value no matter the iteration order.
    let snapshot: HashMap<Entity, (Vec2, Vec2)> = ufos
        .iter()
        .map(|(e, t, u)| (e, (t.translation.truncate(), u.heading)))
        .collect();
    let player_pos = player.single().ok().map(|t| t.translation.truncate());
    // Outside combat (level transition, dialogue) UFOs stop attacking: leaders
    // stop hunting the player and just fly on until they expire or leave.
    let hunting = matches!(phase.get(), Phase::Combat);

    for (entity, mut transform, mut ufo) in &mut ufos {
        ufo.elapsed += dt;
        let pos = transform.translation.truncate();
        // Clean up: a UFO that has run out its life, or overshot well below the
        // player (off the bottom of the view), gives up.
        let below_player = player_pos.is_some_and(|pp| pos.y < pp.y - (Y_BOUND + 100.0));
        if ufo.elapsed > UFO_MAX_LIFETIME || below_player {
            commands.entity(entity).try_despawn();
            continue;
        }

        // Target: a point one gap behind the link ahead (a snaking trail); the
        // leader — or a follower orphaned by a despawn — homes on the player
        // (only while combat is live; otherwise it keeps its current heading).
        let target = match ufo.ahead.and_then(|a| snapshot.get(&a)) {
            Some((ahead_pos, ahead_heading)) => *ahead_pos - *ahead_heading * UFO_CHAIN_GAP,
            None if hunting => player_pos.unwrap_or(pos + ufo.heading),
            None => pos + ufo.heading,
        };

        let mut desired = (target - pos).normalize_or_zero();
        // The leader weaves for a curved dive rather than a beeline.
        if ufo.ahead.is_none() && desired != Vec2::ZERO {
            let angle = UFO_WEAVE_AMP * (ufo.elapsed * UFO_WEAVE_FREQ + ufo.weave_phase).sin();
            desired = rotate(desired, angle);
        }
        if desired != Vec2::ZERO {
            let eased = ufo.heading.lerp(desired, (UFO_TURN_RATE * dt).min(1.0));
            ufo.heading = eased.normalize_or_zero();
            if ufo.heading == Vec2::ZERO {
                ufo.heading = desired;
            }
        }

        // Followers snap onto their slot when close, keeping the chain tight.
        let step = speed * dt;
        let new_pos = if ufo.ahead.is_some() && (target - pos).length() < step {
            target
        } else {
            pos + ufo.heading * step
        };
        transform.translation.x = new_pos.x;
        transform.translation.y = new_pos.y;
    }
}
