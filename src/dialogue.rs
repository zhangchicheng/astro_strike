//! Conversations: the intro briefing and the mid-game boss taunt.
//!
//! * **Intro** (`Phase::Intro`, the start): the fighter auto-cruises through the
//!   random space segments while Captain Reyes briefs the mission. The player has
//!   no control; advancing past the last line hands over control (`Phase::Combat`).
//! * **Boss** (`Phase::Dialogue`): pressing `T` during combat freezes the action
//!   for a short exchange, then returns to combat.
//!
//! Both share one speech-bubble UI (`dialogue.png` + a portrait per line).

use bevy::prelude::*;

use crate::assets::GameAssets;
use crate::common::{VIEW_SCALE, level_bottom_y};
use crate::level::Level;
use crate::state::Phase;

// ----- Dialogue box (from dialogue.png) -----
// dialogue.png's opaque content is 42x37 at offset (6,2); we split the 42px width
// into three 14px vertical segments (left cap / stretchable middle / right cap).
const DLG_SCALE: f32 = VIEW_SCALE;
const DLG_CONTENT_TOP: f32 = 2.0;
const DLG_CONTENT_BOTTOM: f32 = 39.0;
const DLG_CONTENT_LEFT: f32 = 6.0;
const DLG_SEG_W: f32 = 14.0;
const DLG_HEIGHT: f32 = (DLG_CONTENT_BOTTOM - DLG_CONTENT_TOP) * DLG_SCALE;
const DLG_CAP_W: f32 = DLG_SEG_W * DLG_SCALE;

// ----- Portrait (a 32x32 face inside portrait_box.png, pinned at 3,4) -----
const DLG_FRAME: f32 = 40.0 * DLG_SCALE; // portrait_box is 40x40
const DLG_INNER: f32 = 32.0 * DLG_SCALE; // portrait face is 32x32
const DLG_INNER_X: f32 = 3.0 * DLG_SCALE;
const DLG_INNER_Y: f32 = 4.0 * DLG_SCALE;

/// A single line of dialogue: who's talking, what they say, and their portrait.
struct DialogueLine {
    speaker: &'static str,
    text: &'static str,
    portrait: Handle<Image>,
}

/// The conversation currently being shown.
#[derive(Resource)]
struct ActiveDialogue {
    lines: Vec<DialogueLine>,
    index: usize,
    /// Phase to enter after the last line (Combat for briefings/taunts; Title for
    /// the victory debrief).
    on_finish: Phase,
}

/// Marker inserted by `boss::boss_death` before advancing to the next level's
/// intro, requesting the scroll-in breather. Consumed by `queue_intro`.
#[derive(Resource)]
pub struct PostBossIntro;

/// A briefing waiting to open. On a fresh entry (game start, death retry) it
/// waits for the player to press Space/Enter. After a boss kill it first waits
/// for the next level's map to scroll in and its palette dissolve to finish
/// (ignoring input meanwhile, so the player can't pop the briefing mid-cruise),
/// and only then arms the same Space/Enter wait.
#[derive(Resource)]
enum PendingBriefing {
    AwaitKey,
    AwaitScroll,
}

/// How far past the band boundary the camera must be before the post-boss
/// briefing arms its key wait. The palette dissolve starts when the camera
/// center crosses the boundary and runs 0.8s; at the 40 px/s intro scroll
/// that is 32px — this margin keeps the dialog strictly after the color change.
const DISSOLVE_MARGIN: f32 = 40.0;

#[derive(Component)]
pub struct SpeakerText;

#[derive(Component)]
struct BodyText;

