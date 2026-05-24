use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Pod, Zeroable)]
pub struct Point { pub x: f32, pub y: f32 }

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Pod, Zeroable)]
pub struct Rect { pub x: f32, pub y: f32, pub w: f32, pub h: f32 }

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Pod, Zeroable)]
pub struct Color { pub r: u8, pub g: u8, pub b: u8, pub a: u8 }

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self { Self { r, g, b, a: 255 } }
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self { Self { r, g, b, a } }

    /// Convert an sRGB component to linear-light float in [0, 1].
    /// Used in the vertex shader path so the GPU can do correct blending.
    pub fn to_linear(self) -> [f32; 4] {
        fn comp(c: u8) -> f32 {
            let f = c as f32 / 255.0;
            if f <= 0.04045 { f / 12.92 } else { ((f + 0.055) / 1.055).powf(2.4) }
        }
        [comp(self.r), comp(self.g), comp(self.b), self.a as f32 / 255.0]
    }
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self { Self { x, y, w, h } }
    pub fn contains(self, p: Point) -> bool {
        p.x >= self.x && p.x < self.x + self.w && p.y >= self.y && p.y < self.y + self.h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_contains_interior_point() {
        let r = Rect::new(10.0, 10.0, 20.0, 20.0);
        assert!(r.contains(Point { x: 15.0, y: 15.0 }));
        assert!(!r.contains(Point { x: 30.0, y: 15.0 }));
        assert!(!r.contains(Point { x: 15.0, y: 30.0 }));
    }

    #[test]
    fn srgb_white_is_linear_one() {
        let l = Color::rgb(255, 255, 255).to_linear();
        assert!((l[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn srgb_mid_is_not_half_in_linear() {
        // sRGB 128 → linear ~0.215. If we got 0.5 it would mean no conversion.
        let l = Color::rgb(128, 128, 128).to_linear();
        assert!(l[0] > 0.2 && l[0] < 0.23, "got {}", l[0]);
    }
}
