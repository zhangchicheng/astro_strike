//! The boss: a three-part sprite (left wing + body + right wing) assembled at the
//! top of the level. When the player approaches it "awakens" (parts swap to their
//! activated art), a health bar appears, and the fight begins — the boss fires
//! laser beams down at the ship, and player fire whittles down its health until it
//! explodes.

use bevy::prelude::*;
use bevy::sprite::SpriteImageMode;
use rand::Rng;

use crate::animation::{ExplosionMessage, SpriteSheetAnimation};
use crate::assets::GameAssets;
use crate::combat::{GameProgress, PlayerBullet};
use crate::common::{Collider, LEVELS, level_chunk_top_y, overlaps};
use crate::level::Level;
use crate::pickup::BombBlast;
use crate::player::{Invincible, Player};
use crate::state::{Phase, live};

/// The boss hovers this far below the top edge of its level's chunk terrain.
const BOSS_TOP_OFFSET: f32 = 70.0;

/// World Y of the boss for level `l` (0-indexed).
fn boss_y(l: u32) -> f32 {
    level_chunk_top_y(l) - BOSS_TOP_OFFSET
}
/// The boss art's native pixels are drawn this many times bigger.
const BOSS_SCALE: f32 = 2.0;
/// The boss wakes once the ship comes within this world distance below it.
const AWAKEN_RANGE: f32 = 250.0;

/// The opaque hull within `boss/boss.png` (native px): the 96x32 canvas keeps
/// transparent side margins, the inked craft spans the central 64x32.
const BOSS_HULL: Vec2 = Vec2::new(64.0, 32.0);
/// x offset (native px from the image center) of each wing gun pod, where the
/// beams emerge.
const BOSS_MUZZLE_X: f32 = 26.0;

/// Player hits the boss can take before it dies.
const BOSS_MAX_HEALTH: u32 = 40;
/// The boss's damage area (a bit tighter than the hull).
const BOSS_HITBOX: Vec2 = Vec2::new(
    BOSS_HULL.x * BOSS_SCALE * 0.75,
    BOSS_HULL.y * BOSS_SCALE * 0.85,
);

/// The boss fires in bursts: each held beam stays on for `BEAM_DURATION`, then
/// rests for `BEAM_COOLDOWN`, then fires again.
const BEAM_DURATION: f32 = 1.0;
const BEAM_COOLDOWN: f32 = 1.5;
/// The beam grows from the emitter to full length over this long (the "emerge").
const BEAM_EXTEND_TIME: f32 = 0.12;
/// Beam width and how far it reaches from the emitter (world px).
/// >>> Tweak `BEAM_LENGTH` to change the beam length. <<<
const BEAM_WIDTH: f32 = 16.0;
const BEAM_LENGTH: f32 = 220.0;
/// The beam's hit box is a little narrower than the sprite.
const BEAM_HIT_WIDTH: f32 = 10.0;
/// While standing in a beam, the player takes 1 damage at most this often.
const BEAM_DAMAGE_INTERVAL: f32 = 0.35;

/// The awakened boss slides horizontally to track the player (eased at this rate),
/// clamped this far from center so it stays roughly on-screen.
const BOSS_MOVE_RATE: f32 = 1.6;
const BOSS_X_LIMIT: f32 = 140.0;