#[derive(Component)]
struct PortraitImage;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(OnEnter(Phase::Intro), queue_intro)
        .add_systems(
            Update,
            open_pending_briefing
                .run_if(in_state(Phase::Intro))
                .run_if(resource_exists::<PendingBriefing>),
        )
        .add_systems(Update, start_boss_dialogue.run_if(in_state(Phase::Combat)))
        .add_systems(OnEnter(Phase::Dialogue), open_boss_dialogue)
        .add_systems(
            Update,
            advance_dialogue
                .run_if(not(in_state(Phase::Combat)))
                .run_if(resource_exists::<ActiveDialogue>),
        );
}

/// The intro briefing for `level` (1-based). Each level gets its own conversation.
fn level_intro(assets: &GameAssets, level: u32) -> Vec<DialogueLine> {
    match level {
        1 => captain_conversation(assets),
        2 => second_wave_conversation(assets),
        3 => third_sector_conversation(assets),
        4 => fourth_sector_conversation(assets),
        _ => final_sector_conversation(assets),
    }
}

/// Level 1: Captain Reyes' mission briefing (captain speaks, pilot answers).
fn captain_conversation(assets: &GameAssets) -> Vec<DialogueLine> {
    let cap = |text| DialogueLine {
        speaker: "CAPTAIN",
        text,
        portrait: assets.captain.clone(),
    };
    let pilot = |text| DialogueLine {
        speaker: "PILOT",
        text,
        portrait: assets.hero.clone(),
    };
    vec![
        cap("Pilot, this is Captain Reyes. We've got a delicate little problem."),
        pilot("Delicate's my middle name, Captain. Right after 'reckless'."),
        cap("An enemy colony's dug into the belt ahead, bristling with guns."),
        pilot("Sounds cozy. I'll bring a housewarming gift or twelve."),
        cap("It's one fighter against a whole colony. You sure about this?"),
        pilot("I've flown into worse before breakfast. Call this dessert."),
        cap("Their warlord bites back. Try to come home in one piece."),
        pilot("One piece, one legend. Punching it. See you at debrief!"),
    ]
}

/// Level 2: pressing on into the second colony after the first warlord falls.
fn second_wave_conversation(assets: &GameAssets) -> Vec<DialogueLine> {
    let cap = |text| DialogueLine {
        speaker: "CAPTAIN",
        text,
        portrait: assets.captain.clone(),
    };
    let pilot = |text| DialogueLine {
        speaker: "PILOT",
        text,
        portrait: assets.hero.clone(),
    };
    vec![
        cap("One warlord down and you're still in one piece. Colour me impressed."),
        pilot("Told you. Barely scratched the paint. Where to next?"),
        cap("Deeper in. The defenses run twice as thick from here on."),
        pilot("Then I'll fly twice as sharp. Copy that."),
        cap("This warlord watched the first one fall - expect a real fight."),
        pilot("Angry I can work with. Punching it. Let's finish the job."),
    ]
}

/// Level 3: pushing into the middle of the enemy's territory.
fn third_sector_conversation(assets: &GameAssets) -> Vec<DialogueLine> {
    let cap = |text| DialogueLine {
        speaker: "CAPTAIN",
        text,
        portrait: assets.captain.clone(),
    };
    let pilot = |text| DialogueLine {
        speaker: "PILOT",
        text,
        portrait: assets.hero.clone(),
    };
    vec![
        cap("Two down. Past this point is where their real strength begins."),
        pilot("So the warm-up is over. Good - I was getting bored."),
        cap("Their patrols run heavier here. Watch every corner of the sky."),
        pilot("More of them just means more targets. I'll manage."),
        cap("Steady, pilot. Overconfidence has buried better flyers than you."),
        pilot("Noted. Punching it - carefully reckless, as always."),
    ]
}

/// Level 4: breaking the last line of defense before the enemy's home ground.
fn fourth_sector_conversation(assets: &GameAssets) -> Vec<DialogueLine> {
    let cap = |text| DialogueLine {
        speaker: "CAPTAIN",
        text,
        portrait: assets.captain.clone(),
    };
    let pilot = |text| DialogueLine {
        speaker: "PILOT",
        text,
        portrait: assets.hero.clone(),
    };
    vec![
        cap("This is their last line of defense before home ground."),
        pilot("Last lines break like any other. I'll find a way through."),
        cap("They'll throw everything they have left at you here."),
        pilot("Good thing I never learned how to slow down."),
        cap("Break through, and the end of this war is in sight. No pressure."),
        pilot("No pressure taken. Punching it - knock knock."),
    ]
}

