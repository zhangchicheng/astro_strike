//! Loads and renders the Tiled (`.tmx`) level.
//!
//! The map is parsed with the engine-independent `tiled` crate (which resolves
//! the external `.tsx` tilesets and decodes the CSV tile layers). Rendering is
//! split by layer kind:
//!
//!   * **Tile layers** (the terrain) are rendered with `bevy_ecs_tilemap`, which
//!     draws them via an array-texture shader. This avoids the thin seams that
//!     appear between separate per-tile sprites when the camera scrolls.
//!   * **Object layers** (space bodies, buildings, units) are individual
//!     `Sprite`s; these don't tile edge-to-edge so they have no seam problem, and
//!     staying sprites keeps per-object animation (e.g. `unit_4`) simple.
//!
//! Every layer (tilemap or sprite parent) carries a [`ParallaxLayer`] so the
//! parallax system can scroll it; its factor is read from the TMX.

use bevy::prelude::*;
use bevy_ecs_tilemap::prelude::{
    ArrayTextureLoader, TileBundle, TileFlip, TilePos, TileStorage, TileTextureIndex,
    TilemapAnchor, TilemapArrayTexture, TilemapBundle, TilemapId, TilemapSize, TilemapTexture,
    TilemapTileSize, TilemapType,
};
use std::collections::HashMap;
use tiled::{LayerType, Loader, Map, ObjectShape, PropertyValue, TileLayer, Tileset};

use rand::Rng;

use crate::animation::SpriteSheetAnimation;
use crate::common::{
    CHUNK_PX, Collider, LEVEL_ROWS, LEVELS, MAP_COLS, MAP_HALF, MAP_HEIGHT, MAP_WIDTH,
    ORIGINAL_ROWS, SPACE_ROWS,
};
use crate::destructible::{DESTRUCTIBLE_HP, Destructible};
use crate::parallax::ParallaxLayer;
use crate::vehicle::{Convoy, DEFAULT_PATROL_HALF_RANGE, Patrol, RoadBlocks, TankGun};

/// Z of the backmost layer; each subsequent layer is drawn one step in front.
const MAP_Z_BASE: f32 = -20.0;
const MAP_Z_STEP: f32 = 1.0;

/// Number of `space_N.tmx` segments available to pick from randomly.
const SPACE_MAP_COUNT: u32 = 8;
/// Chunk maps that make up one level's terrain (chunk_1..10 = level 1, etc.).
const CHUNKS_PER_LEVEL: u32 = ORIGINAL_ROWS * MAP_COLS;

/// Serves the Tiled loader from [`crate::vfs`] (disk natively, embedded bytes
/// on the web). The crate hands us joined paths like
/// `assets/maps/chunks/../tilesets/terrain.tsx`; normalize the `..` segments
/// and strip everything up to `assets/` to get the vfs key.
struct VfsReader;

impl tiled::ResourceReader for VfsReader {
    type Resource = std::io::Cursor<Vec<u8>>;
    type Error = std::io::Error;

    fn read_from(&mut self, path: &std::path::Path) -> Result<Self::Resource, Self::Error> {
        let not_found =
            || std::io::Error::new(std::io::ErrorKind::NotFound, path.display().to_string());
        let mut parts: Vec<&str> = Vec::new();
        for component in path.components() {
            match component {
                std::path::Component::Normal(part) => {
                    parts.push(part.to_str().ok_or_else(not_found)?);
                }
                std::path::Component::ParentDir => {
                    parts.pop().ok_or_else(not_found)?;
                }
                _ => {}
            }
        }
        let start = parts.iter().rposition(|p| *p == "assets").map(|i| i + 1);
        let rel = parts[start.ok_or_else(not_found)?..].join("/");
        crate::vfs::read_bytes(&rel)
            .map(std::io::Cursor::new)
            .ok_or_else(not_found)
    }
}

/// Marker for object-layer sprites that belong to the map.
#[derive(Component)]
pub struct MapTile;

pub(super) fn plugin(app: &mut App) {
    // Built once — the whole continuous multi-level map (see `common.rs`).
    app.add_systems(Startup, spawn_map);
}

