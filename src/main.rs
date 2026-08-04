//! ASTRO STRIKE — a small 2D space shooter built with the Bevy engine.
//!
//! Code is organized into function-plugins (one concern per file), following the
//! Bevy template's plugin-organization pattern (see design.md).
//!
//! Features:
//!   * Square-based shooter over a Tiled (`.tmx`) level.
//!   * Parallax scrolling of the map layers (factors read from the TMX).
//!   * Sprite-sheet animation: animated map tiles + impact explosions.
//!   * A boss dialogue system that pauses combat and resumes it afterwards.
//!
//! Controls
//!   Arrow keys / WASD .... move the ship (parallax reacts to movement)
//!   Space ................ fire (weapon upgrades come from pickups)
//!   T .................... talk to the boss (pauses combat)
//!   Space / Enter ........ advance dialogue
//!   Space / Enter ........ return to the title after a game over

use bevy::prelude::*;
use rand::Rng;

mod animation;
mod assets;
mod boss;
mod combat;
mod common;
mod crt;
mod destructible;
mod dialogue;
mod enemy;
mod hud;
mod level;
mod menu;
mod music;
mod palette_grade;
mod parallax;
mod pickup;
mod player;
mod recolor;
mod state;
mod tilemap;
mod ufo;
mod vfs;
mod vehicle;

use common::{CAMERA_START_Y, HEIGHT, MAP_HALF, MAP_ROWS, VIEW_SCALE, WIDTH};
use parallax::ParallaxLayer;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Astro Strike".to_string(),
                        resolution: (WIDTH as u32, HEIGHT as u32).into(),
                        resizable: false,
                        ..default()
                    }),
                    ..default()
                })
                // Crisp pixel-art scaling instead of the default blurry linear filter.
                .set(ImagePlugin::default_nearest()),
        )
        .add_plugins(bevy_ecs_tilemap::prelude::TilemapPlugin)
        .insert_resource(ClearColor(Color::srgb(0.02, 0.02, 0.06)))
        .add_systems(Startup, setup_world)
        // Feature plugins (grouped into nested tuples to stay under the 15-element
        // limit on `add_plugins` tuples).
        .add_plugins((
            (
                state::plugin,
                assets::plugin,
                tilemap::plugin,
                palette_grade::plugin,
                crt::plugin,
                parallax::plugin,
                animation::plugin,
                player::plugin,
                enemy::plugin,
                combat::plugin,
            ),
            (
                destructible::plugin,
                boss::plugin,
                vehicle::plugin,
                ufo::plugin,
                pickup::plugin,
                dialogue::plugin,
                hud::plugin,
                menu::plugin,
                music::plugin,
                level::plugin,
            ),
        ))
        .run();
}

/// Camera and a decorative starfield. The stars are the slowest parallax layer
/// (they sit "farthest away"), so they barely drift as the camera scrolls.
fn setup_world(mut commands: Commands, asset_server: Res<AssetServer>) {
    // Zoom the camera in so the world is drawn VIEW_SCALE times larger. Start it
    // at the bottom of the level; `parallax::auto_scroll` scrolls it upward.
    commands.spawn((
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scale: 1.0 / VIEW_SCALE,
            ..OrthographicProjection::default_2d()
        }),
        Transform::from_xyz(0.0, CAMERA_START_Y, 0.0),
    ));

    // Parent that the parallax system scrolls; stars are its children.
    let starfield = commands
        .spawn((
            Transform::from_xyz(0.0, 0.0, -30.0),
            Visibility::default(),
            ParallaxLayer {
                factor: Vec2::splat(0.1),
                base: Vec2::ZERO,
            },
            Name::new("Starfield"),
        ))
        .id();

    // Each star is a random 3x3 tile from the 3x2 grid in `stars.png`. The art
    // uses the default palette + white, so `palette_grade` recolors the whole
    // field to the level's scheme for free — the sprites stay untinted (a color
    // tint would knock the pixels off the palette).
    let stars_image = asset_server.load("effects/stars.png");
    let mut rng = rand::thread_rng();
    let spread = MAP_HALF * 1.3;
    for _ in 0..(160 * MAP_ROWS) {
        let x = rng.gen_range(-spread.x..spread.x);
        let y = rng.gen_range(-spread.y..spread.y);
        let star = rng.gen_range(0..6u32);
        let min = Vec2::new((star % 3) as f32 * 3.0, (star / 3) as f32 * 3.0);
        commands.spawn((
            Sprite {
                image: stars_image.clone(),
                rect: Some(Rect {
                    min,
                    max: min + Vec2::splat(3.0),
                }),
                ..default()
            },
            Transform::from_xyz(x, y, 0.0),
            ChildOf(starfield),
        ));
    }
}