/// Level 5: the final push into the last warlord's home sector.
fn final_sector_conversation(assets: &GameAssets) -> Vec<DialogueLine> {
    let cap = |text| DialogueLine {
        speaker: "CAPTAIN",
        text,
        portrait: assets.captain.clone(),
    };
    let pilot = |text| DialogueLine {
        speaker: "PILOT",
        text,
        portrait: assets.hero.clone(),
    };
    vec![
        cap("This is it, pilot. The last warlord's territory."),
        pilot("Four sectors of practice for this. I almost feel ready."),
        cap("Everything they have left stands between you and the end."),
        pilot("Then I won't have to go looking for them. Efficient."),
        cap("Finish this and the whole belt sleeps easy. Come back to us."),
        pilot("One last run. Punching it - let's end this war."),
    ]
}

/// The post-victory debrief, shown after the last warlord falls (ends at the title).
fn victory_conversation(assets: &GameAssets) -> Vec<DialogueLine> {
    let cap = |text| DialogueLine {
        speaker: "CAPTAIN",
        text,
        portrait: assets.captain.clone(),
    };
    let pilot = |text| DialogueLine {
        speaker: "PILOT",
        text,
        portrait: assets.hero.clone(),
    };
    vec![
        cap("Sensors confirm it - the last warlord is down. The belt is clear."),
        pilot("Copy that. Five sectors dark, and the paint's still mostly on."),
        cap("Five warlords, one fighter. They'll be telling this one for years."),
        pilot("Just doing my job, Captain. Though I won't stop the songs."),
        cap("Come on home, pilot. Debrief's on me - you've earned it."),
        pilot("Setting course for home. This legend could use a nap."),
    ]
}

/// The mid-game boss taunt (boss speaks, pilot answers).
fn boss_conversation(assets: &GameAssets) -> Vec<DialogueLine> {
    let boss = |text| DialogueLine {
        speaker: "WARLORD",
        text,
        portrait: assets.boss_portrait.clone(),
    };
    let pilot = |text| DialogueLine {
        speaker: "PILOT",
        text,
        portrait: assets.hero.clone(),
    };
    vec![
        boss("So, a lone fighter dares to enter my sector..."),
        pilot("Your reign over this sector ends today."),
        boss("Bold words. Let us see if your guns can back them up!"),
        pilot("Weapons hot. Resuming combat."),
    ]
}

/// Arm the pending briefing when a level's intro begins. On a fresh entry (game
/// start, death retry) the captain waits for the player to press Space; after a
/// boss kill the briefing opens once the new level's map has scrolled halfway in.
fn queue_intro(mut commands: Commands, post_boss: Option<Res<PostBossIntro>>) {
    let pending = if post_boss.is_some() {
        commands.remove_resource::<PostBossIntro>();
        PendingBriefing::AwaitScroll
    } else {
        PendingBriefing::AwaitKey
    };
    commands.insert_resource(pending);
}