// ----- Health-bar UI (from boss_health_bar.png, 192x16) --------------------
// The frame is one complete bar image (native x 0..~168). Two 3x3 fill swatches
// sit on the far right — white at (180,6), black at (185,6) — used to paint the
// health blocks laid over the frame's inner track.
const HB_SCALE: f32 = 2.0;
const HB_IMG_W: f32 = 170.0; // bar region width, native px (excludes the fill swatches)
const HB_IMG_H: f32 = 16.0;
/// 36 blocks over the frame's inner track: each a 3x3 swatch drawn at native size
/// (never stretched), starting at native x `HB_TRACK_X0` and laid down at a fixed
/// `HB_BLOCK` + `HB_GAP` pitch so there is a clean, uniform 1px gap between them.
/// (36 blocks span X0 + 36*3 + 35*1 = 15..158 native, matching the inner track.)
const HB_SEGMENTS: u32 = 36;
const HB_BLOCK: f32 = 3.0; // native block size (square swatch)
const HB_GAP: f32 = 1.0; // native gap between blocks
const HB_BLOCK_Y: f32 = 6.0; // native top of every block
const HB_TRACK_X0: f32 = 15.0; // native x of the first block's left edge
const HB_FRAME: Rect = Rect {
    min: Vec2::ZERO,
    max: Vec2::new(HB_IMG_W, HB_IMG_H),
};
const HB_FILL_FULL: Rect = swatch(180.0); // white
const HB_FILL_EMPTY: Rect = swatch(185.0); // black

/// Native left edge of block `i` (fixed `HB_BLOCK` + `HB_GAP` pitch → uniform 1px gaps).
fn block_x(i: u32) -> f32 {
    HB_TRACK_X0 + i as f32 * (HB_BLOCK + HB_GAP)
}

/// A 3x3 fill swatch at `x`, row 6.
const fn swatch(x: f32) -> Rect {
    Rect {
        min: Vec2::new(x, 6.0),
        max: Vec2::new(x + 3.0, 9.0),
    }
}

#[derive(Component)]
struct Boss;

/// A held laser beam, spawned as a child of the boss at a wing muzzle. It grows
/// from the emitter to full length, then holds until firing stops (then despawns).
#[derive(Component)]
struct BossBeam {
    /// Local y of the muzzle (the beam's fixed top) within the boss.
    muzzle_y: f32,
    /// Seconds since it began firing (drives the extend/grow).
    age: f32,
}

/// Root of the on-screen boss health bar.
#[derive(Component)]
struct BossHealthUi;

/// One block of the segmented health bar (index 0 = leftmost). Shows the filled
/// (white) swatch while below current health, else the empty (black) swatch.
#[derive(Component)]
struct BossHealthCell {
    index: u32,
}

/// Boss fight state. `max_health` scales with the level (see `Level::difficulty`).
#[derive(Resource)]
struct BossState {
    awakened: bool,
    health: u32,
    max_health: u32,
}

impl Default for BossState {
    fn default() -> Self {
        Self {
            awakened: false,
            health: BOSS_MAX_HEALTH,
            max_health: BOSS_MAX_HEALTH,
        }
    }
}

/// Drives the fire/rest cycle; `firing` is whether the beam is currently on.
#[derive(Resource)]
struct BossFire {
    timer: Timer,
    firing: bool,
}

/// Breather after a boss dies, so its explosions play out before the game moves on
/// (straight into the next briefing felt abrupt). Present only during the pause.
#[derive(Resource)]
struct BossDefeated(Timer);

const BOSS_DEFEAT_PAUSE: f32 = 2.5;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<BossState>()
        .insert_resource(BossFire {
            timer: Timer::from_seconds(BEAM_COOLDOWN, TimerMode::Once),
            firing: false,
        })
        // (Re)built at the start of every level, fresh state each time.
        // Reset on Intro (level start / retry) AND on Title: dying to the boss
        // now exits to the menu without passing through Intro, and the health
        // bar is UI — left alone it draws on top of the title screen.
        .add_systems(OnEnter(Phase::Intro), reset_boss)
        .add_systems(OnEnter(Phase::Title), reset_boss)
        .add_systems(
            Update,
            (
                awaken_boss,
                boss_move,
                boss_beams,
                grow_beams,
                boss_beam_hits_player,
                bullet_hits_boss,
                bomb_chips_boss,
                update_health_bar,
                boss_death,
            )
                .run_if(in_state(Phase::Combat))
                // Freeze the boss (movement, beams) instantly on game over too.
                .run_if(live),
        )
        .add_systems(
            Update,
            after_boss_defeated
                .run_if(in_state(Phase::Combat))
                .run_if(live)
                .run_if(resource_exists::<BossDefeated>),
        );
}