fn spawn_map(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    array_texture_loader: Res<ArrayTextureLoader>,
) {
    let mut loader = Loader::with_reader(VfsReader);
    let half = MAP_HALF;

    // Tileset images and array-texture preloads are shared across all chunks.
    // Loaded in their authored (default-palette) colors: the per-level color
    // schemes are applied to the finished frame by `palette_grade`, not here.
    let mut images: HashMap<String, Handle<Image>> = HashMap::new();
    let mut array_added: std::collections::HashSet<String> = std::collections::HashSet::new();

    let mut tile_count = 0usize;
    let mut animated_count = 0usize;
    let mut segments_loaded = 0usize;
    // World positions of road blocks (tiles tagged `Block=true`), stopping points
    // for tanks/trucks — collected across all segments, published as a resource.
    let mut blocks: Vec<Vec2> = Vec::new();

    // Build the whole continuous map as (path, col, row). Rows are top-down (row 0
    // = top). The map is the levels concatenated: from the top, the highest level's
    // chunks, then its space corridor, then the next level's chunks/corridor, ...,
    // down to level 1's chunks and space intro at the very bottom.
    let mut rng = rand::thread_rng();
    let mut segments: Vec<(String, u32, u32)> = Vec::new();
    for l in 0..LEVELS {
        // Top-down row where this level's band starts (highest level is on top).
        let band_top_row = (LEVELS - 1 - l) * LEVEL_ROWS;
        // Chunk terrain (chunk_1..10 for level 1, 11..20 for level 2, ...).
        for cr in 0..ORIGINAL_ROWS {
            for col in 0..MAP_COLS {
                let n = l * CHUNKS_PER_LEVEL + cr * MAP_COLS + col + 1;
                segments.push((format!("assets/maps/chunks/chunk_{n}.tmx"), col, band_top_row + cr));
            }
        }
        // Space corridor below this level's chunks: random space segments, but
        // never the same unit orthogonally adjacent to itself — side-by-side
        // repeats read as obvious tiling. (Corridors of different bands never
        // touch, so per-band bookkeeping is enough; 0 is an unused sentinel.)
        let mut above: Vec<u32> = vec![0; MAP_COLS as usize];
        for sr in 0..SPACE_ROWS {
            let mut left = 0u32;
            for col in 0..MAP_COLS {
                let n = loop {
                    let n = rng.gen_range(1..=SPACE_MAP_COUNT);
                    if n != left && n != above[col as usize] {
                        break n;
                    }
                };
                left = n;
                above[col as usize] = n;
                segments.push((
                    format!("assets/maps/space/space_{n}.tmx"),
                    col,
                    band_top_row + ORIGINAL_ROWS + sr,
                ));
            }
        }
    }
    let segment_count = segments.len();

    for (path, col, row) in &segments {
        // Top-left of this segment in full-map pixels (y-down from the map's top-left).
        let chunk_offset = Vec2::new(*col as f32 * CHUNK_PX, *row as f32 * CHUNK_PX);

        let map = match loader.load_tmx_map(path) {
            Ok(map) => map,
            Err(err) => {
                error!("failed to load segment `{path}`: {err}");
                continue;
            }
        };
        segments_loaded += 1;

        for tileset in map.tilesets() {
            if let Some(name) = image_name(tileset)
                && !images.contains_key(&name)
            {
                let handle = asset_server.load(name.clone());
                images.insert(name, handle);
            }
        }

        // A Tiled position within this chunk (top-left, pixels, y-down) + size ->
        // Bevy world-space center (origin at the full map's center, y-up).
        let to_world = |chunk_top_left: Vec2, size: Vec2| {
            let center = chunk_offset + chunk_top_left + size / 2.0;
            Vec2::new(center.x - half.x, half.y - center.y)
        };

        for (layer_index, layer) in map.layers().enumerate() {
            let z = MAP_Z_BASE + layer_index as f32 * MAP_Z_STEP;
            let factor =
                parallax_factor(&layer.name, Vec2::new(layer.parallax_x, layer.parallax_y));

            match layer.layer_type() {
                LayerType::Tiles(tile_layer) => {
                    // World center of this chunk; bevy_ecs_tilemap anchors there.
                    let center = to_world(Vec2::splat(CHUNK_PX / 2.0), Vec2::ZERO);
                    tile_count += spawn_tile_layer(
                        &mut commands,
                        &images,
                        &array_texture_loader,
                        &mut array_added,
                        &map,
                        &format!("{}@{col},{row}", layer.name),
                        &tile_layer,
                        z,
                        factor,
                        center,
                    );
                }
                LayerType::Objects(object_layer) => {
                    // Parent entity for the layer; parallax moves this, children follow.
                    // Children hold full-map world positions, so its base is the origin.
                    let parent = commands
                        .spawn((
                            Transform::from_xyz(0.0, 0.0, z),
                            Visibility::default(),
                            ParallaxLayer {
                                factor,
                                base: Vec2::ZERO,
                            },
                            Name::new(format!("Layer:{}@{col},{row}", layer.name)),
                        ))
                        .id();

                    // Trucks are assembled from several adjacent tiles; collect them
                    // and spawn each as one movable group after the per-tile pass.
                    let mut truck_tiles: Vec<TruckTile> = Vec::new();

                    for (i, object) in object_layer.objects().enumerate() {
                        let Some(object_tile) = object.get_tile() else {
                            continue;
                        };
                        let tileset = object_tile.get_tileset();

                        // Prefer the object's own footprint (supports scaled objects);
                        // fall back to the tileset's native tile size.
                        let size = match object.shape {
                            ObjectShape::Rect { width, height } => Vec2::new(width, height),
                            _ => Vec2::new(tileset.tile_width as f32, tileset.tile_height as f32),
                        };

                        // For tile-objects, (x, y) is the bottom-left corner (y-down).
                        let top_left = Vec2::new(object.x, object.y - size.y);
                        let pos = to_world(top_left, size);
                        // Nudge z so overlapping objects keep their file order.
                        let local_z = i as f32 * 0.001;

                        // Road blocks bound vehicle movement: record the position
                        // (the tile still renders normally as part of the road).
                        if matches!(
                            object.properties.get("Block"),
                            Some(PropertyValue::BoolValue(true))
                        ) {
                            blocks.push(pos);
                        }

                        // Defer truck parts to be assembled into moving groups.
                        if is_truck(&tileset.name)
                            && let Some(image) = images.get(&image_name(tileset).unwrap_or_default())
                        {
                            truck_tiles.push(TruckTile {
                                name: tileset.name.clone(),
                                image: image.clone(),
                                rect: tile_rect(tileset, object_tile.id()),
                                size,
                                pos,
                                z: local_z,
                            });
                            continue;
                        }

                        let Some(entity) = spawn_object_sprite(
                            &mut commands,
                            &images,
                            parent,
                            tileset,
                            object_tile.id(),
                            size,
                            pos,
                            local_z,
                            &mut animated_count,
                        ) else {
                            continue;
                        };
                        tile_count += 1;

                        // Destructible props: give them a collider + the sequence of
                        // damage frames shown as they take hits.
                        let destructible = matches!(
                            object.properties.get("Destructible"),
                            Some(PropertyValue::BoolValue(true))
                        );
                        if destructible {
                            commands.entity(entity).insert((
                                Collider(size),
                                Destructible {
                                    stages: destruction_stages(tileset, object_tile.id()),
                                    taken: 0,
                                    intact: tile_rect(tileset, object_tile.id()),
                                    collider: size,
                                },
                            ));
                        }

                        // Tanks patrol back and forth along the road. The patrol
                        // half-range comes from an optional `Range` object property
                        // (in tiles) so it can be sized to the road; default otherwise.
                        if tileset.name == "tank" {
                            let half_range = float_property(&object.properties, "Range")
                                .map(|tiles| tiles * tileset.tile_width as f32)
                                .unwrap_or(DEFAULT_PATROL_HALF_RANGE);
                            commands.entity(entity).insert((
                                Patrol::new(pos.x, half_range, 1.0),
                                // Random cooldown phase so tanks don't fire in unison.
                                TankGun::new(rng.gen_range(0.0..1.0)),
                            ));
                        }
                    }

                    // Assemble collected truck parts into single patrolling units.
                    tile_count += spawn_trucks(&mut commands, parent, truck_tiles);
                }
                // Image / group layers are not used by this map.
                _ => {}
            }
        }
    }

    let block_count = blocks.len();
    commands.insert_resource(RoadBlocks(blocks));

    info!(
        "stitched {segments_loaded}/{segment_count} segments ({}x{} px), {} tiles/objects ({} animated), {} road blocks",
        MAP_WIDTH as u32, MAP_HEIGHT as u32, tile_count, animated_count, block_count,
    );
}

