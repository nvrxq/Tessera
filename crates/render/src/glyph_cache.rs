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

/// Font weight slot in the cache. Geist Mono ships Regular + Bold with
/// identical typo metrics (UPM=1000, typoAsc=1005, typoDesc=-295, xAvg=600),
/// so the cell grid stays uniform between weights — only the glyph
/// rasterisation differs.
#[derive(Hash, PartialEq, Eq, Clone, Copy, Debug)]
pub enum FontWeight {
    Regular,
    Bold,
}

impl FontWeight {
    #[inline]
    fn idx(self) -> u8 {
        match self {
            FontWeight::Regular => 0,
            FontWeight::Bold => 1,
        }
    }
}

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
struct CacheKey {
    weight: u8,
    glyph_id: u16,
    size_q: u32,
}

pub struct GlyphCache<'a> {
    fonts: [FontRef<'a>; 2], // [Regular, Bold] — indexed by FontWeight::idx
    ctx: ScaleContext,
    atlas: Atlas,
    pub atlas_pixels: Vec<u8>, // RGBA8, atlas.size² * 4
    map: HashMap<CacheKey, GlyphMetrics>,
    pub atlas_dirty: bool,
    pub hits: u64,
    pub misses: u64,
}

impl<'a> GlyphCache<'a> {
    /// Single-weight constructor — bold slot aliases Regular. Kept for tests
    /// and callers that don't care about weight dispatch.
    pub fn new(font_bytes: &'a [u8], atlas_size: u32) -> Option<Self> {
        Self::new_with_bold(font_bytes, font_bytes, atlas_size)
    }