/// Despawns any prior boss, resets its fight state, and spawns a fresh boss for
/// the level about to begin.
fn reset_boss(
    mut commands: Commands,
    assets: Res<GameAssets>,
    level: Res<Level>,
    mut state: ResMut<BossState>,
    existing: Query<Entity, With<Boss>>,
    ui: Query<Entity, With<BossHealthUi>>,
) {
    for entity in &existing {
        commands.entity(entity).try_despawn();
    }
    // Also clear a leftover health bar: on a death-retry (R) the boss may have
    // been awakened without dying, and only `boss_death` removes the bar.
    for entity in &ui {
        commands.entity(entity).try_despawn();
    }
    // And a leftover defeat pause (dying to a stray shot during it, then retrying,
    // must not resume the old timer and skip ahead).
    commands.remove_resource::<BossDefeated>();
    // Later warlords are tougher (see `Level::difficulty`).
    let health = level.difficulty().boss_health;
    *state = BossState {
        awakened: false,
        health,
        max_health: health,
    };
    // Spawn the boss at the top of THIS level's chunks (higher up for later levels).
    spawn_boss(&mut commands, &assets, boss_y(level.0.saturating_sub(1)));
}

fn spawn_boss(commands: &mut Commands, assets: &GameAssets, y: f32) {
    commands.spawn((
        Sprite {
            image: assets.boss.clone(),
            custom_size: Some(assets.boss_size * BOSS_SCALE),
            ..default()
        },
        Transform::from_xyz(0.0, y, 6.0),
        Boss,
        Collider(BOSS_HITBOX),
        Name::new("Boss"),
    ));
}

/// Wake the boss (swap parts to activated art + show the health bar) once the
/// ship is close.
#[allow(clippy::type_complexity)]
fn awaken_boss(
    mut commands: Commands,
    mut state: ResMut<BossState>,
    assets: Res<GameAssets>,
    player: Query<&Transform, With<Player>>,
    mut boss: Query<(&Transform, &mut Sprite), (With<Boss>, Without<Player>)>,
) {
    if state.awakened {
        return;
    }
    let (Ok(player), Ok((boss_tf, mut boss_sprite))) = (player.single(), boss.single_mut()) else {
        return;
    };
    if player.translation.y >= boss_tf.translation.y - AWAKEN_RANGE {
        state.awakened = true;
        boss_sprite.image = assets.boss_activated.clone();
        spawn_health_bar(&mut commands, &assets);
    }
}

/// Frames `(rect, secs)` for the 4-frame beam-segment animation.
fn beam_frames() -> Vec<(Rect, f32)> {
    (0..4)
        .map(|i| {
            let min = Vec2::new(i as f32 * 16.0, 0.0);
            (
                Rect {
                    min,
                    max: min + Vec2::splat(16.0),
                },
                0.08,
            )
        })
        .collect()
}

/// Slide the awakened boss horizontally to hover over the player.
fn boss_move(
    time: Res<Time>,
    state: Res<BossState>,
    mut boss: Query<&mut Transform, (With<Boss>, Without<Player>)>,
    player: Query<&Transform, With<Player>>,
) {
    if !state.awakened {
        return;
    }
    let (Ok(mut boss), Ok(player)) = (boss.single_mut(), player.single()) else {
        return;
    };
    let target = player.translation.x.clamp(-BOSS_X_LIMIT, BOSS_X_LIMIT);
    let t = (BOSS_MOVE_RATE * time.delta_secs()).min(1.0);
    boss.translation.x += (target - boss.translation.x) * t;
}

