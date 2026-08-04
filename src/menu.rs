//! The title screen (cover art + animated "ASTRO STRIKE" + start prompt).
//!
//! `Title` → press Space/Enter → `Intro` (the game begins straight away). The
//! player always flies the default fighter in [`SelectedFighter`].

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::assets::GameAssets;
use crate::common::sfx;
use crate::recolor::PaletteColor;
use crate::state::Phase;

/// The fighter the player flies. There is no in-game picker anymore, so this is
/// just the default model/color, read by `player::spawn_player` and the HUD.
#[derive(Resource)]
pub struct SelectedFighter(pub PaletteColor);

impl Default for SelectedFighter {
    fn default() -> Self {
        SelectedFighter(PaletteColor::ALL[0])
    }
}

/// A title word that slides in from off-screen.
#[derive(Component)]
struct TitleSlideIn {
    start_top: f32,
    end_top: f32,
    delay: f32,
    duration: f32,
    elapsed: f32,
}

/// The "PRESS SPACE TO START" prompt that fades in, then blinks on and off.
#[derive(Component)]
struct TitleStartPrompt {
    duration: f32,
    elapsed: f32,
}

const TITLE_SLIDE_DURATION: f32 = 2.1;
const TITLE_STAGGER: f32 = 0.22;
/// Negative top margin pulling "STRIKE" up toward "ASTRO" so the stacked title
/// words sit close together.
const TITLE_WORD_PULL: f32 = -10.0;
const TITLE_PROMPT_FADE_DURATION: f32 = 0.7;
/// Blink cycle (secs) and the fraction of it the prompt is visible (arcade-style).
const TITLE_PROMPT_BLINK_PERIOD: f32 = 0.85;
const TITLE_PROMPT_BLINK_ON: f32 = 0.6;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<SelectedFighter>()
        .add_systems(OnEnter(Phase::Title), spawn_title)
        .add_systems(
            Update,
            (title_input, animate_title, animate_title_prompt).run_if(in_state(Phase::Title)),
        );
}

fn spawn_title(
    mut commands: Commands,
    assets: Res<GameAssets>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let window_height = windows.single().map(|w| w.height()).unwrap_or(1080.0);

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            // Solid backdrop so the transparent parts of the cover don't reveal
            // the game world behind the title screen.
            BackgroundColor(Color::srgb(0.02, 0.02, 0.06)),
            DespawnOnExit(Phase::Title),
            Name::new("TitleScreen"),
        ))
        .with_children(|root| {
            // Cover art, stretched to fill the screen behind the prompt.
            root.spawn((
                ImageNode {
                    image: assets.game_cover.clone(),
                    image_mode: NodeImageMode::Stretch,
                    ..default()
                },
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
            ));
            // Title + "Start Game" prompt, stacked on the right side of the cover
            // (the moon art sits on the left, leaving the right half for text).
            root.spawn((Node {
                position_type: PositionType::Absolute,
                top: Val::Px(48.0),
                bottom: Val::Px(0.0),
                right: Val::Px(32.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: Val::Px(18.0),
                ..default()
            },))
                .with_children(|panel| {
                    panel
                        .spawn(Node {
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            ..default()
                        })
                        .with_children(|title| {
                            title
                                .spawn((
                                    Node {
                                        position_type: PositionType::Relative,
                                        top: Val::Px(-window_height),
                                        ..default()
                                    },
                                    TitleSlideIn {
                                        start_top: -window_height,
                                        end_top: 0.0,
                                        delay: 0.0,
                                        duration: TITLE_SLIDE_DURATION,
                                        elapsed: 0.0,
                                    },
                                ))
                                .with_children(|word| {
                                    word.spawn((
                                        Text::new("ASTRO"),
                                        TextFont {
                                            font: assets.font.clone().into(),
                                            font_size: FontSize::from(55.0),
                                            ..default()
                                        },
                                        TextColor(Color::srgb(
                                            178.0 / 255.0,
                                            197.0 / 255.0,
                                            210.0 / 255.0,
                                        )),
                                        TextLayout::default().with_justify(Justify::Center),
                                    ));
                                });

                            title
                                .spawn((
                                    Node {
                                        position_type: PositionType::Relative,
                                        top: Val::Px(window_height),
                                        // Pull STRIKE up toward ASTRO so the
                                        // stacked words sit close together.
                                        margin: UiRect::top(Val::Px(TITLE_WORD_PULL)),
                                        ..default()
                                    },
                                    TitleSlideIn {
                                        start_top: window_height,
                                        end_top: 0.0,
                                        delay: TITLE_STAGGER,
                                        duration: TITLE_SLIDE_DURATION,
                                        elapsed: 0.0,
                                    },
                                ))
                                .with_children(|word| {
                                    word.spawn((
                                        Text::new("STRIKE"),
                                        TextFont {
                                            font: assets.font.clone().into(),
                                            font_size: FontSize::from(55.0),
                                            ..default()
                                        },
                                        TextColor(Color::srgb(
                                            178.0 / 255.0,
                                            197.0 / 255.0,
                                            210.0 / 255.0,
                                        )),
                                        TextLayout::default().with_justify(Justify::Center),
                                    ));
                                });
                        });
                    panel.spawn((
                        Text::new("PRESS SPACE TO START"),
                        TextFont {
                            font: assets.font.clone().into(),
                            font_size: FontSize::from(18.0),
                            ..default()
                        },
                        TextColor(Color::srgba(0.85, 0.85, 0.9, 0.0)),
                        TitleStartPrompt {
                            duration: TITLE_PROMPT_FADE_DURATION,
                            elapsed: 0.0,
                        },
                    ));
                });
        });
}

