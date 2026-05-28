//! Locate the freshest Claude Code session file for a given workspace cwd.
//!
//! Claude stores per-conversation state under
//! `~/.claude/projects/<encoded-cwd>/<session-uuid>.jsonl`, where
//! `<encoded-cwd>` is the absolute cwd with every `/` replaced by `-`.
//! `claude --continue` resumes whichever jsonl is newest by mtime in that
//! folder — which is exactly the source of the cross-workspace session
//! mixing bug when two Tessera workspaces point at the same folder.
//!
//! This module gives the workspace service a way to **pin** a workspace to
//! a specific session uuid: spawn → wait briefly → look up the freshest
//! jsonl → store its stem on the workspace row → subsequent spawns pass
//! `--resume <id>` instead of `--continue`.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Convert an absolute path to the on-disk folder name Claude uses.
/// `/Users/x/Tessera` → `-Users-x-Tessera`. Symlinks are intentionally
/// **not** canonicalised here — Claude itself stores by the literal cwd
/// the process was launched in, so we match that.
pub fn encode_cwd(cwd: &Path) -> String {
    cwd.to_string_lossy().replace('/', "-")
}

/// `~/.claude/projects/<encoded-cwd>`. `None` if `$HOME` is unavailable.
pub fn project_dir_for(cwd: &Path) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(home.join(".claude").join("projects").join(encode_cwd(cwd)))
}

/// Return the stem (filename without `.jsonl`) of the most recently
/// modified jsonl in Claude's project folder for `cwd`. Returns `None`
/// when the folder is missing, empty, or unreadable.
///
/// Used in two places:
/// * After the very first spawn, to pin a freshly-created session.
/// * As a fallback when the previously-pinned id no longer resolves
///   (Claude may have rotated or deleted the file).
pub fn newest_session_id(cwd: &Path) -> Option<String> {
    let dir = project_dir_for(cwd)?;
    newest_session_id_in(&dir)
}

/// Variant that takes the project dir directly — used by tests against a
/// fabricated `$HOME` without poking dirs::home_dir.
pub fn newest_session_id_in(project_dir: &Path) -> Option<String> {
    let entries = std::fs::read_dir(project_dir).ok()?;
    let mut best: Option<(SystemTime, String)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };
        let mtime = match entry.metadata().and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(_) => continue,
        };
        match &best {
            Some((cur, _)) if *cur >= mtime => {}
            _ => best = Some((mtime, stem)),
        }
    }
    best.map(|(_, id)| id)
}

/// Is `id` still a live jsonl under Claude's project dir for this cwd?
/// Used before spawning with `--resume <id>` — if the file vanished
/// (Claude cleared it), the caller can fall back to `--continue` or a
/// fresh start.
pub fn session_file_exists(cwd: &Path, id: &str) -> bool {
    let Some(dir) = project_dir_for(cwd) else {
        return false;
    };
    dir.join(format!("{id}.jsonl")).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn encode_cwd_replaces_slashes() {
        assert_eq!(encode_cwd(Path::new("/Users/x/y")), "-Users-x-y");
    }

    #[test]
    fn newest_session_id_in_returns_freshest_jsonl_stem() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("aaaa.jsonl");
        let b = dir.path().join("bbbb.jsonl");
        fs::write(&a, "{}").unwrap();
        // Ensure a strictly newer mtime — filesystem timestamps on macOS
        // are second-resolution in some configs.
        sleep(Duration::from_millis(1100));
        fs::write(&b, "{}").unwrap();
        assert_eq!(
            newest_session_id_in(dir.path()),
            Some("bbbb".to_string()),
            "expected the more recently written file to win"
        );
    }

    #[test]
    fn newest_session_id_in_ignores_non_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("notes.txt"), "x").unwrap();
        fs::write(dir.path().join("session.jsonl"), "{}").unwrap();
        assert_eq!(
            newest_session_id_in(dir.path()),
            Some("session".to_string())
        );
    }

    #[test]
    fn newest_session_id_in_returns_none_for_empty_or_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(newest_session_id_in(dir.path()).is_none());
        assert!(newest_session_id_in(&dir.path().join("nope")).is_none());
    }
}