/// Renders a Tiled tile layer as a `bevy_ecs_tilemap` tilemap. Returns the number
/// of tiles placed. The tilemap entity carries the [`ParallaxLayer`] so it scrolls.
#[allow(clippy::too_many_arguments)]
fn spawn_tile_layer(
    commands: &mut Commands,
    images: &HashMap<String, Handle<Image>>,
    array_texture_loader: &ArrayTextureLoader,
    array_added: &mut std::collections::HashSet<String>,
    map: &Map,
    layer_name: &str,
    tile_layer: &TileLayer,
    z: f32,
    factor: Vec2,
    center: Vec2,
) -> usize {
    let size = TilemapSize {
        x: map.width,
        y: map.height,
    };
    let tilemap_entity = commands.spawn_empty().id();
    let mut storage = TileStorage::empty(size);
    let mut tileset_ref: Option<&Tileset> = None;
    let mut count = 0usize;

    for y in 0..map.height {
        for x in 0..map.width {
            // Tiled's rows go top-down; bevy_ecs_tilemap is y-up, so flip.
            let mapped_y = map.height - 1 - y;
            let Some(tile) = tile_layer.get_tile(x as i32, mapped_y as i32) else {
                continue;
            };
            tileset_ref.get_or_insert_with(|| tile.get_tileset());

            let tile_pos = TilePos { x, y };
            let tile_entity = commands
                .spawn(TileBundle {
                    position: tile_pos,
                    tilemap_id: TilemapId(tilemap_entity),
                    texture_index: TileTextureIndex(tile.id()),
                    flip: TileFlip {
                        x: tile.flip_h,
                        y: tile.flip_v,
                        d: tile.flip_d,
                    },
                    ..default()
                })
                .id();
            storage.set(&tile_pos, tile_entity);
            count += 1;
        }
    }

    // Resolve the layer's tileset image (all cells in these layers share one).
    let handle = tileset_ref
        .and_then(image_name)
        .and_then(|name| images.get(&name).cloned());
    let (Some(tileset), Some(handle)) = (tileset_ref, handle) else {
        commands.entity(tilemap_entity).despawn();
        return 0;
    };

    let tile_size = TilemapTileSize {
        x: tileset.tile_width as f32,
        y: tileset.tile_height as f32,
    };

    // Preprocess the tileset into an array texture (once per image). Array textures
    // sample each tile from its own layer, so neighbours never bleed into one another.
    if let Some(name) = image_name(tileset)
        && array_added.insert(name)
    {
        array_texture_loader.add(TilemapArrayTexture {
            texture: TilemapTexture::Single(handle.clone()),
            tile_size,
            ..default()
        });
    }

    commands.entity(tilemap_entity).insert((
        TilemapBundle {
            grid_size: tile_size.into(),
            map_type: TilemapType::Square,
            size,
            storage,
            texture: TilemapTexture::Single(handle.clone()),
            tile_size,
            anchor: TilemapAnchor::Center,
            transform: Transform::from_xyz(center.x, center.y, z),
            ..default()
        },
        ParallaxLayer {
            factor,
            base: center,
        },
        Name::new(format!("TileLayer:{layer_name}")),
    ));

    count
}

