//! Integration tests for `tessera_core::config`.

use tessera_core::config::{HexColor, UserConfig};

#[test]
fn default_round_trips_through_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let cfg = UserConfig::default();
    cfg.save(&path).expect("save");
    let read = UserConfig::load_or_default(&path);
    assert_eq!(cfg, read);
}

#[test]
fn missing_file_yields_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nope.json");
    let cfg = UserConfig::load_or_default(&path);
    assert_eq!(cfg, UserConfig::default());
}

#[test]
fn malformed_json_falls_back_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(&path, "{this is not json").unwrap();
    let cfg = UserConfig::load_or_default(&path);
    assert_eq!(cfg, UserConfig::default());
}

#[test]
fn unknown_field_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    // Force an extra top-level field that the struct doesn't know about.
    let bogus = serde_json::json!({
        "schema_version": 1,
        "appearance": {
            "ui_font_family": "Geist",
            "density": "compact"
        },
        "unknown_section": { "this_is": "fine" }
    });
    std::fs::write(&path, serde_json::to_vec(&bogus).unwrap()).unwrap();
    let cfg = UserConfig::load_or_default(&path);
    // Known sections should still be populated with defaults for missing
    // subfields, never error.
    assert_eq!(cfg.appearance.ui_font_family, "Geist");
}

#[test]
fn hex_validation_rejects_garbage() {
    assert!(HexColor::new("#GGGGGG").is_err());
    assert!(HexColor::new("123456").is_err());
    assert!(HexColor::new("#12345").is_err());
    assert!(HexColor::new("#1234567").is_err());
    assert!(HexColor::new("#abcdef").is_ok());
    assert!(HexColor::new("#ABCDEF").is_ok());
}

#[test]
fn invalid_hex_in_file_falls_back_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let bogus = serde_json::json!({
        "terminal": { "background": "not-a-hex" }
    });
    std::fs::write(&path, serde_json::to_vec(&bogus).unwrap()).unwrap();
    let cfg = UserConfig::load_or_default(&path);
    // Whole config falls back to default — strict by design so a typo
    // doesn't silently half-apply.
    assert_eq!(cfg, UserConfig::default());
}

#[test]
fn schema_version_constant_is_one() {
    assert_eq!(UserConfig::schema_version(), 1);
}
