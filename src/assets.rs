//! Loaders for the extracted asset directory. See ASSET_FORMATS.md.
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Json(serde_json::Error),
    Png(png::DecodingError),
    Wav(hound::Error),
    Msg(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Msg(m) => write!(f, "{}", m),
            other => write!(f, "{:?}", other),
        }
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error { fn from(e: std::io::Error) -> Self { Error::Io(e) } }
impl From<serde_json::Error> for Error { fn from(e: serde_json::Error) -> Self { Error::Json(e) } }
impl From<png::DecodingError> for Error { fn from(e: png::DecodingError) -> Self { Error::Png(e) } }
impl From<hound::Error> for Error { fn from(e: hound::Error) -> Self { Error::Wav(e) } }
pub type Result<T> = std::result::Result<T, Error>;

/// Where the executables look for the extracted assets when the command line names no folder: `assets` next to the executable, otherwise `assets` in the
/// current directory (where `cargo run` from the project finds them, the executable being under `target/`). When neither exists the first one is returned,
/// so that the error message names the place that was expected.
pub fn default_assets_dir() -> std::path::PathBuf {
    let beside = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("assets")));
    if let Some(p) = &beside {
        if p.is_dir() { return p.clone(); }
    }
    let here = std::path::PathBuf::from("assets");
    if here.is_dir() { return here; }
    beside.unwrap_or(here)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let f = File::open(path)?;
    Ok(serde_json::from_reader(BufReader::new(f))?)
}

// ---------------------------------------------------------------- PNG ----------------------------
pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub color: png::ColorType,
    pub data: Vec<u8>,
}

/// Decode a PNG without any colour expansion (indexed PNGs keep their palette indices).
pub fn read_png(path: &Path) -> Result<Decoded> {
    let mut decoder = png::Decoder::new(File::open(path)?);
    decoder.set_transformations(png::Transformations::IDENTITY);
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf)?;
    buf.truncate(info.buffer_size());
    Ok(Decoded { width: info.width, height: info.height, color: info.color_type, data: buf })
}

// ---------------------------------------------------------------- palette ------------------------
#[derive(Deserialize)]
struct PaletteFile {
    name: String,
    colors: Vec<[u8; 3]>,
}

/// The 256-colour Mac system palette used by all tiles and sprites.
pub struct Palette {
    pub colors: [[u8; 3]; 256],
}

impl Palette {
    pub fn load_system(root: &Path) -> Result<Palette> {
        for entry in std::fs::read_dir(root.join("palettes"))? {
            let p = entry?.path();
            if p.extension().and_then(|e| e.to_str()) != Some("json") { continue; }
            let pf: PaletteFile = read_json(&p)?;
            if pf.name == "System colours" {
                if pf.colors.len() != 256 { return Err(Error::Msg("system palette is not 256 entries".into())); }
                let mut colors = [[0u8; 3]; 256];
                for (i, c) in pf.colors.iter().enumerate() { colors[i] = *c; }
                return Ok(Palette { colors });
            }
        }
        Err(Error::Msg("system palette (\"System colours\") not found in palettes/".into()))
    }
    pub fn argb(&self, index: u8) -> u32 {
        let c = self.colors[index as usize];
        0xFF00_0000 | ((c[0] as u32) << 16) | ((c[1] as u32) << 8) | (c[2] as u32)
    }
    /// Colour -> index. All 256 system colours are distinct, so this is lossless.
    pub fn reverse_map(&self) -> HashMap<[u8; 3], u8> {
        let mut m = HashMap::new();
        for (i, c) in self.colors.iter().enumerate() { m.insert(*c, i as u8); }
        m
    }
}