/// Open the current level's briefing bubble on Space/Enter. Post-boss, the key
/// is armed only once the new band has scrolled in and its palette dissolve is
/// over (see [`PendingBriefing`] / [`DISSOLVE_MARGIN`]).
fn open_pending_briefing(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    camera: Query<&Transform, With<Camera2d>>,
    mut pending: ResMut<PendingBriefing>,
    assets: Res<GameAssets>,
    level: Res<Level>,
) {
    let pressed = keys.just_pressed(KeyCode::Space) || keys.just_pressed(KeyCode::Enter);
    let open = match &mut *pending {
        PendingBriefing::AwaitKey => pressed,
        PendingBriefing::AwaitScroll => {
            let settled = camera.single().is_ok_and(|cam| {
                cam.translation.y
                    >= level_bottom_y(level.0.saturating_sub(1)) + DISSOLVE_MARGIN
            });
            if settled {
                *pending = PendingBriefing::AwaitKey;
            }
            false
        }
    };
    if !open {
        return;
    }
    commands.remove_resource::<PendingBriefing>();
    let lines = level_intro(&assets, level.0);
    spawn_dialogue_ui(&mut commands, &assets, &lines[0], Phase::Intro);
    commands.insert_resource(ActiveDialogue {
        lines,
        index: 0,
        on_finish: Phase::Combat,
    });
}

/// Queues the post-victory debrief (shown in `Phase::Dialogue`; ends at the title).
/// Called by `boss::after_boss_defeated` when the final boss falls.
pub fn start_victory(commands: &mut Commands, assets: &GameAssets) {
    commands.insert_resource(ActiveDialogue {
        lines: victory_conversation(assets),
        index: 0,
        on_finish: Phase::Title,
    });
}

/// Press `T` during combat to start the boss conversation (pauses combat).
fn start_boss_dialogue(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    assets: Res<GameAssets>,
    mut next: ResMut<NextState<Phase>>,
) {
    if !keys.just_pressed(KeyCode::KeyT) {
        return;
    }
    commands.insert_resource(ActiveDialogue {
        lines: boss_conversation(&assets),
        index: 0,
        on_finish: Phase::Combat,
    });
    next.set(Phase::Dialogue);
}

/// Opens the boss speech bubble when entering `Phase::Dialogue`.
fn open_boss_dialogue(
    mut commands: Commands,
    assets: Res<GameAssets>,
    dialogue: Res<ActiveDialogue>,
) {
    let line = &dialogue.lines[dialogue.index];
    spawn_dialogue_ui(&mut commands, &assets, line, Phase::Dialogue);
}

/// One vertical slice of `dialogue.png`, stretched to fill its cell.
fn box_segment(image: Handle<Image>, min_x: f32) -> ImageNode {
    ImageNode {
        image,
        rect: Some(Rect {
            min: Vec2::new(min_x, DLG_CONTENT_TOP),
            max: Vec2::new(min_x + DLG_SEG_W, DLG_CONTENT_BOTTOM),
        }),
        image_mode: NodeImageMode::Stretch,
        ..default()
    }
}