/// Spawns one object-layer tile as a sprite child of `parent`. Returns the spawned
/// entity, or `None` if the tile's image could not be resolved.
#[allow(clippy::too_many_arguments)]
fn spawn_object_sprite(
    commands: &mut Commands,
    images: &HashMap<String, Handle<Image>>,
    parent: Entity,
    tileset: &Tileset,
    local_id: u32,
    size: Vec2,
    pos: Vec2,
    local_z: f32,
    animated_count: &mut usize,
) -> Option<Entity> {
    let image = images.get(&image_name(tileset)?)?.clone();
    let mut sprite = build_sprite(image, tileset, local_id, size);

    let animation = build_animation(tileset, local_id);
    if let Some(anim) = &animation {
        // Start on the first animation frame.
        if let Some(rect) = anim.first_rect() {
            sprite.rect = Some(rect);
        }
        *animated_count += 1;
    }

    let mut entity = commands.spawn((
        sprite,
        Transform::from_xyz(pos.x, pos.y, local_z),
        MapTile,
        ChildOf(parent),
    ));
    if let Some(anim) = animation {
        entity.insert(anim);
    }
    Some(entity.id())
}

/// True for tileset names whose objects assemble into a multi-part moving truck.
fn is_truck(tileset_name: &str) -> bool {
    tileset_name == "truck_empty" || tileset_name == "truck_loaded"
}

