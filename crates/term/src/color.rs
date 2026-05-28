//! sRGB colour used by the grid model and palette. Inlined into `term`
//! when the abandoned GPU `render` crate was removed — the only consumer
//! that needs more than the basic struct is the Canvas2D path in
//! `src-tauri::terminal`, which calls `rgb()` to build wire cells. We keep
//! `rgba` and `to_linear` for API parity in case a future renderer wants
//! them, but they're not in any hot path here.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Convert an sRGB component to linear-light float in [0, 1].
    pub fn to_linear(self) -> [f32; 4] {
        fn comp(c: u8) -> f32 {
            let f = c as f32 / 255.0;
            if f <= 0.04045 {
                f / 12.92
            } else {
                ((f + 0.055) / 1.055).powf(2.4)
            }
        }
        [
            comp(self.r),
            comp(self.g),
            comp(self.b),
            self.a as f32 / 255.0,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_white_is_linear_one() {
        let l = Color::rgb(255, 255, 255).to_linear();
        assert!((l[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn srgb_mid_is_not_half_in_linear() {
        let l = Color::rgb(128, 128, 128).to_linear();
        assert!(l[0] > 0.2 && l[0] < 0.23, "got {}", l[0]);
    }
}