/// Builds the speech-bubble UI at the top of the screen: a portrait on the left,
/// then the `dialogue.png` bubble with speaker/body text. `despawn` is the phase
/// whose exit tears the UI down.
fn spawn_dialogue_ui(
    commands: &mut Commands,
    assets: &GameAssets,
    line: &DialogueLine,
    despawn: Phase,
) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(16.0),
                left: Val::Px(24.0),
                right: Val::Px(24.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(12.0),
                ..default()
            },
            DespawnOnExit(despawn),
            Name::new("DialogueUi"),
        ))
        .with_children(|root| {
            // Portrait: the speaker's face pinned at (3,4) inside portrait_box.
            root.spawn((
                ImageNode::new(assets.portrait_box.clone()),
                Node {
                    width: Val::Px(DLG_FRAME),
                    height: Val::Px(DLG_FRAME),
                    flex_shrink: 0.0,
                    ..default()
                },
            ))
            .with_children(|frame| {
                frame.spawn((
                    ImageNode::new(line.portrait.clone()),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(DLG_INNER_X),
                        top: Val::Px(DLG_INNER_Y),
                        width: Val::Px(DLG_INNER),
                        height: Val::Px(DLG_INNER),
                        ..default()
                    },
                    PortraitImage,
                ));
            });

            // Speech bubble: dialogue.png box (to the right of the portrait) + text.
            root.spawn(Node {
                flex_grow: 1.0,
                height: Val::Px(DLG_HEIGHT),
                ..default()
            })
            .with_children(|bubble| {
                // Background: left cap / stretchable middle / right cap.
                bubble
                    .spawn(Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(0.0),
                        left: Val::Px(0.0),
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        flex_direction: FlexDirection::Row,
                        ..default()
                    })
                    .with_children(|bar| {
                        bar.spawn((
                            box_segment(assets.dialogue_box.clone(), DLG_CONTENT_LEFT),
                            Node {
                                width: Val::Px(DLG_CAP_W),
                                height: Val::Percent(100.0),
                                ..default()
                            },
                        ));
                        bar.spawn((
                            box_segment(assets.dialogue_box.clone(), DLG_CONTENT_LEFT + DLG_SEG_W),
                            Node {
                                flex_grow: 1.0,
                                height: Val::Percent(100.0),
                                ..default()
                            },
                        ));
                        bar.spawn((
                            box_segment(
                                assets.dialogue_box.clone(),
                                DLG_CONTENT_LEFT + 2.0 * DLG_SEG_W,
                            ),
                            Node {
                                width: Val::Px(DLG_CAP_W),
                                height: Val::Percent(100.0),
                                ..default()
                            },
                        ));
                    });

                // Text, laid over the bubble and vertically centered. The extra
                // bottom padding biases the text upward: pocod's glyphs sit low
                // in their line boxes, so a plain center hugs the bottom border.
                bubble
                    .spawn(Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(0.0),
                        left: Val::Px(0.0),
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        justify_content: JustifyContent::Center,
                        // No extra gap between the speaker name and the body (the
                        // lines' own boxes already separate them).
                        row_gap: Val::Px(0.0),
                        padding: UiRect {
                            left: Val::Px(12.0),
                            right: Val::Px(12.0),
                            top: Val::Px(0.0),
                            bottom: Val::Px(4.0),
                        },
                        ..default()
                    })
                    .with_children(|column| {
                        column.spawn((
                            Text::new(line.speaker),
                            TextFont {
                                font: assets.font.clone().into(),
                                font_size: FontSize::from(18.0),
                                ..default()
                            },
                            TextColor(Color::srgb(1.0, 0.85, 0.3)),
                            SpeakerText,
                        ));
                        column.spawn((
                            Text::new(line.text),
                            TextFont {
                                font: assets.font.clone().into(),
                                font_size: FontSize::from(15.0),
                                ..default()
                            },
                            TextColor(Color::WHITE),
                            BodyText,
                        ));
                    });
            });
        });
}

/// Advance on Space/Enter; after the last line, enter the conversation's
/// `on_finish` phase (Combat for briefings/taunts, Title for the victory debrief).
fn advance_dialogue(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut dialogue: ResMut<ActiveDialogue>,
    mut next: ResMut<NextState<Phase>>,
    mut speaker: Query<&mut Text, (With<SpeakerText>, Without<BodyText>)>,
    mut body: Query<&mut Text, (With<BodyText>, Without<SpeakerText>)>,
    mut portrait: Query<&mut ImageNode, With<PortraitImage>>,
) {
    if !keys.just_pressed(KeyCode::Space) && !keys.just_pressed(KeyCode::Enter) {
        return;
    }

    dialogue.index += 1;
    if dialogue.index >= dialogue.lines.len() {
        let finish = dialogue.on_finish;
        commands.remove_resource::<ActiveDialogue>();
        next.set(finish);
        return;
    }

    let line = &dialogue.lines[dialogue.index];
    if let Ok(mut text) = speaker.single_mut() {
        text.0 = line.speaker.to_string();
    }
    if let Ok(mut text) = body.single_mut() {
        text.0 = line.text.to_string();
    }
    if let Ok(mut image) = portrait.single_mut() {
        image.image = line.portrait.clone();
    }
}
