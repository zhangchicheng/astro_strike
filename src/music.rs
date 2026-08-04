//! Looping background music.
//!
//! Starts `ChillMenu.wav` once at startup and lets it loop for the whole session,
//! across every screen and phase.

use bevy::audio::Volume;
use bevy::prelude::*;

use crate::assets::GameAssets;

/// Background-music level (0.0 = silent, 1.0 = full). Kept low so it sits under
/// the gameplay sound effects.
const MUSIC_VOLUME: f32 = 0.35;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Startup, start_music);
}

fn start_music(mut commands: Commands, assets: Res<GameAssets>) {
    commands.spawn((
        AudioPlayer::new(assets.music.clone()),
        PlaybackSettings::LOOP.with_volume(Volume::Linear(MUSIC_VOLUME)),
        Name::new("BackgroundMusic"),
    ));
}