/// Runs the fire/rest cycle: when idle and the timer elapses, emit a held beam from
/// each wing (children of the boss); when firing and the timer elapses, despawn the
/// beams (they vanish instantly) and start the cooldown.
#[allow(clippy::too_many_arguments)]
fn boss_beams(
    mut commands: Commands,
    time: Res<Time>,
    state: Res<BossState>,
    assets: Res<GameAssets>,
    level: Res<Level>,
    mut fire: ResMut<BossFire>,
    boss: Query<Entity, With<Boss>>,
    beams: Query<Entity, With<BossBeam>>,
) {
    if !state.awakened || state.health == 0 {
        return;
    }
    let Ok(boss_entity) = boss.single() else {
        return;
    };
    if !fire.timer.tick(time.delta()).just_finished() {
        return;
    }

    if fire.firing {
        // Stop firing: the beams disappear at once.
        for beam in &beams {
            commands.entity(beam).try_despawn();
        }
        fire.firing = false;
        // Later warlords rest less between bursts (see `Level::difficulty`).
        fire.timer = Timer::from_seconds(level.difficulty().boss_beam_cooldown, TimerMode::Once);
    } else {
        // Start firing: one held beam from each wing muzzle.
        fire.firing = true;
        fire.timer = Timer::from_seconds(BEAM_DURATION, TimerMode::Once);
        let frames = beam_frames();
        spawn_beam(
            &mut commands,
            boss_entity,
            &assets.boss_projectile,
            &frames,
            Vec2::new(-BOSS_MUZZLE_X * BOSS_SCALE, 0.0),
        );
        spawn_beam(
            &mut commands,
            boss_entity,
            &assets.boss_projectile,
            &frames,
            Vec2::new(BOSS_MUZZLE_X * BOSS_SCALE, 0.0),
        );
    }
}

/// Spawns one held beam as a child of the boss, its top pinned at `muzzle_local`.
/// The 16px art is tiled down the beam and cycled through 4 frames (the laser
/// shimmer); `grow_beams` stretches it to length.
fn spawn_beam(
    commands: &mut Commands,
    boss: Entity,
    image: &Handle<Image>,
    frames: &[(Rect, f32)],
    muzzle_local: Vec2,
) {
    let animation = SpriteSheetAnimation::new(frames.to_vec(), true, false);
    let rect = animation.first_rect();
    commands.spawn((
        Sprite {
            image: image.clone(),
            custom_size: Some(Vec2::new(BEAM_WIDTH, 0.0)),
            rect,
            image_mode: SpriteImageMode::Tiled {
                tile_x: false,
                tile_y: true,
                stretch_value: 1.0,
            },
            ..default()
        },
        // Local to the boss, in front of its body so the beam renders on top.
        Transform::from_xyz(muzzle_local.x, muzzle_local.y, 1.0),
        BossBeam {
            muzzle_y: muzzle_local.y,
            age: 0.0,
        },
        animation,
        ChildOf(boss),
    ));
}

/// Grow each beam from its emitter to full length; the top stays at the muzzle.
fn grow_beams(time: Res<Time>, mut beams: Query<(&mut Transform, &mut Sprite, &mut BossBeam)>) {
    let dt = time.delta_secs();
    for (mut transform, mut sprite, mut beam) in &mut beams {
        beam.age += dt;
        let t = (beam.age / BEAM_EXTEND_TIME).min(1.0);
        let length = t * BEAM_LENGTH;
        sprite.custom_size = Some(Vec2::new(BEAM_WIDTH, length));
        // Center-anchored sprite: put the center half a length below the muzzle so
        // the top stays pinned and it extends downward.
        transform.translation.y = beam.muzzle_y - length / 2.0;
    }
}