/// The colour grid of the debris effects (PICT 960, 13 x 10 pixels: ramp = column, step = row) as palette indices, `ramp * 10 + step` (0x29668: `GetCPixel`,
/// `Color2Index`). Colours that are not in the system palette take the nearest one.
pub fn load_particle_colours(root: &Path, palette: &Palette) -> Result<Vec<u8>> {
    let img = read_png(&root.join("pictures/app/960_particle_colour_grid.png"))?;
    if img.width != 13 || img.height != 10 { return Err(Error::Msg("particle colour grid is not 13 x 10".into())); }
    let bpp = match img.color { png::ColorType::Rgba => 4, png::ColorType::Rgb => 3, _ => return Err(Error::Msg("particle colour grid must be RGB(A)".into())) };
    let rev = palette.reverse_map();
    let mut out = Vec::with_capacity(130);
    for ramp in 0..13usize {
        for step in 0..10usize {
            let o = (step * 13 + ramp) * bpp;
            let px = [img.data[o], img.data[o + 1], img.data[o + 2]];
            let idx = rev.get(&px).copied().unwrap_or_else(|| {
                (0..256usize).min_by_key(|&i| palette.colors[i].iter().zip(px.iter()).map(|(a, b)| (*a as i32 - *b as i32).pow(2)).sum::<i32>()).unwrap_or(0) as u8
            });
            out.push(idx);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- tiles --------------------------
#[derive(Deserialize)]
struct TilesMeta {
    tile_size: usize,
    count: usize,
    columns: usize,
}

/// 32x32 8-bit palette-indexed tiles. `tiles[i]` is 1024 bytes, row-major. `mirrored[i]` is the same tile flipped
/// horizontally (the original's separate TILF sheet), used for map cells with the "mirror" flag.
pub struct TileSet {
    pub count: usize,
    pub tiles: Vec<Vec<u8>>,
    pub mirrored: Vec<Vec<u8>>,
}

fn slice_sheet(path: &Path, meta: &TilesMeta) -> Result<Vec<Vec<u8>>> {
    let img = read_png(path)?;
    if img.color != png::ColorType::Indexed { return Err(Error::Msg("tile sheet must be an indexed PNG".into())); }
    let w = img.width as usize;
    let mut tiles = Vec::with_capacity(meta.count);
    for i in 0..meta.count {
        let (tx, ty) = ((i % meta.columns) * 32, (i / meta.columns) * 32);
        let mut t = vec![0u8; 1024];
        for y in 0..32 {
            let src = (ty + y) * w + tx;
            t[y * 32..y * 32 + 32].copy_from_slice(&img.data[src..src + 32]);
        }
        tiles.push(t);
    }
    Ok(tiles)
}

impl TileSet {
    pub fn load(root: &Path) -> Result<TileSet> {
        let meta: TilesMeta = read_json(&root.join("tiles/tiles.json"))?;
        if meta.tile_size != 32 { return Err(Error::Msg("unexpected tile size".into())); }
        let tiles = slice_sheet(&root.join("tiles/tiles.png"), &meta)?;
        let mirrored = slice_sheet(&root.join("tiles/tiles_mirrored.png"), &meta)?;
        Ok(TileSet { count: meta.count, tiles, mirrored })
    }
}

// ---------------------------------------------------------------- sprites ------------------------
/// A sprite as palette indices plus an opacity mask (index 0 is white, so transparency can't live in the indices).
pub struct Sprite {
    pub width: usize,
    pub height: usize,
    pub index: Vec<u8>,
    pub opaque: Vec<bool>,
}

pub fn load_sprite(root: &Path, id: u32, rev: &HashMap<[u8; 3], u8>) -> Result<Sprite> {
    let img = read_png(&root.join(format!("sprites/{:05}.png", id)))?;
    if img.color != png::ColorType::Rgba { return Err(Error::Msg(format!("sprite {} is not RGBA", id))); }
    let n = (img.width * img.height) as usize;
    let (mut index, mut opaque) = (vec![0u8; n], vec![false; n]);
    for i in 0..n {
        let p = &img.data[i * 4..i * 4 + 4];
        if p[3] != 0 {
            index[i] = *rev.get(&[p[0], p[1], p[2]]).ok_or_else(|| Error::Msg(format!("sprite {}: colour not in system palette", id)))?;
            opaque[i] = true;
        }
    }
    Ok(Sprite { width: img.width as usize, height: img.height as usize, index, opaque })
}

// ---------------------------------------------------------------- animation table ----------------
#[derive(Deserialize)]
struct SetEntry {
    first_id: u32,
    count: u32,
}
#[derive(Deserialize)]
struct SetInfo {
    id: u32,
    entries: Vec<SetEntry>,
}
#[derive(Deserialize)]
struct SetsFile {
    sets: Vec<SetInfo>,
}

/// The sprite animation table (`SpIc` set 1000, `sprites/sets.json`): the draw routines name a sprite as (animation, frame) with both numbers
/// 1-based; the sprite id is `entries[animation - 1].first_id + frame - 1` (verified by the draw routines 0x2f88 ... 0x3540 and the 0x3e70 lookup).
pub struct AnimTable {
    entries: Vec<(u32, u32)>,
}

impl AnimTable {
    pub fn load(root: &Path) -> Result<AnimTable> {
        let f: SetsFile = read_json(&root.join("sprites/sets.json"))?;
        let set = f.sets.into_iter().find(|s| s.id == 1000).ok_or_else(|| Error::Msg("sprites/sets.json has no set 1000".to_string()))?;
        Ok(AnimTable { entries: set.entries.into_iter().map(|e| (e.first_id, e.count)).collect() })
    }
    pub fn from_entries(entries: Vec<(u32, u32)>) -> AnimTable { AnimTable { entries } }
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
    /// Sprite id of (`anim`, `frame`); `None` where the original draws nothing (frame <= 0) or the request is outside the table.
    pub fn sprite_id(&self, anim: i32, frame: i32) -> Option<u32> {
        if anim < 1 || frame < 1 { return None; }
        let (first, count) = *self.entries.get(anim as usize - 1)?;
        if frame as u32 > count { return None; }
        Some(first + frame as u32 - 1)
    }
}

/// Lazily loads and caches sprites by id.
pub struct SpriteBank {
    root: std::path::PathBuf,
    rev: HashMap<[u8; 3], u8>,
    cache: HashMap<u32, Sprite>,
    /// Sprites that had to be read from disk while the game was running (after `preload_all` this should stay 0: each one is a frame hitch).
    pub late_loads: u32,
}

impl SpriteBank {
    pub fn new(root: &Path, palette: &Palette) -> SpriteBank {
        SpriteBank { root: root.to_path_buf(), rev: palette.reverse_map(), cache: HashMap::new(), late_loads: 0 }
    }
    pub fn get(&mut self, id: u32) -> Result<&Sprite> {
        if !self.cache.contains_key(&id) {
            let s = load_sprite(&self.root, id, &self.rev)?;
            self.cache.insert(id, s);
            self.late_loads += 1;
        }
        Ok(&self.cache[&id])
    }

    /// Decode every sprite of `sprites/` now, on all cores, so that no PNG is read in the middle of a frame (decoding a sprite for the first
    /// time used to happen the first time anything showed it: a disk read + PNG decode + a palette lookup per pixel, many milliseconds for
    /// a large one, i.e. a visible hitch every time something new appeared). Sprites that fail to load are left out: using one reports the
    /// error as before. Returns the number of sprites now in memory.
    pub fn preload_all(&mut self) -> Result<usize> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Mutex;
        let mut ids: Vec<u32> = Vec::new();
        for e in std::fs::read_dir(self.root.join("sprites"))? {
            let p = e?.path();
            if p.extension().map_or(false, |x| x == "png") {
                if let Some(id) = p.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse::<u32>().ok()) {
                    if !self.cache.contains_key(&id) { ids.push(id); }
                }
            }
        }
        ids.sort_unstable();
        let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).clamp(1, 8);
        let next = AtomicUsize::new(0);
        let done: Mutex<Vec<(u32, Sprite)>> = Mutex::new(Vec::with_capacity(ids.len()));
        let (root, rev, ids_ref) = (&self.root, &self.rev, &ids);
        std::thread::scope(|sc| {
            for _ in 0..workers {
                sc.spawn(|| loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= ids_ref.len() { break; }
                    if let Ok(s) = load_sprite(root, ids_ref[i], rev) {
                        done.lock().unwrap_or_else(|e| e.into_inner()).push((ids_ref[i], s));
                    }
                });
            }
        });
        for (id, s) in done.into_inner().unwrap_or_else(|e| e.into_inner()) { self.cache.insert(id, s); }
        Ok(self.cache.len())
    }
}