/// Reads a numeric TMX object property (float or int) as `f32`, if present.
fn float_property(props: &tiled::Properties, name: &str) -> Option<f32> {
    match props.get(name) {
        Some(PropertyValue::FloatValue(v)) => Some(*v),
        Some(PropertyValue::IntValue(v)) => Some(*v as f32),
        _ => None,
    }
}

/// One collected truck tile, pending assembly into a group in `spawn_trucks`.
struct TruckTile {
    /// Tileset name — trucks of different kinds (empty/loaded) never merge.
    name: String,
    image: Handle<Image>,
    rect: Rect,
    size: Vec2,
    /// World-space center of the tile.
    pos: Vec2,
    /// File-order z nudge.
    z: f32,
}

/// Assembles collected truck tiles into groups (a run of adjacent same-kind tiles
/// on one row = one truck) and spawns each as a single [`Convoy`] parent whose
/// part-sprites are children, so the whole truck moves as one unit. Returns the
/// number of tiles placed.
fn spawn_trucks(commands: &mut Commands, parent: Entity, mut tiles: Vec<TruckTile>) -> usize {
    // Order by kind, then row (y), then position (x) so one truck's tiles are contiguous.
    tiles.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then(a.pos.y.total_cmp(&b.pos.y))
            .then(a.pos.x.total_cmp(&b.pos.x))
    });

    let mut count = 0;
    let mut i = 0;
    while i < tiles.len() {
        // Extend the group while the next tile is the same kind, same row, and
        // horizontally adjacent (a larger x gap means a separate truck).
        let mut j = i + 1;
        while j < tiles.len()
            && tiles[j].name == tiles[i].name
            && (tiles[j].pos.y - tiles[j - 1].pos.y).abs() < 1.0
            && (tiles[j].pos.x - tiles[j - 1].pos.x).abs() <= tiles[j].size.x + 1.0
        {
            j += 1;
        }
        let group = &tiles[i..j];
        count += group.len();

        // The parent sits at the group's center; parts are offset from it.
        let min_x = group.iter().map(|t| t.pos.x).fold(f32::INFINITY, f32::min);
        let max_x = group.iter().map(|t| t.pos.x).fold(f32::NEG_INFINITY, f32::max);
        let center = Vec2::new((min_x + max_x) / 2.0, group[0].pos.y);
        // Half the truck's full span: the end tiles' centers plus half a tile each.
        let half_width = (max_x - min_x) / 2.0 + group[0].size.x / 2.0;

        commands
            .spawn((
                Transform::from_xyz(center.x, center.y, group[0].z),
                Visibility::default(),
                Convoy::new(center.x, half_width),
                Name::new(format!("Truck:{}", group[0].name)),
                ChildOf(parent),
            ))
            .with_children(|truck| {
                for t in group {
                    truck.spawn((
                        Sprite {
                            image: t.image.clone(),
                            rect: Some(t.rect),
                            custom_size: Some(t.size),
                            ..default()
                        },
                        Transform::from_xyz(t.pos.x - center.x, 0.0, 0.0),
                        MapTile,
                    ));
                }
            });
        i = j;
    }
    count
}