/// A held beam damages the player while they stand in it (throttled).
fn boss_beam_hits_player(
    time: Res<Time>,
    mut progress: ResMut<GameProgress>,
    mut cooldown: Local<f32>,
    beams: Query<(&GlobalTransform, &Sprite), With<BossBeam>>,
    player: Query<(&Transform, &Collider, Has<Invincible>), With<Player>>,
) {
    if *cooldown > 0.0 {
        *cooldown -= time.delta_secs();
    }
    let Ok((pt, pc, invincible)) = player.single() else {
        return;
    };
    // An invincibility pickup shrugs the beam off too.
    if invincible {
        return;
    }
    let ppos = pt.translation.truncate();
    for (beam_tf, sprite) in &beams {
        let size = sprite.custom_size.unwrap_or(Vec2::ZERO);
        let hit = Vec2::new(BEAM_HIT_WIDTH, size.y);
        if overlaps(ppos, pc.0, beam_tf.translation().truncate(), hit) {
            if *cooldown <= 0.0 {
                progress.damage();
                *cooldown = BEAM_DAMAGE_INTERVAL;
            }
            break;
        }
    }
}

/// Player fire chips away at the boss (once it has awakened).
fn bullet_hits_boss(
    mut commands: Commands,
    mut state: ResMut<BossState>,
    mut explosions: MessageWriter<ExplosionMessage>,
    bullets: Query<(Entity, &Transform, &Collider), With<PlayerBullet>>,
    boss: Query<(&GlobalTransform, &Collider), With<Boss>>,
) {
    if !state.awakened || state.health == 0 {
        return;
    }
    let Ok((boss_tf, boss_col)) = boss.single() else {
        return;
    };
    let boss_pos = boss_tf.translation().truncate();
    for (bullet, bt, bc) in &bullets {
        if overlaps(boss_pos, boss_col.0, bt.translation.truncate(), bc.0) {
            commands.entity(bullet).try_despawn();
            state.health = state.health.saturating_sub(1);
            explosions.write(ExplosionMessage::at(bt.translation.truncate()));
        }
    }
}

/// A bomb pickup only CHIPS the boss: one point of damage per blast, with a
/// small explosion as feedback — never the instant wipe the planes get, which
/// would trivialize the fight. Only an awakened boss (fight in progress) is hit.
fn bomb_chips_boss(
    mut blasts: MessageReader<BombBlast>,
    mut state: ResMut<BossState>,
    mut explosions: MessageWriter<ExplosionMessage>,
    boss: Query<&GlobalTransform, With<Boss>>,
) {
    for _ in blasts.read() {
        if !state.awakened || state.health == 0 {
            continue;
        }
        state.health = state.health.saturating_sub(1);
        if let Ok(boss_tf) = boss.single() {
            explosions.write(ExplosionMessage::at(boss_tf.translation().truncate()));
        }
    }
}

/// When the boss's health hits zero, blow it up and clear its UI. A mid-run boss
/// hands straight over to the next level's intro — the scroll carries on
/// seamlessly (the intro cap sits above the boss) and leftover craft fly off on
/// their own; the pre-briefing quiet stretch supplies the breather. Only the
/// final boss gets the [`BossDefeated`] pause, before the victory debrief.
fn boss_death(
    mut commands: Commands,
    state: Res<BossState>,
    mut level: ResMut<Level>,
    mut next: ResMut<NextState<Phase>>,
    mut explosions: MessageWriter<ExplosionMessage>,
    boss: Query<(Entity, &Transform), With<Boss>>,
    ui: Query<Entity, With<BossHealthUi>>,
) {
    if state.health > 0 {
        return;
    }
    // `boss.single()` is empty once we've already despawned it, so this only fires
    // for the one frame the boss dies.
    let Ok((entity, transform)) = boss.single() else {
        return;
    };
    let center = transform.translation.truncate();
    // A cluster of blasts across the whole boss.
    let mut rng = rand::thread_rng();
    for _ in 0..10 {
        let offset = Vec2::new(
            rng.gen_range(-BOSS_HULL.x..BOSS_HULL.x),
            rng.gen_range(-BOSS_HULL.y..BOSS_HULL.y),
        );
        explosions.write(ExplosionMessage::at(center + offset));
    }
    commands.entity(entity).try_despawn();
    for e in &ui {
        commands.entity(e).try_despawn();
    }

    if level.0 >= LEVELS {
        // Final boss: let the explosions play and the moment breathe, then the
        // victory debrief (see `after_boss_defeated`).
        commands.insert_resource(BossDefeated(Timer::from_seconds(
            BOSS_DEFEAT_PAUSE,
            TimerMode::Once,
        )));
    } else {
        // Mid-run boss: straight into the next level's intro; the camera scrolls
        // on into its space corridor without stalling. Ask the briefing for the
        // longer pre-dialogue breather (a fresh entry uses a short one).
        commands.insert_resource(crate::dialogue::PostBossIntro);
        level.0 += 1;
        next.set(Phase::Intro);
    }
}

