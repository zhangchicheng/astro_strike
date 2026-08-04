//! Palette swapping.
//!
//! Every asset is drawn in a default 2-shade palette (`#383070` / `#6070A8`).
//! [`Palettes::DEFAULT`] lists that default plus four target palettes; we recolor
//! each asset at load time by swapping its two default shades for a target's two
//! shades, so nothing keeps the default color. Pixels that aren't a default shade
//! (white highlights, black outlines, transparency) are left untouched.

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;

/// An `[r, g, b]` color.
pub type Shade = [u8; 3];
/// A palette's `[dark, light]` shades.
pub type Pair = [Shade; 2];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PaletteColor {
    Red,
    Yellow,
    Green,
    Blue,
}

impl PaletteColor {
    /// All colors, in the order shown on the fighter-select screen.
    pub const ALL: [PaletteColor; 4] = [
        PaletteColor::Blue,
        PaletteColor::Red,
        PaletteColor::Green,
        PaletteColor::Yellow,
    ];
}

pub struct Palettes {
    pub default: Pair,
    pub red: Pair,
    pub green: Pair,
    pub yellow: Pair,
    pub blue: Pair,
}

impl Palettes {
    /// Every palette the art gets recolored to (formerly `assets/palettes.ron`;
    /// hardcoded since the palette-grade shader made runtime edits pointless).
    pub const DEFAULT: Palettes = Palettes {
        default: [[0x38, 0x30, 0x70], [0x60, 0x70, 0xA8]],
        red: [[0xE5, 0x33, 0x1F], [0xF0, 0x8B, 0x79]],
        green: [[0x00, 0x66, 0x33], [0x72, 0xA7, 0x84]],
        yellow: [[0xF9, 0xA2, 0x04], [0xFB, 0xC8, 0x6A]],
        blue: [[0x02, 0x6C, 0x80], [0x73, 0xAB, 0xAE]],
    };

    pub fn get(&self, color: PaletteColor) -> Pair {
        match color {
            PaletteColor::Red => self.red,
            PaletteColor::Yellow => self.yellow,
            PaletteColor::Green => self.green,
            PaletteColor::Blue => self.blue,
        }
    }
}

/// "White" in the source art (highlights, stars, glows) — the assets use
/// `#F8F8F8`, not pure white.
pub const WHITE: Shade = [248, 248, 248];

/// The map's palette when the player has lost: the whole world drains to gray.
pub const GAME_OVER_PAIR: Pair = [[79, 79, 79], [133, 133, 133]];

/// A level's map color scheme: what the two default primaries become across the
/// whole band (terrain, buildings, units, and space bodies alike), plus an
/// optional remap of pure-white pixels (highlights/stars).
pub struct LevelScheme {
    pub pair: Pair,
    pub white: Option<Shade>,
}

/// The per-level map schemes (1-based; out-of-range levels keep the default art).
pub fn level_scheme(level: u32) -> LevelScheme {
    match level {
        1 => LevelScheme {
            pair: [[47, 111, 69], [97, 168, 121]],
            white: None,
        },
        2 => LevelScheme {
            pair: [[111, 47, 107], [168, 153, 97]],
            white: Some([194, 251, 254]),
        },
        // Level 3 keeps the default primaries; only white shifts.
        3 => LevelScheme {
            pair: [[56, 48, 112], [96, 112, 168]],
            white: Some([241, 254, 165]),
        },
        4 => LevelScheme {
            pair: [[111, 47, 47], [97, 144, 168]],
            white: None,
        },
        5 => LevelScheme {
            pair: [[53, 29, 50], [168, 97, 97]],
            white: None,
        },
        _ => LevelScheme {
            pair: [[56, 48, 112], [96, 112, 168]],
            white: None,
        },
    }
}

/// Loads a PNG from disk and swaps its default palette (`from`) for `to`.
/// Returns `None` if the file can't be read or decoded.
pub fn recolor_png(
    images: &mut Assets<Image>,
    filename: &str,
    from: Pair,
    to: Pair,
) -> Option<Handle<Image>> {
    recolor_png_map(images, filename, &[(from[0], to[0]), (from[1], to[1])])
}

/// Loads a PNG from disk and applies each exact `(from, to)` color substitution.
/// Pixels are matched against the *original* colors, so mappings never chain.
/// Returns `None` if the file can't be read or decoded.
pub fn recolor_png_map(
    images: &mut Assets<Image>,
    filename: &str,
    mappings: &[(Shade, Shade)],
) -> Option<Handle<Image>> {
    let bytes = crate::vfs::read_bytes(filename)?;
    let mut image = Image::from_buffer(
        bytes.as_slice(),
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true, // is_srgb: stored bytes are the sRGB hex values we compare against
        ImageSampler::nearest(),
        RenderAssetUsages::default(),
    )
    .ok()?;

    if let Some(data) = image.data.as_mut() {
        for px in data.chunks_exact_mut(4) {
            let rgb = [px[0], px[1], px[2]];
            if let Some((_, to)) = mappings.iter().find(|(from, _)| *from == rgb) {
                px[0..3].copy_from_slice(to);
            }
        }
    }
    Some(images.add(image))
}

/// Root for direct file reads, mirroring Bevy's own resolution: an explicit
/// `BEVY_ASSET_ROOT` wins; otherwise the executable's directory when it has an
/// `assets/` folder next to it (a shipped bundle, launched from anywhere);
/// otherwise the working directory (a `cargo run` from the project root).
pub fn asset_root() -> String {
    if let Ok(root) = std::env::var("BEVY_ASSET_ROOT") {
        return root;
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
        && dir.join("assets").is_dir()
        && let Some(dir) = dir.to_str()
    {
        return dir.to_string();
    }
    ".".to_string()
}