// ---------------------------------------------------------------- levels -------------------------
#[derive(Deserialize, Debug, Clone)]
pub struct Group {
    pub index: u32,
    #[serde(rename = "u32")]
    pub fields: Vec<u32>, // 31 big-endian u32 fields of the original 124-byte record; semantics unverified
}

#[derive(Deserialize, Debug, Clone)]
pub struct GroundSprite {
    pub index: u32,
    pub flags: Vec<u8>,
    /// Object kind 0..=24 (selects behaviour and animation).
    #[serde(default)]
    pub kind: i32,
    #[serde(default)]
    pub param: i32,
    /// 1..=8 direction/variant for kinds 1 and 24; copied to a state field by kinds 16..=21.
    #[serde(default)]
    pub variant: i32,
    #[serde(default)]
    pub prev_link: i32, // index of the paired record before this one, -1 = none
    #[serde(default)]
    pub next_link: i32,
    /// 1-based entry in the sprite animation table (sprites/sets.json, set 1000). Verified on screen for kinds 4 and 8.
    #[serde(default)]
    pub anim_index: u32,
    /// Record byte 1: the object also draws a drop shadow (see render::SHADOW_OFFSET).
    #[serde(default)]
    pub casts_shadow: bool,
    /// Sprite id of the initial frame (0 = none) and its 0-based frame number within the strip.
    #[serde(default)]
    pub sprite_id: u32,
    #[serde(default)]
    pub initial_frame: u32,
    /// Add to (x, row-top) to get the sprite's top-left; centres the sprite in a 32x32 / 64x64 box.
    #[serde(default)]
    pub draw_offset: [i32; 2],
    pub i32_a: Vec<i32>,
    pub x: f32, // pixel column (multiple of 32 in the data seen)
    pub y: f32, // map row (tile units)
    pub i32_b: Vec<i32>,
    pub i32_c: Vec<i32>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Level {
    pub id: u32,
    pub name: String,
    pub width: usize,
    pub height: usize,
    /// 0-based index into the tile sheet (the original's ids are 1-based; the extractor already converted)
    pub base_tile: Vec<u16>,
    /// 1 = flip vertically, 2 = take tile from the mirrored sheet, 3 = both
    pub base_flip: Vec<u8>,
    pub fringe: Vec<u8>,
    pub shadow: Vec<u8>,
    pub ground_marker: Vec<u8>,
    pub spawn_trigger_by_row: Vec<u32>,
    pub groups: Vec<Group>,
    pub ground_sprites: Vec<GroundSprite>,
}

impl Level {
    /// The level's `__GS` resource as the original keeps it in memory: 250 records of 50 bytes (index = the number in the marker layer; unused = zeros).
    pub fn ground_resource(&self) -> Vec<u8> {
        let mut out = vec![0u8; crate::ground::RECORDS * crate::ground::RES_REC];
        for g in &self.ground_sprites {
            let i = g.index as usize;
            if i >= crate::ground::RECORDS { continue; }
            let get = |v: &Vec<i32>, k: usize| v.get(k).copied().unwrap_or(0);
            let rec = crate::ground::resource_record(&g.flags, [get(&g.i32_a, 0), get(&g.i32_a, 1), get(&g.i32_a, 2)], g.x, g.y, [get(&g.i32_b, 0), get(&g.i32_b, 1)], &g.i32_c);
            out[i * crate::ground::RES_REC..(i + 1) * crate::ground::RES_REC].copy_from_slice(&rec);
        }
        out
    }
}

pub fn load_level(root: &Path, id: u32) -> Result<Level> {
    let lv: Level = read_json(&root.join(format!("levels/level_{:02}.json", id)))?;
    let n = lv.width * lv.height;
    if lv.base_tile.len() != n || lv.fringe.len() != n || lv.shadow.len() != n || lv.ground_marker.len() != n {
        return Err(Error::Msg(format!("level {}: layer size mismatch", id)));
    }
    Ok(lv)
}

// ---------------------------------------------------------------- sounds / tables ----------------
#[derive(Deserialize)]
struct SoundInfo { id: u32, name: String, file: String, #[serde(default)] exact_rate: f64 }
#[derive(Deserialize)]
struct SoundIndex { sounds: Vec<SoundInfo> }

pub struct Sound {
    pub id: u32,
    pub name: String,
    pub sample_rate: u32,
    pub samples: Vec<i16>, // 8-bit mono source, widened
}

pub fn load_sounds(root: &Path) -> Result<Vec<Sound>> {
    let idx: SoundIndex = read_json(&root.join("sounds/index.json"))?;
    let mut out = Vec::new();
    for s in idx.sounds {
        let mut r = hound::WavReader::open(root.join(&s.file))?;
        let rate = r.spec().sample_rate;
        let samples = r.samples::<i16>().collect::<std::result::Result<Vec<i16>, hound::Error>>()?;
        out.push(Sound { id: s.id, name: s.name, sample_rate: rate, samples });
    }
    Ok(out)
}

/// The sound effects as the audio engine wants them: `(slot, sample)` for `snd ` resources 10000..10071 (slot = id - 10000), with the exact
/// sample rate of the resource header (16.16 fixed; falls back to the WAV's integer rate when the index has no `exact_rate`).
pub fn load_samples(root: &Path) -> Result<Vec<(usize, crate::audio::Sample)>> {
    let idx: SoundIndex = read_json(&root.join("sounds/index.json"))?;
    let mut out = Vec::new();
    for s in idx.sounds {
        if !(10000..10000 + crate::audio::SOUND_COUNT as u32).contains(&s.id) { continue; }
        let mut r = hound::WavReader::open(root.join(&s.file))?;
        let int_rate = r.spec().sample_rate;
        let pcm = r.samples::<i16>().collect::<std::result::Result<Vec<i16>, hound::Error>>()?;
        let exact = if s.exact_rate > 0.0 { s.exact_rate } else { int_rate as f64 };
        out.push(((s.id - 10000) as usize, crate::audio::Sample::new(&pcm, (exact * 65536.0).round() as u32)));
    }
    Ok(out)
}

#[derive(Deserialize)]
struct TableInfo {
    #[serde(rename = "type")]
    kind: String,
    id: u32,
    file: String,
}
#[derive(Deserialize)]
struct TableIndex { tables: Vec<TableInfo> }

/// Raw lookup table bytes, e.g. `load_table(root, "DARK", 128)` = the "System Shadow" 256-byte remap.
pub fn load_table(root: &Path, kind: &str, id: u32) -> Result<Vec<u8>> {
    let idx: TableIndex = read_json(&root.join("tables/index.json"))?;
    let t = idx.tables.iter().find(|t| t.kind == kind && t.id == id)
        .ok_or_else(|| Error::Msg(format!("table {} {} not found", kind, id)))?;
    Ok(std::fs::read(root.join(&t.file))?)
}
