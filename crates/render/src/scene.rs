use crate::geometry::{Color, Rect};

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RectEntry {
    pub rect: Rect,
    pub color: Color,
    pub corner_radius: f32,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GlyphEntry {
    pub rect: Rect,
    pub color: Color,
    /// UV rectangle inside the glyph atlas, normalized to [0,1].
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ImageEntry {
    pub rect: Rect,
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
}

#[derive(Default, Debug)]
pub struct Scene {
    pub rects: Vec<RectEntry>,
    pub glyphs: Vec<GlyphEntry>,
    pub images: Vec<ImageEntry>,
}

impl Scene {
    pub fn new() -> Self { Self::default() }
    pub fn clear(&mut self) {
        self.rects.clear();
        self.glyphs.clear();
        self.images.clear();
    }
    pub fn push_rect(&mut self, e: RectEntry) { self.rects.push(e); }
    pub fn push_glyph(&mut self, e: GlyphEntry) { self.glyphs.push(e); }
    pub fn push_image(&mut self, e: ImageEntry) { self.images.push(e); }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_rect() -> RectEntry {
        RectEntry { rect: Rect::new(0.0, 0.0, 10.0, 10.0), color: Color::rgb(255, 0, 0), corner_radius: 0.0 }
    }

    #[test]
    fn push_then_clear_resets_counts() {
        let mut s = Scene::new();
        s.push_rect(sample_rect());
        s.push_rect(sample_rect());
        assert_eq!(s.rects.len(), 2);
        s.clear();
        assert_eq!(s.rects.len(), 0);
        assert_eq!(s.glyphs.len(), 0);
        assert_eq!(s.images.len(), 0);
    }
}
