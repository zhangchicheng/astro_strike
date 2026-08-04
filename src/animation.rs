//! Sprite-sheet animation used by two features:
//!   1. TMX tile animations (e.g. the `unit_4` object) — looping, per-frame
//!      durations read from the tileset.
//!   2. Explosions played on impact — a one-shot strip that despawns when done.
//!
//! Both share a single [`SpriteSheetAnimation`] component that cycles the
//! `Sprite.rect` sub-rectangle over time.

use bevy::prelude::*;

use crate::assets::GameAssets;
use crate::common::sfx;
use crate::state::{live, playing};

/// Which explosion strip a blast plays.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ExplosionKind {
    /// `explosion_1.png` — the standard 7-frame blast.
    #[default]
    Standard,
    /// `explosion_2.png` — the 5-frame blast UFOs go up in.
    Ufo,
}

/// Fired when something should blow up at a world position.
#[derive(Message)]
pub struct ExplosionMessage {
    pub position: Vec2,
    pub kind: ExplosionKind,
}

impl ExplosionMessage {
    /// A standard blast at `position`.
    pub fn at(position: Vec2) -> Self {
        Self {
            position,
            kind: ExplosionKind::Standard,
        }
    }

    /// A UFO blast at `position`.
    pub fn ufo(position: Vec2) -> Self {
        Self {
            position,
            kind: ExplosionKind::Ufo,
        }
    }
}

// Both strips are rows of 16x16 frames: `explosion_1.png` 7 of them (112x16),
// `explosion_2.png` 5 (80x16).
const EXPLOSION_FRAME: Vec2 = Vec2::new(16.0, 16.0);
const EXPLOSION_FRAMES: usize = 7;
const UFO_EXPLOSION_FRAMES: usize = 5;
const EXPLOSION_FRAME_TIME: f32 = 0.05;
const EXPLOSION_DISPLAY_SIZE: Vec2 = Vec2::new(48.0, 48.0);

/// Cycles a sprite through a list of source rectangles.
#[derive(Component)]
pub struct SpriteSheetAnimation {
    /// `(source rect, seconds to show it)` for each frame.
    frames: Vec<(Rect, f32)>,
    current: usize,
    elapsed: f32,
    looping: bool,
    despawn_on_finish: bool,
}

impl SpriteSheetAnimation {
    pub fn new(frames: Vec<(Rect, f32)>, looping: bool, despawn_on_finish: bool) -> Self {
        Self {
            frames,
            current: 0,
            elapsed: 0.0,
            looping,
            despawn_on_finish,
        }
    }

    /// The rect for the first frame, so the sprite starts on the right cell.
    pub fn first_rect(&self) -> Option<Rect> {
        self.frames.first().map(|(rect, _)| *rect)
    }
}

/// Parses an Aseprite JSON export (hash format, as produced by Aseprite's
/// "Sprite Sheet" export) into ordered `(source rect, seconds)` frames. Frames
/// are ordered by their position in the sheet (top-to-bottom, then left-to-right)
/// so the result is independent of JSON key ordering.
/// `path` is relative to `assets/` (read through `vfs`, so it works on the web).
pub fn frames_from_aseprite_json(path: &str) -> Vec<(Rect, f32)> {
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct AseRect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    }
    #[derive(Deserialize)]
    struct AseFrame {
        frame: AseRect,
        duration: f32,
    }
    #[derive(Deserialize)]
    struct AseSheet {
        frames: std::collections::HashMap<String, AseFrame>,
    }

    let Some(text) = crate::vfs::read_string(path) else {
        error!("failed to read aseprite json `{path}`");
        return Vec::new();
    };
    let sheet: AseSheet = match serde_json::from_str(&text) {
        Ok(sheet) => sheet,
        Err(err) => {
            error!("failed to parse aseprite json `{path}`: {err}");
            return Vec::new();
        }
    };

    let mut frames: Vec<AseFrame> = sheet.frames.into_values().collect();
    frames.sort_by(|a, b| {
        (a.frame.y, a.frame.x)
            .partial_cmp(&(b.frame.y, b.frame.x))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    frames
        .into_iter()
        .map(|f| {
            let min = Vec2::new(f.frame.x, f.frame.y);
            (
                Rect {
                    min,
                    max: min + Vec2::new(f.frame.w, f.frame.h),
                },
                f.duration / 1000.0,
            )
        })
        .collect()
}

/// Builds source rects for a horizontal strip of `count` frames of `frame` size.
fn strip_frames(frame: Vec2, count: usize, duration: f32) -> Vec<(Rect, f32)> {
    (0..count)
        .map(|i| {
            let min = Vec2::new(i as f32 * frame.x, 0.0);
            (
                Rect {
                    min,
                    max: min + frame,
                },
                duration,
            )
        })
        .collect()
}

pub(super) fn plugin(app: &mut App) {
    // Animation runs whenever the world is live, not just combat: the boss's
    // death explosions spawn on the last combat frame and must keep playing (and
    // despawn on finish) across level transitions and dialogue — gated on combat
    // only, they froze on their last frame. On game over the world freezes
    // instantly, so `animate_sprites` stops too; `spawn_explosions` stays
    // phase-gated only, so the explosion of whatever killed the player still
    // appears (held on its first frame) as part of the frozen tableau.
    app.add_message::<ExplosionMessage>()
        .add_systems(Update, spawn_explosions.run_if(playing))
        .add_systems(Update, animate_sprites.run_if(live));
}

fn spawn_explosions(
    mut commands: Commands,
    mut messages: MessageReader<ExplosionMessage>,
    assets: Res<GameAssets>,
) {
    for message in messages.read() {
        let (image, frames) = match message.kind {
            ExplosionKind::Standard => (assets.explosion.clone(), EXPLOSION_FRAMES),
            ExplosionKind::Ufo => (assets.explosion_2.clone(), UFO_EXPLOSION_FRAMES),
        };
        commands.spawn(explosion(image, frames, message.position));
        // Explosion sound (one shot; despawns when finished).
        commands.spawn(sfx(assets.explosion_sound.clone()));
    }
}

/// Bundle for a one-shot impact explosion that despawns when its animation ends.
fn explosion(image: Handle<Image>, frames: usize, pos: Vec2) -> impl Bundle {
    let animation = SpriteSheetAnimation::new(
        strip_frames(EXPLOSION_FRAME, frames, EXPLOSION_FRAME_TIME),
        false,
        true,
    );
    let rect = animation.first_rect();
    (
        Sprite {
            image,
            custom_size: Some(EXPLOSION_DISPLAY_SIZE),
            rect,
            ..default()
        },
        Transform::from_xyz(pos.x, pos.y, 10.0),
        animation,
    )
}

fn animate_sprites(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut SpriteSheetAnimation, &mut Sprite)>,
) {
    let dt = time.delta_secs();
    for (entity, mut anim, mut sprite) in &mut query {
        if anim.frames.is_empty() {
            continue;
        }
        anim.elapsed += dt;
        let current_duration = anim.frames[anim.current].1;
        if anim.elapsed < current_duration {
            continue;
        }
        anim.elapsed -= current_duration;

        let next = anim.current + 1;
        if next < anim.frames.len() {
            anim.current = next;
        } else if anim.looping {
            anim.current = 0;
        } else if anim.despawn_on_finish {
            commands.entity(entity).despawn();
            continue;
        } else {
            continue; // hold on the last frame
        }
        sprite.rect = Some(anim.frames[anim.current].0);
    }
}
