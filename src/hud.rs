//! Image-based HUD (bottom-left): a hero portrait in its frame with a segmented
//! health bar aligned to the bottom of the frame.
//!
//! The bar is a row of segments: `healthbar_1` (left cap), `healthbar_2`,
//! `healthbar_3`, then `healthbar_repeat` tiled for the rest — one segment per
//! point of [`MAX_HEALTH`]. Each segment shows its filled image while its index
//! is below current health, and its depleted (`_w`) image otherwise. The source
//! art has a 1px transparent separator on some segments, so we crop each to its
//! opaque width (via `ImageNode.rect`) to pack them with no gaps.

use bevy::prelude::*;

use crate::assets::GameAssets;
use crate::combat::{GameProgress, MAX_HEALTH};
use crate::common::VIEW_SCALE;
use crate::menu::SelectedFighter;
use crate::recolor::PaletteColor;
use crate::state::Phase;

// UI images are drawn at their native asset size times the single VIEW_SCALE, so
// every element keeps the source art's proportions and matches the 2x world zoom.
const PORTRAIT_SIZE: f32 = 35.0 * VIEW_SCALE; // portrait_box_2 is 35x35
const HERO_SIZE: f32 = 32.0 * VIEW_SCALE; // hero.png is 32x32
const HERO_INSET: f32 = 1.0 * VIEW_SCALE; // hero sits at (1,1) inside the frame
const SEG_HEIGHT: f32 = 11.0; // native segment height (source pixels)

/// Root of the HUD tree (rebuilt each level, so despawn the old one first).
#[derive(Component)]
struct HudRoot;

#[derive(Component)]
pub struct GameOverText;

#[derive(Component)]
pub struct RewardText;

/// One segment of the health bar.
#[derive(Component)]
struct HealthCell {
    index: u32,
    full: Handle<Image>,
    empty: Handle<Image>,
}

pub(super) fn plugin(app: &mut App) {
    // Built when the level begins (it persists across levels); torn down when the
    // game returns to the title so it doesn't overlay the cover art.
    app.add_systems(OnEnter(Phase::Intro), spawn_hud)
        .add_systems(OnEnter(Phase::Title), despawn_hud)
        .add_systems(
            Update,
            (update_health_cells, update_reward, update_game_over),
        );
}

/// Removes the HUD when returning to the title (e.g. after beating the last level).
#[allow(clippy::type_complexity)]
fn despawn_hud(
    mut commands: Commands,
    hud: Query<Entity, Or<(With<HudRoot>, With<GameOverText>)>>,
) {
    // The banner is a separate top-level entity (it centers on the screen, not
    // in the HUD corner), so it must be torn down alongside the HUD root —
    // otherwise it lingers over the title and duplicates on the next game.
    for entity in &hud {
        commands.entity(entity).try_despawn();
    }
}

/// Ordered `(filled, depleted, opaque width)` for each health segment. The filled
/// art is recolored to `color` so the bar matches the chosen fighter.
fn segments(assets: &GameAssets, color: PaletteColor) -> Vec<(Handle<Image>, Handle<Image>, f32)> {
    let mut cells = vec![
        (
            assets.healthbar_1.full(color),
            assets.healthbar_1.empty.clone(),
            7.0,
        ),
        (
            assets.healthbar_2.full(color),
            assets.healthbar_2.empty.clone(),
            6.0,
        ),
        (
            assets.healthbar_3.full(color),
            assets.healthbar_3.empty.clone(),
            6.0,
        ),
    ];
    for _ in cells.len() as u32..MAX_HEALTH {
        cells.push((
            assets.healthbar_repeat.full(color),
            assets.healthbar_repeat.empty.clone(),
            6.0,
        ));
    }
    cells
}

/// Crops a segment to its opaque `width` and stretches it to fill the cell, so
/// neighbours abut with no gap.
fn segment_image(image: Handle<Image>, width: f32) -> ImageNode {
    ImageNode {
        image,
        rect: Some(Rect {
            min: Vec2::ZERO,
            max: Vec2::new(width, SEG_HEIGHT),
        }),
        image_mode: NodeImageMode::Stretch,
        ..default()
    }
}

