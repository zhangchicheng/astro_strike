//! Asset preloading (see design.md "Asset Preloading" pattern).
//!
//! Handles for the sprite-sheet / UI images that gameplay spawns at runtime are
//! loaded once up front and stored in a resource, so there is no hitch the first
//! time an explosion plays, a ship spawns, or a dialogue opens.

use bevy::prelude::*;

use crate::animation::frames_from_aseprite_json;
use crate::recolor::{PaletteColor, Palettes, recolor_png};

/// Ships used for enemies, each paired with the projectile it fires (the round
/// slug `projectile_4.png` or the short bolt `projectile_3.png` — a fixed trait
/// of the model, not random). Native sizes are read from the PNG headers at
/// load time (the art gets re-exported often — hardcoded sizes kept going
/// stale and a wrong size draws stretched/smeared). Every `spaceship_N.png`
/// pairs with a `flame_N.png` thruster strip: two frames side by side, drawn
/// at the ship's tail. All ship art points "up", so enemies are flipped down.
const ENEMY_SHIPS: &[(&str, &str)] = &[
    ("spaceships/spaceship_2.png", "projectiles/projectile_4.png"),
    ("spaceships/spaceship_3.png", "projectiles/projectile_4.png"),
    ("spaceships/spaceship_4.png", "projectiles/projectile_3.png"),
    ("spaceships/spaceship_5.png", "projectiles/projectile_3.png"),
    ("spaceships/spaceship_6.png", "projectiles/projectile_3.png"),
    ("spaceships/spaceship_7.png", "projectiles/projectile_4.png"),
    ("spaceships/spaceship_8.png", "projectiles/projectile_4.png"),
    ("spaceships/spaceship_9.png", "projectiles/projectile_4.png"),
    ("spaceships/spaceship_10.png", "projectiles/projectile_3.png"),
];

/// The UFO sprites. `ufo_1` is a 32x16 two-frame animation; the rest are single
/// 16x16 frames. Kept in their native (blue/purple) palette so the alien saucers
/// read as visually distinct from the red enemy ships.
const UFO_SHIPS: &[&str] = &[
    "spaceships/ufo_1.png", // animated (2 frames)
    "spaceships/ufo_2.png",
    "spaceships/ufo_3.png",
    "spaceships/ufo_4.png",
];

/// The four selectable player fighters: distinct ship models (`spaceship_0..3`),
/// each recolored to one palette so it also reads as a distinct color (which the
/// health bar matches). Listed in the order shown on the select screen. Sizes
/// are read from the PNG headers at load time.
const PLAYER_SHIPS: &[(&str, PaletteColor)] = &[
    ("spaceships/spaceship_0.png", PaletteColor::Blue),
    ("spaceships/spaceship_1.png", PaletteColor::Red),
    ("spaceships/spaceship_2.png", PaletteColor::Green),
    ("spaceships/spaceship_3.png", PaletteColor::Yellow),
];

/// Themed color for each pickup, so players can tell them apart at a glance.
/// Order matches `pickup::PickupKind`: Speed, Repair, Weapon, Bomb.
const PICKUP_COLORS: [PaletteColor; 4] = [
    PaletteColor::Yellow, // Speed (boots)
    PaletteColor::Green,  // Repair (orb) — the "restore health" pickup
    PaletteColor::Red,    // Weapon (arrow)
    PaletteColor::Blue,   // Bomb (teardrop)
];

/// Native size of a PNG under `assets/`, read straight from its IHDR header
/// (width/height as big-endian u32 at bytes 16/20 — no decoding needed). The
/// single source of truth for art dimensions, so re-exported files can't go
/// stale against hardcoded sizes.
fn png_size(file: &str) -> Option<Vec2> {
    let bytes = crate::vfs::read_bytes(file)?;
    let dim = |at: usize| -> Option<u32> {
        Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
    };
    Some(Vec2::new(dim(16)? as f32, dim(20)? as f32))
}

/// Like [`png_size`], but loud about a missing/unreadable file and falling back
/// to a visible-but-wrong placeholder instead of crashing.
fn png_size_or_warn(file: &str) -> Vec2 {
    png_size(file).unwrap_or_else(|| {
        error!("could not read PNG header of `{file}`; using 16x16");
        Vec2::splat(16.0)
    })
}

/// A ship's thruster flame: a two-frame strip (frames side by side, each one
/// ship-width wide) played in a loop at the ship's tail. The art is pure white,
/// so `palette_grade` tints it with the level's white accent.
#[derive(Clone)]
pub struct FlameArt {
    pub image: Handle<Image>,
    /// One frame's native size.
    pub size: Vec2,
}

/// Seconds per thruster-flame frame.
const FLAME_FRAME_SECS: f32 = 0.12;

