//! Claude Code session-file helpers.
//!
//! Claude stores per-conversation state under
//! `~/.claude/projects/<encoded-cwd>/<session-uuid>.jsonl`, where
//! `<encoded-cwd>` is the absolute cwd with every non-`[A-Za-z0-9]` char
//! replaced by `-` (see `encode_cwd`).
//!
//! **Identity is no longer inferred by mtime.** The workspace service now
//! *generates* a session uuid per workspace and passes it to claude via
//! `claude --session-id <uuid>` (fresh) or `claude --resume <uuid>` (once
//! `<uuid>.jsonl` exists). `session_file_exists_anywhere` is the
//! encoding-robust existence check that picks between the two. The old
//! mtime-detection path (`newest_session_id*`) caused two workspaces sharing
//! a folder to steal each other's conversation and has been retired from the
//! spawner; those functions remain only for diagnostics/tests.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Convert an absolute path to the on-disk folder name Claude uses.
/// `/Users/x/Tessera` → `-Users-x-Tessera`. Claude maps **every** non
/// `[A-Za-z0-9]` character to `-`, not just `/` — verified empirically:
/// `/tmp/tessera_sidtest` is stored under `-tmp-tessera-sidtest` (the `_`
/// became `-`), and a leading `.claude` becomes `-claude`. Matching only
/// `/` (as we used to) sent the lookup to a non-existent directory for any
/// path containing `_`, `.`, spaces, etc. Symlinks are intentionally not
/// canonicalised — Claude stores by the literal launch cwd.
pub fn encode_cwd(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
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

/// Does `<id>.jsonl` exist under **any** project dir in `~/.claude/projects`?
///
/// We now generate the session uuid ourselves and pass it to claude via
/// `--session-id`, so the id is globally unique and a direct filename match
/// across project dirs is unambiguous. This is the encoding-robust check the
/// spawner uses to decide `--resume <id>` (jsonl exists) vs `--session-id
/// <id>` (fresh): it doesn't depend on reconstructing claude's exact
/// cwd→dirname rule, which `encode_cwd` can only approximate.
pub fn session_file_exists_anywhere(id: &str) -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    let projects = home.join(".claude").join("projects");
    let needle = format!("{id}.jsonl");
    let Ok(entries) = std::fs::read_dir(&projects) else {
        return false;
    };
    for entry in entries.flatten() {
        if entry.path().join(&needle).is_file() {
            return true;
        }
    }
    false
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
    fn encode_cwd_maps_underscore_dot_and_space() {
        // Claude maps every non-alphanumeric char to '-'. Verified against a
        // real session dir: /tmp/tessera_sidtest -> -tmp-tessera-sidtest.
        assert_eq!(
            encode_cwd(Path::new("/tmp/tessera_sidtest")),
            "-tmp-tessera-sidtest"
        );
        assert_eq!(
            encode_cwd(Path::new("/a/.claude/my.dir")),
            "-a--claude-my-dir"
        );
        assert_eq!(encode_cwd(Path::new("/x/a b")), "-x-a-b");
    }

    #[test]
    fn session_file_exists_anywhere_finds_by_uuid_across_dirs() {
        // Point HOME at a fabricated tree so we don't touch the real
        // ~/.claude. Two project dirs; the id lives in the second one.
        let home = tempfile::tempdir().unwrap();
        let projects = home.path().join(".claude").join("projects");
        let dir_a = projects.join("-some-other-proj");
        let dir_b = projects.join("-the-proj");
        fs::create_dir_all(&dir_a).unwrap();
        fs::create_dir_all(&dir_b).unwrap();
        fs::write(dir_b.join("abc-123.jsonl"), "{}").unwrap();

        // dirs::home_dir() reads $HOME on unix.
        let prev = std::env::var_os("HOME");
        // SAFETY: single-threaded test; restored below.
        unsafe { std::env::set_var("HOME", home.path()) };
        let found = session_file_exists_anywhere("abc-123");
        let missing = session_file_exists_anywhere("does-not-exist");
        match prev {
            Some(v) => unsafe { std::env::set_var("HOME", v) },
            None => unsafe { std::env::remove_var("HOME") },
        }
        assert!(found, "uuid jsonl should be found regardless of dir name");
        assert!(!missing);
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