fn spawn_hud(
    mut commands: Commands,
    assets: Res<GameAssets>,
    selected: Res<SelectedFighter>,
    existing: Query<(), With<HudRoot>>,
) {
    // The HUD persists across levels (its systems keep it current), so only build
    // one when there isn't one yet.
    if !existing.is_empty() {
        return;
    }
    let color = selected.0;
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(14.0),
                left: Val::Px(12.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::FlexEnd, // health+reward column bottom-aligns with the frame
                column_gap: Val::Px(0.0),         // health bar immediately adjacent to the frame
                ..default()
            },
            HudRoot,
            Name::new("Hud"),
        ))
        .with_children(|root| {
            // Portrait: hero pinned at (1,1) inside the portrait_box_2 frame so the
            // 32x32 hero fits the 35x35 frame exactly.
            root.spawn((
                ImageNode::new(assets.portrait_box_2.clone()),
                Node {
                    width: Val::Px(PORTRAIT_SIZE),
                    height: Val::Px(PORTRAIT_SIZE),
                    ..default()
                },
            ))
            .with_children(|frame| {
                frame.spawn((
                    ImageNode::new(assets.hero.clone()),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(HERO_INSET),
                        top: Val::Px(HERO_INSET),
                        width: Val::Px(HERO_SIZE),
                        height: Val::Px(HERO_SIZE),
                        ..default()
                    },
                ));
            });

            // Column: health bar on top, reward value directly below it.
            root.spawn(Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                row_gap: Val::Px(4.0),
                ..default()
            })
            .with_children(|column| {
                // Health bar, segments packed tightly.
                column
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        ..default()
                    })
                    .with_children(|bar| {
                        for (index, (full, empty, width)) in
                            segments(&assets, color).into_iter().enumerate()
                        {
                            bar.spawn((
                                segment_image(full.clone(), width),
                                Node {
                                    width: Val::Px(width * VIEW_SCALE),
                                    height: Val::Px(SEG_HEIGHT * VIEW_SCALE),
                                    ..default()
                                },
                                HealthCell {
                                    index: index as u32,
                                    full,
                                    empty,
                                },
                            ));
                        }
                    });

                // Reward / money earned, in the pixel font.
                column.spawn((
                    Text::new("$0"),
                    TextFont {
                        font: assets.font.clone().into(),
                        font_size: FontSize::from(22.0),
                        ..default()
                    },
                    TextColor(Color::srgb(1.0, 0.85, 0.3)),
                    RewardText,
                ));
            });
        });

    // Game-over banner (hidden until needed; `palette_grade` tints it to the
    // current level's light shade, so it pops against the gray death palette).
    commands.spawn((
        Text::new("GAME OVER"),
        TextFont {
            font: assets.font.clone().into(),
            font_size: FontSize::from(36.0),
            ..default()
        },
        TextColor(Color::srgb(1.0, 0.4, 0.4)),
        TextLayout::default().with_justify(Justify::Center),
        // Full-width so the centered text stays centered regardless of font width.
        Node {
            position_type: PositionType::Absolute,
            top: Val::Percent(40.0),
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        Visibility::Hidden,
        GameOverText,
    ));
}

/// Swap each health segment between its filled and depleted image.
fn update_health_cells(
    progress: Res<GameProgress>,
    mut cells: Query<(&HealthCell, &mut ImageNode)>,
) {
    for (cell, mut image) in &mut cells {
        image.image = if cell.index < progress.health {
            cell.full.clone()
        } else {
            cell.empty.clone()
        };
    }
}

fn update_reward(progress: Res<GameProgress>, mut text: Query<&mut Text, With<RewardText>>) {
    if let Ok(mut text) = text.single_mut() {
        text.0 = format!("${}", progress.score);
    }
}

fn update_game_over(
    progress: Res<GameProgress>,
    mut banner: Query<&mut Visibility, With<GameOverText>>,
) {
    if let Ok(mut visibility) = banner.single_mut() {
        *visibility = if progress.game_over {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}