impl FlameArt {
    /// The two animation frames `(source rect, seconds)`: the left and right
    /// halves of the strip.
    pub fn frames(&self) -> Vec<(Rect, f32)> {
        let frame = |i: f32| Rect {
            min: Vec2::new(i * self.size.x, 0.0),
            max: Vec2::new((i + 1.0) * self.size.x, self.size.y),
        };
        vec![(frame(0.0), FLAME_FRAME_SECS), (frame(1.0), FLAME_FRAME_SECS)]
    }
}

/// An enemy ship's art: sprite, native pixel size, thruster flame, and the
/// projectile it fires.
#[derive(Clone)]
pub struct ShipArt {
    pub image: Handle<Image>,
    pub size: Vec2,
    pub flame: FlameArt,
    pub bullet: BulletArt,
}

/// A selectable player fighter: its recolored sprite, native pixel size (so it's
/// drawn at the right aspect), palette color (drives the health-bar tint), and
/// thruster flame.
pub struct Fighter {
    pub color: PaletteColor,
    pub image: Handle<Image>,
    pub size: Vec2,
    pub flame: FlameArt,
}

/// A UFO's art: its sprite and, for `ufo_1`, its two animation frames (the other
/// UFOs are single-frame, so `frames` is `None`).
pub struct UfoArt {
    pub image: Handle<Image>,
    pub frames: Option<Vec<(Rect, f32)>>,
}

/// One projectile style: its sprite, native draw size, and hit box (sized to
/// the opaque content, since the tall bolt sprites are mostly transparent canvas).
#[derive(Clone)]
pub struct BulletArt {
    pub image: Handle<Image>,
    pub size: Vec2,
    pub collider: Vec2,
}

/// One health-bar segment: a filled image per selectable fighter palette (so the
/// bar matches the chosen ship's color) plus the shared depleted (`_w`) image.
pub struct HealthSeg {
    pub fulls: Vec<(PaletteColor, Handle<Image>)>,
    pub empty: Handle<Image>,
}

impl HealthSeg {
    /// The filled segment recolored to `color` (falls back to the first palette).
    pub fn full(&self, color: PaletteColor) -> Handle<Image> {
        self.fulls
            .iter()
            .find(|(c, _)| *c == color)
            .or_else(|| self.fulls.first())
            .map(|(_, handle)| handle.clone())
            .unwrap_or_default()
    }
}

#[derive(Resource)]
pub struct GameAssets {
    /// 7-frame 16x16 explosion strip (`explosion_1.png`, 112x16).
    pub explosion: Handle<Image>,
    /// 5-frame 16x16 UFO explosion strip (`explosion_2.png`, 80x16).
    pub explosion_2: Handle<Image>,
    /// The boss (`boss/boss.png`, one image) and its awakened variant, plus the
    /// image's native size (auto-read; the hull is inked centered with
    /// transparent side margins).
    pub boss: Handle<Image>,
    pub boss_activated: Handle<Image>,
    pub boss_size: Vec2,
    /// Boss attack orb (`boss_projectile.png`, 64x16, four 16x16 frames).
    pub boss_projectile: Handle<Image>,
    /// Boss health-bar art (`boss_health_bar.png`, 64x16; frame 3-slice + fill swatches).
    pub boss_health_bar: Handle<Image>,
    /// Selectable player fighters: `spaceship_0..3`, each recolored per palette.
    pub fighters: Vec<Fighter>,
    /// Enemy ships with their native sizes and thruster flames.
    pub enemy_ships: Vec<ShipArt>,
    /// UFO sprites (native palette); `ufo::spawn_ufo_swarms` picks one per swarm.
    pub ufos: Vec<UfoArt>,
    /// Animated projectile sheet (`projectile_1.png`, 64x16, four 16x16 frames).
    /// Shared by the player's missile and the tank's shell.
    pub projectile: Handle<Image>,
    /// Missile frames `(source rect, seconds)` parsed from `projectile_1.json`.
    pub projectile_frames: Vec<(Rect, f32)>,
    /// The player's bullet (`projectile_2.png`, the long energy bolt, fired up).
    pub player_bullet: BulletArt,
    /// Pickup strips (`pickups.png`, 64x16, four 16x16 frames). One copy recolored
    /// per pickup kind; indexed by `pickup::PickupKind as usize` (see `PICKUP_COLORS`).
    pub pickups: Vec<Handle<Image>>,

    // ----- Menu -----
    /// Title-screen cover art (`game_cover.png`, 400x300).
    pub game_cover: Handle<Image>,

