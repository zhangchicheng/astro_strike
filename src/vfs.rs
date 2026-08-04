//! Unified access to the files the game reads DIRECTLY rather than through
//! Bevy's asset server: the palette, the PNGs it recolors and measures, the
//! projectile animation JSON, and the Tiled maps.
//!
//! Natively these come from disk under [`crate::recolor::asset_root`]. On the
//! web there is no filesystem, so the same files are embedded into the binary
//! at compile time (`build.rs` generates the table). Audio, fonts, and images
//! drawn as-is keep going through the asset server, which fetches over HTTP on
//! the web by itself.

/// File contents for a path relative to `assets/` (forward slashes).
#[cfg(not(target_arch = "wasm32"))]
pub fn read_bytes(rel: &str) -> Option<Vec<u8>> {
    std::fs::read(format!("{}/assets/{rel}", crate::recolor::asset_root())).ok()
}

/// File contents for a path relative to `assets/` (forward slashes).
#[cfg(target_arch = "wasm32")]
pub fn read_bytes(rel: &str) -> Option<Vec<u8>> {
    embedded::EMBEDDED
        .iter()
        .find(|(path, _)| *path == rel)
        .map(|(_, bytes)| bytes.to_vec())
}

/// UTF-8 file contents for a path relative to `assets/`.
pub fn read_string(rel: &str) -> Option<String> {
    String::from_utf8(read_bytes(rel)?).ok()
}

#[cfg(target_arch = "wasm32")]
mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded_assets.rs"));
}
