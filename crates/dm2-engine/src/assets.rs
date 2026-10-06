//! Game data loaded from the user's own copy of the original files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use dm2_formats::dungeon::Dungeon;
use dm2_formats::gdat::Gdat;
use dm2_formats::image;

use crate::gfx::{Bitmap, Sprite};
use crate::layout::Layout;

pub struct Assets {
    pub gdat: Gdat,
    pub dungeon: Dungeon,
    pub layout: Layout,
    pub palette: [[u8; 3]; 256],
    /// Colour ramps for depth lighting (None if the table is missing).
    pub light: Option<crate::viewport::light::Light>,
    sprites: HashMap<(u8, u8, u8), Option<Rc<Sprite>>>,
    scaled: HashMap<(u8, u8, u8, i32, i32), Option<Rc<Sprite>>>,
}

#[derive(Debug)]
pub enum LoadError {
    Missing(PathBuf),
    Bad(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Missing(p) => write!(f, "missing game file {}", p.display()),
            LoadError::Bad(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for LoadError {}

impl Assets {
    /// Load from a directory holding the original GRAPHICS.DAT and DUNGEON.DAT.
    pub fn load(data_dir: &Path) -> Result<Assets, LoadError> {
        let read = |name: &str| {
            let p = data_dir.join(name);
            std::fs::read(&p).map_err(|_| LoadError::Missing(p))
        };
        let gdat = Gdat::from_bytes(read("GRAPHICS.DAT")?).map_err(|e| LoadError::Bad(format!("GRAPHICS.DAT: {e:?}")))?;
        let dungeon = Dungeon::parse(&read("DUNGEON.DAT")?).map_err(|e| LoadError::Bad(format!("DUNGEON.DAT: {e:?}")))?;
        let layout = Layout::load(&gdat).ok_or_else(|| LoadError::Bad("layout table missing".into()))?;
        let palette = image::master_palette(&gdat).ok_or_else(|| LoadError::Bad("palette missing".into()))?;
        let light = crate::viewport::light::Light::load(&gdat);
        Ok(Assets { gdat, dungeon, layout, palette, light, sprites: HashMap::new(), scaled: HashMap::new() })
    }

    /// True if image (cat, idx, 1, sub) exists (0x3C92E).
    pub fn has_image(&self, cat: u8, idx: u8, sub: u8) -> bool {
        self.gdat.record(dm2_formats::gdat::Key::new(cat, idx, 1, sub)).is_some()
    }

    /// Image scaled by (sx, sy) 64ths, cached like the original's scaled-copy cache.
    pub fn sprite_scaled(&mut self, cat: u8, idx: u8, sub: u8, sx: i32, sy: i32) -> Option<Rc<Sprite>> {
        if sx == 64 && sy == 64 {
            return self.sprite(cat, idx, sub);
        }
        if let Some(s) = self.scaled.get(&(cat, idx, sub, sx, sy)) {
            return s.clone();
        }
        let s = self.sprite(cat, idx, sub).and_then(|s| s.scaled(sx, sy)).map(Rc::new);
        self.scaled.insert((cat, idx, sub, sx, sy), s.clone());
        s
    }

    pub fn sprite(&mut self, cat: u8, idx: u8, sub: u8) -> Option<Rc<Sprite>> {
        let g = &self.gdat;
        self.sprites
            .entry((cat, idx, sub))
            .or_insert_with(|| Sprite::load(g, cat, idx, sub).map(Rc::new))
            .clone()
    }

    /// Draw image (cat, idx, sub) at layout id `rid`, applying its drawing
    /// offset (0x1B8E5). Returns false if the image or placement is missing.
    pub fn draw(&mut self, dst: &mut Bitmap, cat: u8, idx: u8, sub: u8, rid: u16, flip: u8, key: Option<u8>) -> bool {
        let Some(s) = self.sprite(cat, idx, sub) else { return false };
        let img = (s.w as i32, s.h as i32);
        let p = if s.off != (0, 0) {
            self.layout.resolve(rid | 0x8000, s.off.0, s.off.1, img)
        } else {
            self.layout.resolve(rid, img.0, img.1, img)
        };
        let Some(p) = p else { return false };
        s.blit(dst, &p, flip, key);
        true
    }
}

/// The repo's default location for the user's extracted game data.
pub fn default_data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../original/dumast2/DATA")
}
