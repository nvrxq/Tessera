//! `wezterm_term::TerminalConfiguration` impl scoped to Tessera defaults.

use std::sync::Arc;
use wezterm_term::TerminalConfiguration;

/// Configuration knobs Tessera exposes to wezterm-term.
/// Most callers can use `TesseraConfig::default()` — the values match
/// what we want for an embedded agent terminal.
#[derive(Debug, Clone)]
pub struct TesseraConfig {
    pub scrollback: usize,
}

impl Default for TesseraConfig {
    fn default() -> Self {
        Self {
            // 5000 lines matches the prior xterm.js setting; revisit if memory
            // pressure shows up under long Claude sessions.
            scrollback: 5000,
        }
    }
}

impl TerminalConfiguration for TesseraConfig {
    fn scrollback_size(&self) -> usize {
        self.scrollback
    }

    fn color_palette(&self) -> wezterm_term::color::ColorPalette {
        // We resolve colors *outside* wezterm via `crate::palette::ColorPalette`,
        // so this returns the wezterm-native default. The two palettes serve
        // different purposes:
        //   - wezterm's: for OSC color queries / palette mutation by the shell.
        //   - ours: for the renderer (sRGB output).
        wezterm_term::color::ColorPalette::default()
    }
}

pub fn shared_config() -> Arc<dyn TerminalConfiguration> {
    Arc::new(TesseraConfig::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_scrollback_is_5000_lines() {
        assert_eq!(TesseraConfig::default().scrollback, 5000);
    }

    #[test]
    fn impl_returns_configured_scrollback() {
        let c = TesseraConfig::default();
        assert_eq!(c.scrollback_size(), 5000);
    }
}
