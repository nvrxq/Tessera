//! Tessera terminal model — wraps `wezterm-term` and exposes a Tessera-shaped
//! grid + cursor + block event API.
//!
//! See `docs/superpowers/specs/2026-05-24-warp-renderer.md` §7.

pub mod palette;

pub fn crate_name() -> &'static str {
    "tessera-term"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_tessera_term() {
        assert_eq!(crate_name(), "tessera-term");
    }
}