    /// Two-weight constructor: Regular + Bold from separate font files.
    pub fn new_with_bold(
        regular_bytes: &'a [u8],
        bold_bytes: &'a [u8],
        atlas_size: u32,
    ) -> Option<Self> {
        let reg = FontRef::from_index(regular_bytes, 0)?;
        let bold = FontRef::from_index(bold_bytes, 0)?;
        let pixels = vec![0u8; (atlas_size * atlas_size * 4) as usize];
        Some(Self {
            fonts: [reg, bold],
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

    #[inline]
    fn font_for(&self, w: FontWeight) -> FontRef<'a> {
        self.fonts[w.idx() as usize]
    }

    pub fn cell_metrics(&self, px_size: f32) -> CellMetrics {
        // Port of Warp's `grid_cell_dimensions` + `calculate_grid_baseline_position`
        // (warpdotdev/warp app/src/terminal/grid_size_util.rs lines 13–79,
        // dual-licensed AGPL/MIT — we mirror the algorithm, not the source).
        //
        // **The "приплюснуто" fix:** Warp's actual on-screen cells aren't
        // just `real_font_height`. The formula is:
        //
        //   cell_h = (asc + desc + leading) × (line_height_ratio / DEFAULT_UI_LINE_HEIGHT_RATIO)
        //
        // where Warp's defaults are:
        //   DEFAULT_LINE_HEIGHT_RATIO       = 1.4   (used in BLOCK grid)
        //   DEFAULT_UI_LINE_HEIGHT_RATIO    = 1.2   (UI text divisor)
        //   → effective multiplier on real font height = 1.4 / 1.2 ≈ 1.1667
        //
        // (see warp_core/src/ui/appearance.rs:117 and warpui_core's
        // text_layout / elements/text + formatted_text_element)
        //
        // I previously used multiplier = 1.0 (raw font metrics) → cells
        // came out 17 % shorter than Warp's. That's the missing breathing
        // room the user kept calling "squashed".
        let font = self.fonts[FontWeight::Regular.idx() as usize];
        let charmap = font.charmap();

        // Warp uses 'm' lowercase as the canonical advance probe (not 'M').
        let gid = charmap.map('m');
        let advance = font.glyph_metrics(&[]).scale(px_size).advance_width(gid);

        // Pull REAL font metrics. swash returns positive descent (absolute).
        let m = font.metrics(&[]).scale(px_size);

        // Warp's line-height multiplier — see comment above.
        const WARP_LINE_HEIGHT_RATIO: f32 = 1.4;
        const WARP_UI_LINE_HEIGHT_RATIO: f32 = 1.2;
        let ratio_multiplier = WARP_LINE_HEIGHT_RATIO / WARP_UI_LINE_HEIGHT_RATIO;

        // Cell height = real font height × multiplier, ceil to integer pixel.
        // Geist Mono at 20 px: (20.1 + 5.9 + 0) × 1.1667 = 30.33 → ceil 31.
        let line_height_px = ((m.ascent + m.descent + m.leading) * ratio_multiplier)
            .ceil()
            .max(1.0);

        // Baseline: same formula as Warp's `calculate_grid_baseline_position`,
        // which uses `.min(1.0)` on the ratio multiplier — so descent stays
        // at its native (un-scaled) value, and the extra height from the
        // multiplier all goes ABOVE the baseline. That extra space is
        // exactly the visual "air" Warp has and our earlier port lacked.
        //   baseline_y = cell_h - leading.floor() - descent.floor() × min(ratio, 1.0)
        // For Geist Mono at 20 px with ratio 1.1667 → min = 1.0:
        //   31 - 0 - 5 = 26.  M cap-height ~14 px → glyph y ∈ [12, 26],
        //   leaves 12 px breathing room above the cap (was 7 px before).
        let baseline_scale = ratio_multiplier.min(1.0);
        let baseline_y = line_height_px - m.leading.floor() - (m.descent * baseline_scale).floor();

        let advance_px = advance.round().max(1.0);

        CellMetrics {
            advance_px,
            ascent: baseline_y,
            descent: line_height_px - baseline_y,
            line_gap: m.leading,
            line_height_px,
        }
    }

    /// Convenience wrapper for callers that don't care about weight.
    pub fn get_or_rasterize(&mut self, ch: char, px_size: f32) -> Option<GlyphMetrics> {
        self.get_or_rasterize_weighted(ch, FontWeight::Regular, px_size)
    }

    pub fn get_or_rasterize_weighted(
        &mut self,
        ch: char,
        weight: FontWeight,
        px_size: f32,
    ) -> Option<GlyphMetrics> {
        let font = self.font_for(weight);
        let glyph_id = font.charmap().map(ch);
        let key = CacheKey {
            weight: weight.idx(),
            glyph_id,
            size_q: (px_size * 4.0) as u32,
        };
        if let Some(g) = self.map.get(&key) {
            self.hits += 1;
            return Some(g.clone());
        }
        self.misses += 1;

        let mut scaler = self.ctx.builder(font).size(px_size).hint(true).build();
        // LCD subpixel rasterisation: swash returns 3 coverage bytes per pixel
        // (R, G, B subpixel masks). Combined with per-channel blending in
        // `glyph.wgsl` this gives the "freetype-like" crispness on Linux that
        // straight `Format::Alpha` (single-channel grayscale) cannot. See
        // Warp blog "Adventures in Text Rendering" for the misalignment-blur
        // problem and Arkanis 2023 for the dual-blend treatment.
        let image = Render::new(&[
            Source::ColorOutline(0),
            Source::ColorBitmap(StrikeWith::BestFit),
            Source::Outline,
        ])
        .format(Format::Subpixel)
        .render(&mut scaler, glyph_id)?;

        let w = image.placement.width;
        let h = image.placement.height;
        if w == 0 || h == 0 {
            return None;
        }

        let region = self.atlas.allocate(w, h)?;
        let atlas_w = self.atlas.size();
        // swash::Image contents under Format::Subpixel (via zeno):
        //   Content::Mask         — 1 byte/pixel (rare fallback).
        //   Content::SubpixelMask — 4 bytes/pixel (RGBA layout; A undefined).
        //                           R = coverage sampled at x − 0.3
        //                           G = coverage sampled at x
        //                           B = coverage sampled at x + 0.3
        //                           zeno does NOT apply an LCD filter — raw
        //                           subpixel masks have severe colour fringes
        //                           unless filtered before display.
        //   Content::Color        — 4 bytes/pixel (RGBA): color glyphs / emoji.
        //
        // We store filtered RGB coverage in atlas RGB and max(RGB) in A so the
        // shader can use A as a single-channel "opacity gate" for destination
        // attenuation.
        //
        // Filter: FreeType's default "fir5" LCD filter — symmetric 5-tap
        // [1, 2, 3, 2, 1] / 9 across the conceptual 3·w-wide subpixel row.
        // This is the same kernel Microsoft ClearType / Pango / Skia use as
        // their default. Without it, vertical stems on dark-mode terminals
        // explode into pure-channel rainbow noise.
        use swash::scale::image::Content;
        for row in 0..h {
            let row_base = (row * w * 4) as usize;
            // Read the s-th subpixel in this row (s ∈ [0, 3w)). Indices
            // outside the placement bounds clamp to zero — this matches what
            // FreeType does for glyph border padding.
            let read_subpx = |s: i32| -> u32 {
                if s < 0 || s >= (w as i32) * 3 {
                    return 0;
                }
                let pixel = (s as usize) / 3;
                let channel = (s as usize) % 3;
                image.data[row_base + pixel * 4 + channel] as u32
            };
            for col in 0..w {
                let dst_x = region.px_min[0] + col;
                let dst_y = region.px_min[1] + row;
                let dst_i = ((dst_y * atlas_w + dst_x) * 4) as usize;
                match image.content {
                    Content::Mask => {
                        // Single-channel grayscale — replicate across RGB so
                        // the shader sees uniform per-channel coverage (no
                        // colour fringes for fallback glyphs).
                        let src_i = (row * w + col) as usize;
                        let a = image.data[src_i];
                        self.atlas_pixels[dst_i] = a;
                        self.atlas_pixels[dst_i + 1] = a;
                        self.atlas_pixels[dst_i + 2] = a;
                        self.atlas_pixels[dst_i + 3] = a;
                    }
                    Content::SubpixelMask => {
                        let sp_r = (col as i32) * 3;
                        // Convolve [1,2,3,2,1] / 9 around each subpixel.
                        let filt = |s: i32| -> u8 {
                            let v = read_subpx(s - 2)
                                + 2 * read_subpx(s - 1)
                                + 3 * read_subpx(s)
                                + 2 * read_subpx(s + 1)
                                + read_subpx(s + 2);
                            ((v + 4) / 9).min(255) as u8
                        };
                        let r = filt(sp_r);
                        let g = filt(sp_r + 1);
                        let b = filt(sp_r + 2);
                        self.atlas_pixels[dst_i] = r;
                        self.atlas_pixels[dst_i + 1] = g;
                        self.atlas_pixels[dst_i + 2] = b;
                        self.atlas_pixels[dst_i + 3] = r.max(g).max(b);
                    }
                    Content::Color => {
                        let src_i = (row * w * 4 + col * 4) as usize;
                        self.atlas_pixels[dst_i] = image.data[src_i];
                        self.atlas_pixels[dst_i + 1] = image.data[src_i + 1];
                        self.atlas_pixels[dst_i + 2] = image.data[src_i + 2];
                        self.atlas_pixels[dst_i + 3] = image.data[src_i + 3];
                    }
                }
            }
        }
        self.atlas_dirty = true;

        let m = GlyphMetrics {
            advance: font
                .glyph_metrics(&[])
                .scale(px_size)
                .advance_width(glyph_id),
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