/// After the final boss's pause, play the victory debrief (ends at the title).
/// `Level` is left as-is on the way out (it resets in `level::start_new_game`)
/// so the still-Combat `auto_scroll` doesn't snap the camera to level 1's cap.
fn after_boss_defeated(
    mut commands: Commands,
    time: Res<Time>,
    mut pause: ResMut<BossDefeated>,
    assets: Res<GameAssets>,
    mut next: ResMut<NextState<Phase>>,
) {
    if !pause.0.tick(time.delta()).is_finished() {
        return;
    }
    commands.remove_resource::<BossDefeated>();
    crate::dialogue::start_victory(&mut commands, &assets);
    next.set(Phase::Dialogue);
}

/// Light each block filled (white) or empty (black) for the boss's current health.
fn update_health_bar(state: Res<BossState>, mut cells: Query<(&BossHealthCell, &mut ImageNode)>) {
    let fraction = state.health as f32 / state.max_health.max(1) as f32;
    let filled = (fraction * HB_SEGMENTS as f32).round() as u32;
    for (cell, mut image) in &mut cells {
        image.rect = Some(if cell.index < filled {
            HB_FILL_FULL
        } else {
            HB_FILL_EMPTY
        });
    }
}

/// One vertical slice of the health-bar frame, stretched to fill its cell.
fn frame_slice(image: Handle<Image>, rect: Rect) -> ImageNode {
    ImageNode {
        image,
        rect: Some(rect),
        image_mode: NodeImageMode::Stretch,
        ..default()
    }
}

/// Builds the boss health-bar UI: the complete bar image as the frame, with
/// [`HB_SEGMENTS`] fixed 3x3 blocks laid over its inner track at native pixel
/// positions (never stretched — each block is drawn at its own size).
fn spawn_health_bar(commands: &mut Commands, assets: &GameAssets) {
    let img = assets.boss_health_bar.clone();
    let s = HB_SCALE;
    let bar_w = HB_IMG_W * s;
    let bar_h = HB_IMG_H * s;

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(18.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            BossHealthUi,
            Name::new("BossHealthBar"),
        ))
        .with_children(|root| {
            // The bar; blocks are positioned absolutely within it.
            root.spawn(Node {
                width: Val::Px(bar_w),
                height: Val::Px(bar_h),
                position_type: PositionType::Relative,
                ..default()
            })
            .with_children(|bar| {
                // Frame: the whole bar image (minus the fill swatches), behind the blocks.
                bar.spawn((
                    frame_slice(img.clone(), HB_FRAME),
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(0.0),
                        left: Val::Px(0.0),
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                ));

                // Blocks: 36 fixed 3x3 swatches over the inner track, unstretched.
                for index in 0..HB_SEGMENTS {
                    bar.spawn((
                        frame_slice(img.clone(), HB_FILL_FULL),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(block_x(index) * s),
                            top: Val::Px(HB_BLOCK_Y * s),
                            width: Val::Px(HB_BLOCK * s),
                            height: Val::Px(HB_BLOCK * s),
                            ..default()
                        },
                        BossHealthCell { index },
                    ));
                }
            });
        });
}
