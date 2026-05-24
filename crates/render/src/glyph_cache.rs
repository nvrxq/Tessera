use std::collections::HashMap;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::Format;
use swash::FontRef;

use crate::atlas::{AllocatedRegion, Atlas};

#[derive(Clone, Debug)]
pub struct GlyphMetrics {
    pub advance: f32,
    pub bearing: [f32; 2],
    pub size_px: [u32; 2],
    pub region: AllocatedRegion,
}

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
struct CacheKey {
    glyph_id: u16,
    size_q: u32,
}

pub struct GlyphCache<'a> {
    font: FontRef<'a>,
    ctx: ScaleContext,
    atlas: Atlas,
    pub atlas_pixels: Vec<u8>, // RGBA8, atlas.size² * 4
    map: HashMap<CacheKey, GlyphMetrics>,
    pub atlas_dirty: bool,
    pub hits: u64,
    pub misses: u64,
}

impl<'a> GlyphCache<'a> {
    pub fn new(font_bytes: &'a [u8], atlas_size: u32) -> Option<Self> {
        let font = FontRef::from_index(font_bytes, 0)?;
        let pixels = vec![0u8; (atlas_size * atlas_size * 4) as usize];
        Some(Self {
            font,
            ctx: ScaleContext::new(),
            atlas: Atlas::new(atlas_size),
            atlas_pixels: pixels,
            map: HashMap::new(),
            atlas_dirty: false,
            hits: 0,
            misses: 0,
        })
    }

    pub fn atlas_size(&self) -> u32 {
        self.atlas.size()
    }

    pub fn cell_metrics(&self, px_size: f32) -> CellMetrics {
        let m = self.font.metrics(&[]).scale(px_size);
        let charmap = self.font.charmap();
        let gid = charmap.map('M');
        let advance = self.font.glyph_metrics(&[]).scale(px_size).advance_width(gid);
        CellMetrics {
            advance_px: advance.ceil(),
            ascent: m.ascent,
            descent: m.descent,
            line_gap: m.leading,
            line_height_px: (m.ascent + m.descent + m.leading).ceil(),
        }
    }

    pub fn get_or_rasterize(&mut self, ch: char, px_size: f32) -> Option<GlyphMetrics> {
        let glyph_id = self.font.charmap().map(ch);
        let key = CacheKey { glyph_id, size_q: (px_size * 4.0) as u32 };
        if let Some(g) = self.map.get(&key) {
            self.hits += 1;
            return Some(g.clone());
        }
        self.misses += 1;

        let mut scaler = self.ctx.builder(self.font).size(px_size).hint(true).build();
        // swash 0.2: Render methods take &mut self, build then call render separately.
        let image = Render::new(&[
            Source::ColorOutline(0),
            Source::ColorBitmap(StrikeWith::BestFit),
            Source::Outline,
        ])
        .format(Format::Alpha)
        .render(&mut scaler, glyph_id)?;

        let w = image.placement.width;
        let h = image.placement.height;
        if w == 0 || h == 0 {
            return None;
        }

        let region = self.atlas.allocate(w, h)?;
        let atlas_w = self.atlas.size();
        for row in 0..h {
            for col in 0..w {
                let src_i = (row * w + col) as usize;
                let dst_x = region.px_min[0] + col;
                let dst_y = region.px_min[1] + row;
                let dst_i = ((dst_y * atlas_w + dst_x) * 4) as usize;
                let a = image.data[src_i];
                self.atlas_pixels[dst_i] = 255;
                self.atlas_pixels[dst_i + 1] = 255;
                self.atlas_pixels[dst_i + 2] = 255;
                self.atlas_pixels[dst_i + 3] = a;
            }
        }
        self.atlas_dirty = true;

        let m = GlyphMetrics {
            advance: self.font.glyph_metrics(&[]).scale(px_size).advance_width(glyph_id),
            bearing: [image.placement.left as f32, image.placement.top as f32],
            size_px: [w, h],
            region,
        };
        self.map.insert(key, m.clone());
        Some(m)
    }
}

#[derive(Copy, Clone, Debug)]
pub struct CellMetrics {
    pub advance_px: f32,
    pub ascent: f32,
    pub descent: f32,
    pub line_gap: f32,
    pub line_height_px: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    const FONT: &[u8] = include_bytes!("../assets/GeistMono-Regular.ttf");

    #[test]
    fn rasterize_m_produces_non_empty_atlas_region() {
        let mut c = GlyphCache::new(FONT, 1024).expect("font loads");
        let m = c.get_or_rasterize('M', 26.0).expect("rasterizes");
        assert!(m.size_px[0] > 0 && m.size_px[1] > 0);
        assert!(m.region.px_max[0] > m.region.px_min[0]);
    }

    #[test]
    fn second_call_is_cache_hit() {
        let mut c = GlyphCache::new(FONT, 1024).unwrap();
        c.get_or_rasterize('M', 26.0);
        let misses_before = c.misses;
        c.get_or_rasterize('M', 26.0);
        assert_eq!(c.misses, misses_before, "second call should hit");
        assert!(c.hits >= 1);
    }

    #[test]
    fn cell_metrics_are_positive() {
        let c = GlyphCache::new(FONT, 1024).unwrap();
        let m = c.cell_metrics(26.0);
        assert!(m.advance_px > 0.0);
        assert!(m.line_height_px > 0.0);
    }
}
