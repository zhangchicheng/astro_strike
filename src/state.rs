//! Global game phase.
//!
//! * `Title` — the cover screen ("Start Game"); Space begins the game.
//! * `Intro` — the fighter auto-cruises through space while the captain briefs.
//! * `Combat` — normal shooter gameplay.
//! * `Dialogue` — a conversation (boss taunt, victory debrief) that pauses combat
//!   actions while the world stays in motion.
//!
//! Combat *actions* (spawning, firing, collisions, player control) are gated on
//! `in_state(Phase::Combat)`; the scroll, animations, and passive movement run
//! whenever [`live`] — every phase except the title, and only while the player
//! hasn't failed: on game over the whole screen freezes instantly (only the
//! game-over banner and the return-to-title key stay active).

use bevy::prelude::*;

use crate::combat::GameProgress;

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Phase {
    #[default]
    Title,
    Intro,
    Combat,
    Dialogue,
}

/// True in every phase except the title screen.
pub fn playing(state: Res<State<Phase>>) -> bool {
    !matches!(state.get(), Phase::Title)
}

/// True while the world should be in motion: [`playing`] AND the player hasn't
/// failed. Gates the scroll, sprite animation, and all passive movement (drifting
/// craft, patrols, UFO chains) — so the screen never freezes mid-run (even during
/// dialogue) but freezes instantly on game over.
pub fn live(state: Res<State<Phase>>, progress: Res<GameProgress>) -> bool {
    !matches!(state.get(), Phase::Title) && !progress.game_over
}

pub(super) fn plugin(app: &mut App) {
    app.init_state::<Phase>();
}
