//! Tessera GPU renderer (Warp-style scene + 3 pipelines).
//!
//! See `docs/superpowers/specs/2026-05-24-warp-renderer.md`.

pub mod atlas;
pub mod geometry;
pub mod scene;

pub fn crate_name() -> &'static str {
    "tessera-render"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_tessera_render() {
        assert_eq!(crate_name(), "tessera-render");
    }
}