    // ----- Audio -----
    /// Player fire sound (`laser_1.wav`).
    pub laser: Handle<AudioSource>,
    /// Enemy explosion sound (`explosion_1.wav`).
    pub explosion_sound: Handle<AudioSource>,
    /// Title confirm chime and game-over jingle (Arcade_Sound_FX).
    pub confirm_sfx: Handle<AudioSource>,
    pub game_over_sfx: Handle<AudioSource>,
    /// Played when the player collects a pickup (`collect_1.wav`).
    pub pickup_sfx: Handle<AudioSource>,
    /// Looping background music (`ChillMenu.wav`).
    pub music: Handle<AudioSource>,

    // ----- HUD / dialogue -----
    /// Hero portrait (`hero.png`, 32x32) and the HUD frame it sits in.
    pub hero: Handle<Image>,
    /// HUD portrait frame (`portrait_box_2.png`, 35x35).
    pub portrait_box_2: Handle<Image>,
    /// Boss portrait content (`boss.png`, 32x32) and its dialogue frame.
    pub boss_portrait: Handle<Image>,
    /// Captain portrait for the intro briefing (`captain.png`, 32x32).
    pub captain: Handle<Image>,
    pub portrait_box: Handle<Image>,
    /// Dialogue box frame (`dialogue.png`, opaque content 42x37 at offset 6,2).
    pub dialogue_box: Handle<Image>,
    /// Health-bar segments; the filled art (`#6070A8`) is recolored per fighter
    /// palette so the bar matches the chosen ship, while `_w` stays the depleted color.
    pub healthbar_1: HealthSeg,
    pub healthbar_2: HealthSeg,
    pub healthbar_3: HealthSeg,
    pub healthbar_repeat: HealthSeg,
    /// Pixel font (pocod) for all in-game text.
    pub font: Handle<Font>,
}