/// Builds a textured sprite that samples `local_id` out of `tileset`.
fn build_sprite(image: Handle<Image>, tileset: &Tileset, local_id: u32, size: Vec2) -> Sprite {
    Sprite {
        image,
        rect: Some(tile_rect(tileset, local_id)),
        custom_size: Some(size),
        ..default()
    }
}

/// The per-hit sprite rects a destructible object cycles through, ending on its
/// destroyed look. Its length is the object's hit points.
///
/// Most props stay intact until the final hit, so their early stages just repeat
/// the intact tile and the last is the destroyed tile. `unit_2` is special: it has
/// genuine intermediate frames (frame 0 intact, frames 1..=3 progressive damage),
/// so it visibly crumbles one stage per hit.
fn destruction_stages(tileset: &Tileset, local_id: u32) -> Vec<Rect> {
    if tileset.name == "unit_2" {
        // Progressive: show damage frames 1, 2, 3 on successive hits.
        return (1..=DESTRUCTIBLE_HP)
            .map(|i| tile_rect(tileset, local_id + i))
            .collect();
    }
    // Standard: no visual change until the final hit swaps in the destroyed tile.
    let intact = tile_rect(tileset, local_id);
    let mut stages = vec![intact; (DESTRUCTIBLE_HP - 1) as usize];
    stages.push(tile_rect(tileset, destroyed_tile_id(tileset, local_id)));
    stages
}

/// The local tile id of a destructible tile's *destroyed* state.
///
/// Most destructible tilesets are two columns — intact on the left, destroyed on
/// the right — so the destroyed tile is the next column (`+1`). `building_3` is
/// different: it's a 3x3 building whose tileset packs the intact 3x3 block on the
/// left and the destroyed 3x3 block on the right (6 columns total). Each of its
/// tiles therefore maps to the destroyed tile half the columns to the right, in
/// the same row (e.g. local 0 -> 3, local 7 -> 10, local 14 -> 17).
fn destroyed_tile_id(tileset: &Tileset, local_id: u32) -> u32 {
    match tileset.name.as_str() {
        "building_3" => local_id + tileset.columns / 2,
        _ => local_id + 1,
    }
}

/// Source rectangle (in image pixels) for a tile's local id within its tileset.
fn tile_rect(tileset: &Tileset, local_id: u32) -> Rect {
    let cols = tileset.columns.max(1);
    let (tw, th) = (tileset.tile_width as f32, tileset.tile_height as f32);
    let min = Vec2::new((local_id % cols) as f32 * tw, (local_id / cols) as f32 * th);
    Rect {
        min,
        max: min + Vec2::new(tw, th),
    }
}

/// If the tileset tile defines a TMX animation, build a looping sprite-sheet
/// animation from its frames.
fn build_animation(tileset: &Tileset, local_id: u32) -> Option<SpriteSheetAnimation> {
    let tile = tileset.get_tile(local_id)?;
    let frames = tile.animation.as_ref()?;
    let anim_frames = frames
        .iter()
        .map(|frame| {
            (
                tile_rect(tileset, frame.tile_id),
                frame.duration as f32 / 1000.0,
            )
        })
        .collect();
    Some(SpriteSheetAnimation::new(anim_frames, true, false))
}

/// The asset path (e.g. `maps/tilesets/terrain.png`) of a tileset's image. The
/// TSX stores a path relative to itself; tileset images live next to their TSX
/// under `assets/maps/tilesets/`, so the asset-server path is that prefix plus
/// the bare file name.
fn image_name(tileset: &Tileset) -> Option<String> {
    let name = tileset.image.as_ref()?.source.file_name()?.to_str()?;
    Some(format!("maps/tilesets/{name}"))
}

/// Resolves a layer's parallax factor. Honors an explicit (non-default) value
/// from the TMX; otherwise falls back to a sensible demo default by layer name
/// so the effect is visible even before `parallaxx`/`parallaxy` are set in Tiled.
fn parallax_factor(layer_name: &str, tmx: Vec2) -> Vec2 {
    if tmx != Vec2::ONE {
        return tmx;
    }
    match layer_name {
        // Distant celestial bodies drift slowly relative to the ship.
        "Space" => Vec2::splat(0.3),
        // Terrain and buildings sit in the world and move at full relative speed.
        _ => Vec2::ONE,
    }
}