fn title_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    assets: Res<GameAssets>,
    mut next: ResMut<NextState<Phase>>,
) {
    if keys.just_pressed(KeyCode::Space) || keys.just_pressed(KeyCode::Enter) {
        commands.spawn(sfx(assets.confirm_sfx.clone()));
        next.set(Phase::Intro);
    }
}

fn animate_title(time: Res<Time>, mut titles: Query<(&mut Node, &mut TitleSlideIn)>) {
    for (mut node, mut slide) in &mut titles {
        let elapsed = slide.elapsed + time.delta_secs();
        slide.elapsed = elapsed;
        let t = if elapsed <= slide.delay {
            0.0
        } else if slide.duration > 0.0 {
            ((elapsed - slide.delay) / slide.duration).clamp(0.0, 1.0)
        } else {
            1.0
        };

        // Back-ease-out for a subtle cinematic overshoot on arrival.
        let c1 = 1.70158;
        let c3 = c1 + 1.0;
        let eased = 1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2);
        node.top = Val::Px(slide.start_top + (slide.end_top - slide.start_top) * eased);
    }
}

fn animate_title_prompt(
    time: Res<Time>,
    titles: Query<&TitleSlideIn>,
    mut prompt: Query<(&mut TitleStartPrompt, &mut TextColor)>,
) {
    let titles_done = !titles.is_empty()
        && titles
            .iter()
            .all(|slide| slide.elapsed >= slide.delay + slide.duration);

    for (mut reveal, mut color) in &mut prompt {
        if !titles_done {
            continue;
        }

        reveal.elapsed += time.delta_secs();
        let t = if reveal.duration > 0.0 {
            (reveal.elapsed / reveal.duration).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let fade_alpha = t * t * (3.0 - 2.0 * t);
        let alpha = if t < 1.0 {
            // Fade in smoothly once the title has settled.
            fade_alpha
        } else {
            // Then blink fully on/off as a clear "ready to start" cue.
            let phase = ((reveal.elapsed - reveal.duration) / TITLE_PROMPT_BLINK_PERIOD).fract();
            if phase < TITLE_PROMPT_BLINK_ON {
                1.0
            } else {
                0.0
            }
        };
        color.0 = Color::srgba(0.85, 0.85, 0.9, alpha);
    }
}