impl FromWorld for GameAssets {
    fn from_world(world: &mut World) -> Self {
        // AssetServer is a cheap handle to clone, freeing the world borrow so we
        // can also take Assets<Image> mutably for the recolored (palette-swapped)
        // sprites.
        let assets = world.resource::<AssetServer>().clone();
        let palettes = Palettes::DEFAULT;
        let mut images = world.resource_mut::<Assets<Image>>();

        // Recolor a sprite to a target palette, falling back to the raw asset.
        let recolor = |images: &mut Assets<Image>, file: &str, color: PaletteColor| {
            recolor_png(images, file, palettes.default, palettes.get(color))
                .unwrap_or_else(|| assets.load(file.to_string()))
        };

        // A ship's flame strip sits next to it: `spaceship_N.png` -> `flame_N.png`.
        // Loaded plain — the art is pure white, tinted at runtime by the shader.
        // One frame = half the strip; sizes come from the PNG headers.
        let flame = |assets: &AssetServer, ship_path: &str| {
            let path = ship_path.replace("spaceship_", "flame_");
            let strip = png_size_or_warn(&path);
            FlameArt {
                image: assets.load(path),
                size: Vec2::new(strip.x / 2.0, strip.y),
            }
        };

        // Projectile art: `bullet` builds one style. The draw size is the PNG's
        // native size (auto-read, like the ships); the collider is explicit when
        // the opaque content is smaller than the canvas, `None` = the full size.
        let bullet = |path: &str, collider: Option<Vec2>| {
            let size = png_size_or_warn(path);
            BulletArt {
                image: assets.load(path.to_string()),
                size,
                collider: collider.unwrap_or(size),
            }
        };
        let bolt = bullet("projectiles/projectile_3.png", Some(Vec2::new(4.0, 16.0)));
        let slug = bullet("projectiles/projectile_4.png", None);

        // Enemies are red; the player and its pilot are blue; the captain yellow;
        // the boss/warlord red.
        let enemy_ships = ENEMY_SHIPS
            .iter()
            .map(|&(path, projectile)| ShipArt {
                image: recolor(&mut images, path, PaletteColor::Red),
                size: png_size_or_warn(path),
                flame: flame(&assets, path),
                bullet: if projectile == "projectiles/projectile_4.png" {
                    slug.clone()
                } else {
                    bolt.clone()
                },
            })
            .collect();
        // UFOs keep their native palette (loaded as-is). `ufo_1` is a 32x16 strip
        // of two 16x16 frames; the rest are single 16x16 sprites.
        let ufo_frame = |i: f32| Rect {
            min: Vec2::new(i * 16.0, 0.0),
            max: Vec2::new(i * 16.0 + 16.0, 16.0),
        };
        let ufos = UFO_SHIPS
            .iter()
            .map(|&path| UfoArt {
                image: assets.load(path.to_string()),
                frames: if path.ends_with("ufo_1.png") {
                    Some(vec![(ufo_frame(0.0), 0.18), (ufo_frame(1.0), 0.18)])
                } else {
                    None
                },
            })
            .collect();
        // Four distinct ship models, each recolored to its palette.
        let fighters = PLAYER_SHIPS
            .iter()
            .map(|&(path, color)| Fighter {
                color,
                image: recolor(&mut images, path, color),
                size: png_size_or_warn(path),
                flame: flame(&assets, path),
            })
            .collect();
        let boss = recolor(&mut images, "boss/boss.png", PaletteColor::Red);
        let boss_activated = recolor(&mut images, "boss/boss_activated.png", PaletteColor::Red);
        let boss_size = png_size_or_warn("boss/boss.png");
        // Boss attack orb keeps its own white art; the health-bar frame is
        // recolored red to match the boss (its white/black fill swatches are left).
        let boss_projectile = assets.load("boss/boss_projectile.png");
        let boss_health_bar = recolor(&mut images, "boss/boss_health_bar.png", PaletteColor::Red);
        let hero = recolor(&mut images, "ui/hero.png", PaletteColor::Blue);
        let boss_portrait = recolor(&mut images, "ui/boss.png", PaletteColor::Red);
        let captain = recolor(&mut images, "ui/captain.png", PaletteColor::Yellow);
        // One recolored copy of the pickup strip per kind's theme color.
        let pickups = PICKUP_COLORS
            .iter()
            .map(|&color| recolor(&mut images, "effects/pickups.png", color))
            .collect();

        // Effects, UI chrome and audio keep their own art (loaded as-is).
        let explosion = assets.load("effects/explosion_1.png");
        let explosion_2 = assets.load("effects/explosion_2.png");
        let projectile = assets.load("projectiles/projectile_1.png");
        // Three enemy bullet styles: a long energy bolt with trail dashes, a short
        // bolt with a trailing dot, and the classic round-headed slug (the old
        // `bullet.png`, renamed). The spinning `projectile_1` stays reserved for
        // the player's missiles and the tanks' shells.
        let player_bullet = bullet("projectiles/projectile_0.png", None);
        let laser = assets.load("audio/laser_1.wav");
        let explosion_sound = assets.load("audio/explosion_1.wav");
        let game_cover = assets.load("ui/game_cover.png");
        let confirm_sfx = assets.load("audio/Arcade_Sound_FX/confirm_1.wav");
        let game_over_sfx = assets.load("audio/Arcade_Sound_FX/lose_1.wav");
        let pickup_sfx = assets.load("audio/Arcade_Sound_FX/collect_1.wav");
        let music = assets.load("audio/ChillMenu.wav");
        let portrait_box_2 = assets.load("ui/portrait_box_2.png");
        let portrait_box = assets.load("ui/portrait_box.png");
        let dialogue_box = assets.load("ui/dialogue.png");

        // The Aseprite JSON is read straight from disk (not via the asset
        // server), so resolve it against the same asset root.
        let projectile_frames =
            frames_from_aseprite_json("projectiles/projectile_1.json");
        // Each filled segment is recolored to all four fighter palettes; the HUD
        // picks the one matching the selected ship. The depleted `_w` art is white.
        let seg = |images: &mut Assets<Image>, name: &str| HealthSeg {
            fulls: PaletteColor::ALL
                .iter()
                .map(|&color| (color, recolor(images, &format!("{name}.png"), color)))
                .collect(),
            empty: assets.load(format!("{name}_w.png")),
        };
        let healthbar_1 = seg(&mut images, "ui/healthbar_1");
        let healthbar_2 = seg(&mut images, "ui/healthbar_2");
        let healthbar_3 = seg(&mut images, "ui/healthbar_3");
        let healthbar_repeat = seg(&mut images, "ui/healthbar_repeat");
        // `pocod-fixed.ttf` is a patched copy of pocod.ttf: the original leaves the
        // space character unmapped, and Bevy's shaper then draws the `.notdef`
        // glyph (a dash) for every space. The copy blanks `.notdef`'s outline.
        let font = assets.load("fonts/pocod-fixed.ttf");

        Self {
            explosion,
            explosion_2,
            boss,
            boss_activated,
            boss_size,
            boss_projectile,
            boss_health_bar,
            fighters,
            enemy_ships,
            ufos,
            projectile,
            projectile_frames,
            player_bullet,
            pickups,
            game_cover,
            laser,
            explosion_sound,
            confirm_sfx,
            game_over_sfx,
            pickup_sfx,
            music,
            hero,
            portrait_box_2,
            boss_portrait,
            captain,
            portrait_box,
            dialogue_box,
            healthbar_1,
            healthbar_2,
            healthbar_3,
            healthbar_repeat,
            font,
        }
    }
}

impl GameAssets {
    /// The selectable fighter for `color` (falls back to the first fighter).
    pub fn fighter(&self, color: PaletteColor) -> &Fighter {
        self.fighters
            .iter()
            .find(|f| f.color == color)
            .or_else(|| self.fighters.first())
            .expect("at least one fighter is always loaded")
    }
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<GameAssets>();
}
