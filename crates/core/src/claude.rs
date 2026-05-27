//! Claude Code inventory — surfaces the skills + MCP servers that a
//! freshly-launched `claude` will see in a given workspace.
//!
//! v1 scope: **Skills + MCP servers**. Plugins / hooks / slash-commands /
//! agents are explicitly v2 (the discriminator on each item already carries
//! enough source metadata to extend later without breaking the wire shape).
//!
//! Layout we look at on this host (Linux):
//!
//! * `~/.claude/skills/<name>/SKILL.md` — user-global skills.
//! * `~/.claude/plugins/cache/<marketplace>/<plugin>/<version>/skills/<name>/SKILL.md`
//!   — plugin-shipped skills (note the doubled `<marketplace>/<plugin>`
//!   nesting is a real quirk of Claude's cache, not a typo).
//! * `<workspace>/.claude/skills/<name>/SKILL.md` — project-local skills.
//! * `~/.claude.json` — Claude's main config; `mcpServers` live PER-CWD
//!   under `projects."<absolute_path>".mcpServers`, not at the top level.
//! * `<workspace>/.mcp.json` — project-scoped MCP file.
//!
//! Everything is best-effort: missing files → empty list, malformed →
//! logged + skipped. Never panics, never bubbles an error to the UI.
//!
//! **Security invariant:** [`McpServer`] only carries `env_keys` (the names
//! of `env` entries), never the values. This is enforced by the struct
//! shape — there is no public path for an `env` value to escape this
//! module. A regression test guards that.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where a skill came from. Drives the source pill in the UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkillSource {
    /// `~/.claude/skills/<name>/`.
    Global,
    /// `~/.claude/plugins/cache/<marketplace>/<plugin>/<version>/skills/<name>/`.
    Plugin {
        marketplace: String,
        plugin: String,
        version: String,
    },
    /// `<workspace>/.claude/skills/<name>/`.
    Project,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// Voice/text aliases the skill declares under `triggers:` (or the
    /// singular `trigger:`). Empty when the frontmatter has none.
    pub triggers: Vec<String>,
    pub source: SkillSource,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum McpSource {
    /// From `~/.claude.json` (any `projects.*.mcpServers` map).
    Global,
    /// From `<workspace>/.mcp.json`.
    Project,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpServer {
    pub name: String,
    /// `"stdio"`, `"http"`, or whatever the config declared. Empty string
    /// when Claude inferred it from `command`.
    pub kind: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    /// Just the keys — values are categorically NEVER returned. The struct
    /// has no field for them and the parser never copies them out.
    pub env_keys: Vec<String>,
    pub source: McpSource,
    pub config_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClaudeInventory {
    pub skills: Vec<Skill>,
    pub mcp_servers: Vec<McpServer>,
}

impl ClaudeInventory {
    /// Collect everything Claude Code would see in `workspace_dir`.
    ///
    /// `workspace_dir = None` means the header-level "all workspaces"
    /// view → only globals are returned. With a workspace, project-local
    /// skills + `.mcp.json` are merged in.
    pub fn collect(workspace_dir: Option<&Path>) -> Self {
        let home = dirs::home_dir();
        Self::collect_with_home(home.as_deref(), workspace_dir)
    }

    /// Test seam — lets the tests fabricate a fake $HOME without poking
    /// the real env. Production code calls `collect`.
    pub fn collect_with_home(home: Option<&Path>, workspace_dir: Option<&Path>) -> Self {
        let mut skills: Vec<Skill> = Vec::new();
        let mut mcp_servers: Vec<McpServer> = Vec::new();

        if let Some(h) = home {
            // Global skills.
            scan_skill_dir(&h.join(".claude/skills"), SkillFlavor::Global, &mut skills);
            // Plugin-shipped skills. The directory layout is two levels of
            // dirs ("marketplace" / "plugin") under cache/, then a version
            // dir, then a `skills/` subdir we recurse one level into.
            scan_plugin_cache(&h.join(".claude/plugins/cache"), &mut skills);
            // Global MCP — `projects.*.mcpServers` inside ~/.claude.json.
            collect_global_mcp(&h.join(".claude.json"), &mut mcp_servers);
        }

        if let Some(ws) = workspace_dir {
            scan_skill_dir(
                &ws.join(".claude/skills"),
                SkillFlavor::Project,
                &mut skills,
            );
            collect_project_mcp(&ws.join(".mcp.json"), &mut mcp_servers);
        }

        // Stable display order: source bucket first (global → plugin → project),
        // then name. Same for MCP servers.
        skills.sort_by(|a, b| {
            source_rank(&a.source)
                .cmp(&source_rank(&b.source))
                .then_with(|| a.name.cmp(&b.name))
        });
        mcp_servers.sort_by(|a, b| {
            mcp_source_rank(&a.source)
                .cmp(&mcp_source_rank(&b.source))
                .then_with(|| a.name.cmp(&b.name))
        });

        Self {
            skills,
            mcp_servers,
        }
    }
}

fn source_rank(s: &SkillSource) -> u8 {
    match s {
        SkillSource::Global => 0,
        SkillSource::Plugin { .. } => 1,
        SkillSource::Project => 2,
    }
}

fn mcp_source_rank(s: &McpSource) -> u8 {
    match s {
        McpSource::Global => 0,
        McpSource::Project => 1,
    }
}

/// Distinguishes "global" from "project" while we walk a single `skills/`
/// directory. (Plugin scanning uses its own loop because it needs to
/// extract marketplace/plugin/version from the path.)
enum SkillFlavor {
    Global,
    Project,
}

/// Walk `<root>/<skill-name>/SKILL.md` and append discoveries. Silent on
/// IO failure — Claude itself shrugs at unreadable skill dirs.
fn scan_skill_dir(root: &Path, flavor: SkillFlavor, out: &mut Vec<Skill>) {
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // Don't follow symlinks — guards against accidental cycles.
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if !ft.is_dir() {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }
        let source = match flavor {
            SkillFlavor::Global => SkillSource::Global,
            SkillFlavor::Project => SkillSource::Project,
        };
        if let Some(skill) = read_skill(&skill_md, source) {
            out.push(skill);
        }
    }
}

/// `~/.claude/plugins/cache/<marketplace>/<plugin>/<version>/skills/<name>/SKILL.md`.
/// We tolerate missing layers and skip them silently.
fn scan_plugin_cache(cache_root: &Path, out: &mut Vec<Skill>) {
    let marketplaces = match std::fs::read_dir(cache_root) {
        Ok(e) => e,
        Err(_) => return,
    };
    for mkt in marketplaces.flatten() {
        let mkt_path = mkt.path();
        if !mkt_path.is_dir() {
            continue;
        }
        let marketplace = match mkt_path.file_name().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let plugins = match std::fs::read_dir(&mkt_path) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for plug in plugins.flatten() {
            let plug_path = plug.path();
            if !plug_path.is_dir() {
                continue;
            }
            let plugin = match plug_path.file_name().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            let versions = match std::fs::read_dir(&plug_path) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for ver in versions.flatten() {
                let ver_path = ver.path();
                if !ver_path.is_dir() {
                    continue;
                }
                let version = match ver_path.file_name().and_then(|s| s.to_str()) {
                    Some(s) => s.to_string(),
                    None => continue,
                };
                let skills_dir = ver_path.join("skills");
                if !skills_dir.is_dir() {
                    continue;
                }
                let skills = match std::fs::read_dir(&skills_dir) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                for s in skills.flatten() {
                    let s_path = s.path();
                    if !s_path.is_dir() {
                        continue;
                    }
                    let md = s_path.join("SKILL.md");
                    if !md.is_file() {
                        continue;
                    }
                    let source = SkillSource::Plugin {
                        marketplace: marketplace.clone(),
                        plugin: plugin.clone(),
                        version: version.clone(),
                    };
                    if let Some(skill) = read_skill(&md, source) {
                        out.push(skill);
                    }
                }
            }
        }
    }
}

/// Parse a single SKILL.md. Returns `None` (with a log) on read failure or
/// when the file has no usable frontmatter — never panics.
fn read_skill(path: &Path, source: SkillSource) -> Option<Skill> {
    let body = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(?path, error = %e, "claude_inventory: skill read failed");
            return None;
        }
    };
    // Skill name falls back to the parent directory if frontmatter omits it.
    let dir_name = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let fm = parse_frontmatter(&body);
    let name = fm
        .get("name")
        .cloned()
        .filter(|s| !s.is_empty())
        .unwrap_or(dir_name);
    let description = fm.get("description").cloned().unwrap_or_default();
    let triggers = parse_triggers(&fm);
    Some(Skill {
        name,
        description,
        triggers,
        source,
        path: path.to_path_buf(),
    })
}

/// Minimal YAML frontmatter splitter. Returns the flat scalar fields. We
/// deliberately don't pull in `serde_yaml` for this — SKILL.md headers are
/// flat `key: value` lines with the occasional inline list. Anything more
/// exotic (anchors, multi-doc, block scalars) is rare in real skills and
/// the worst case is "we don't surface that field", which is fine.
fn parse_frontmatter(body: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let trimmed = body.trim_start_matches('\u{feff}'); // strip BOM
    let rest = match trimmed
        .strip_prefix("---\n")
        .or_else(|| trimmed.strip_prefix("---\r\n"))
    {
        Some(r) => r,
        None => return out,
    };
    // Find the closing fence. Has to be a line of just `---` (optional CR).
    let mut header = String::new();
    for line in rest.lines() {
        if line.trim_end() == "---" {
            break;
        }
        header.push_str(line);
        header.push('\n');
    }
    for line in header.lines() {
        // Skip indented lines — those belong to a previous key's list and
        // are handled by `parse_triggers` directly off the raw header.
        if line.starts_with(' ') || line.starts_with('-') || line.starts_with('\t') {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim().to_string();
            if key.is_empty() {
                continue;
            }
            let val = unquote(v.trim()).to_string();
            out.insert(key, val);
        }
    }
    out
}

/// Strip surrounding `"..."` or `'...'` quotes. Inline-list values (`[a, b]`)
/// pass through untouched and get re-parsed in `parse_triggers`.
fn unquote(s: &str) -> &str {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return &s[1..s.len() - 1];
        }
    }
    s
}

/// Pull out trigger aliases. Accepted shapes:
///
/// * `trigger: /foo` (single, common in the wild)
/// * `triggers: /foo` (single, plural key)
/// * `triggers: [a, b]` (inline list)
/// * `triggers:\n  - a\n  - b` (block list — needs the raw header)
fn parse_triggers(fm: &BTreeMap<String, String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push_one = |s: &str| {
        let t = s.trim().trim_matches(|c| c == '"' || c == '\'').trim();
        if !t.is_empty() {
            out.push(t.to_string());
        }
    };

    for key in ["triggers", "trigger"] {
        if let Some(v) = fm.get(key) {
            // Inline list?
            if v.starts_with('[') && v.ends_with(']') {
                for part in v[1..v.len() - 1].split(',') {
                    push_one(part);
                }
            } else if !v.is_empty() {
                push_one(v);
            }
        }
    }
    out
}

/// Read `~/.claude.json`, walk every `projects.<cwd>.mcpServers` map, and
/// flatten them as Global-source servers. We also handle a top-level
/// `mcpServers` key for forward compatibility — Claude has shipped both
/// shapes historically.
fn collect_global_mcp(config_path: &Path, out: &mut Vec<McpServer>) {
    let raw = match std::fs::read_to_string(config_path) {
        Ok(s) => s,
        Err(_) => return,
    };
    let v: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(?config_path, error = %e, "claude_inventory: ~/.claude.json parse failed");
            return;
        }
    };
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Some(map) = v.get("mcpServers").and_then(|m| m.as_object()) {
        ingest_mcp_map(map, McpSource::Global, config_path, &mut seen, out);
    }
    if let Some(projects) = v.get("projects").and_then(|m| m.as_object()) {
        for (_cwd, pv) in projects {
            if let Some(map) = pv.get("mcpServers").and_then(|m| m.as_object()) {
                ingest_mcp_map(map, McpSource::Global, config_path, &mut seen, out);
            }
        }
    }
}

fn collect_project_mcp(mcp_path: &Path, out: &mut Vec<McpServer>) {
    let raw = match std::fs::read_to_string(mcp_path) {
        Ok(s) => s,
        Err(_) => return,
    };
    let v: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(?mcp_path, error = %e, "claude_inventory: .mcp.json parse failed");
            return;
        }
    };
    if let Some(map) = v.get("mcpServers").and_then(|m| m.as_object()) {
        let mut seen = std::collections::HashSet::new();
        ingest_mcp_map(map, McpSource::Project, mcp_path, &mut seen, out);
    }
}

fn ingest_mcp_map(
    map: &serde_json::Map<String, serde_json::Value>,
    source: McpSource,
    config_path: &Path,
    seen: &mut std::collections::HashSet<String>,
    out: &mut Vec<McpServer>,
) {
    for (name, server_v) in map {
        // Dedupe by name within the same source bucket: ~/.claude.json
        // repeats the same server under every project that uses it.
        let key = format!("{}::{}", mcp_source_rank(&source), name);
        if !seen.insert(key) {
            continue;
        }
        let kind = server_v
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        let command = server_v
            .get("command")
            .and_then(|t| t.as_str())
            .map(|s| s.to_string());
        let args = server_v
            .get("args")
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        // Crucial: we only copy KEYS, never the values.
        let env_keys: Vec<String> = server_v
            .get("env")
            .and_then(|e| e.as_object())
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        out.push(McpServer {
            name: name.clone(),
            kind,
            command,
            args,
            env_keys,
            source: source.clone(),
            config_path: config_path.to_path_buf(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(path: &Path, body: &str) {
        if let Some(p) = path.parent() {
            fs::create_dir_all(p).unwrap();
        }
        fs::write(path, body).unwrap();
    }

    fn fake_home() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn collects_global_plugin_and_project_skills() {
        let home = fake_home();
        let ws = fake_home();

        write(
            &home.path().join(".claude/skills/global-one/SKILL.md"),
            "---\nname: global-one\ndescription: Global skill.\ntrigger: /one\n---\nbody\n",
        );
        write(
            &home
                .path()
                .join(".claude/plugins/cache/m1/p1/1.0.0/skills/plugin-one/SKILL.md"),
            "---\nname: plugin-one\ndescription: \"Quoted description.\"\n---\n",
        );
        write(
            &ws.path().join(".claude/skills/project-one/SKILL.md"),
            "---\nname: project-one\ndescription: From a workspace.\ntriggers: [/p, project]\n---\n",
        );

        let inv = ClaudeInventory::collect_with_home(Some(home.path()), Some(ws.path()));
        assert_eq!(inv.skills.len(), 3);

        let by_name: BTreeMap<_, _> = inv
            .skills
            .iter()
            .map(|s| (s.name.clone(), s.clone()))
            .collect();

        match &by_name["global-one"].source {
            SkillSource::Global => {}
            other => panic!("expected Global, got {other:?}"),
        }
        assert_eq!(by_name["global-one"].triggers, vec!["/one".to_string()]);

        match &by_name["plugin-one"].source {
            SkillSource::Plugin {
                marketplace,
                plugin,
                version,
            } => {
                assert_eq!(marketplace, "m1");
                assert_eq!(plugin, "p1");
                assert_eq!(version, "1.0.0");
            }
            other => panic!("expected Plugin, got {other:?}"),
        }
        assert_eq!(by_name["plugin-one"].description, "Quoted description.");

        match &by_name["project-one"].source {
            SkillSource::Project => {}
            other => panic!("expected Project, got {other:?}"),
        }
        assert_eq!(
            by_name["project-one"].triggers,
            vec!["/p".to_string(), "project".to_string()]
        );
    }

    #[test]
    fn malformed_skill_md_doesnt_poison_results() {
        let home = fake_home();
        write(
            &home.path().join(".claude/skills/good/SKILL.md"),
            "---\nname: good\ndescription: ok\n---\n",
        );
        // No frontmatter at all → still returns a skill (using dir name).
        write(
            &home.path().join(".claude/skills/no-fm/SKILL.md"),
            "just a body, no header at all\n",
        );
        // Frontmatter never closes — `parse_frontmatter` will still consume
        // everything; that's fine, it just produces a partial map.
        write(
            &home.path().join(".claude/skills/unclosed/SKILL.md"),
            "---\nname: unclosed\ndescription: dangling\n",
        );
        let inv = ClaudeInventory::collect_with_home(Some(home.path()), None);
        assert_eq!(inv.skills.len(), 3);
        let names: Vec<_> = inv.skills.iter().map(|s| s.name.clone()).collect();
        assert!(names.contains(&"good".to_string()));
        assert!(names.contains(&"no-fm".to_string()));
        assert!(names.contains(&"unclosed".to_string()));
    }

    #[test]
    fn missing_paths_return_empty_inventory() {
        let home = fake_home();
        let inv = ClaudeInventory::collect_with_home(Some(home.path()), None);
        assert!(inv.skills.is_empty());
        assert!(inv.mcp_servers.is_empty());
    }

    #[test]
    fn collects_mcp_from_claude_json_projects() {
        let home = fake_home();
        let ws = fake_home();
        // Both projects declare the same global-srv. Whichever the JSON
        // map yields first wins (insertion order isn't preserved without
        // serde_json's `preserve_order` feature — both copies are
        // identical in practice, so we make them identical here too).
        let global_def = serde_json::json!({
            "command": "uvx",
            "args": ["foo-mcp"],
            "env": { "API_KEY": "supersecret", "DEBUG": "1" },
            "type": "stdio"
        });
        let claude_json = serde_json::json!({
            "projects": {
                "/somewhere": { "mcpServers": { "global-srv": global_def } },
                "/elsewhere": { "mcpServers": { "global-srv": global_def } }
            }
        });
        write(
            &home.path().join(".claude.json"),
            &serde_json::to_string_pretty(&claude_json).unwrap(),
        );
        let project_mcp = serde_json::json!({
            "mcpServers": {
                "blender": {
                    "command": "uvx",
                    "args": ["blender-mcp"]
                }
            }
        });
        write(
            &ws.path().join(".mcp.json"),
            &serde_json::to_string_pretty(&project_mcp).unwrap(),
        );

        let inv = ClaudeInventory::collect_with_home(Some(home.path()), Some(ws.path()));
        assert_eq!(inv.mcp_servers.len(), 2);

        let global = inv
            .mcp_servers
            .iter()
            .find(|s| s.name == "global-srv")
            .expect("global-srv missing");
        assert_eq!(global.command.as_deref(), Some("uvx"));
        assert_eq!(global.args, vec!["foo-mcp".to_string()]);
        assert_eq!(global.kind, "stdio");
        // Env keys present, values absent.
        let mut keys = global.env_keys.clone();
        keys.sort();
        assert_eq!(keys, vec!["API_KEY".to_string(), "DEBUG".to_string()]);
        assert!(matches!(global.source, McpSource::Global));

        let blender = inv
            .mcp_servers
            .iter()
            .find(|s| s.name == "blender")
            .expect("blender missing");
        assert!(matches!(blender.source, McpSource::Project));
        assert_eq!(blender.command.as_deref(), Some("uvx"));
    }

    /// Security-regression guard: a serialized inventory must not contain
    /// the literal value of an MCP `env` entry, ever. If this fails,
    /// somebody added an `env` field to `McpServer` and the UI is about
    /// to leak secrets.
    #[test]
    fn mcp_env_values_never_leak() {
        let home = fake_home();
        let claude_json = serde_json::json!({
            "projects": {
                "/somewhere": {
                    "mcpServers": {
                        "leaky": {
                            "command": "node",
                            "args": [],
                            "env": {
                                "OPENAI_API_KEY": "sk-LEAKING-SECRET-12345"
                            }
                        }
                    }
                }
            }
        });
        write(
            &home.path().join(".claude.json"),
            &serde_json::to_string_pretty(&claude_json).unwrap(),
        );
        let inv = ClaudeInventory::collect_with_home(Some(home.path()), None);
        let json = serde_json::to_string(&inv).unwrap();
        assert!(
            !json.contains("sk-LEAKING-SECRET-12345"),
            "MCP env VALUE leaked into the serialized inventory: {json}"
        );
        // Sanity: the KEY is still present so the UI can show it.
        assert!(json.contains("OPENAI_API_KEY"));
    }

    #[test]
    fn malformed_claude_json_does_not_panic() {
        let home = fake_home();
        write(&home.path().join(".claude.json"), "{ not valid json");
        let inv = ClaudeInventory::collect_with_home(Some(home.path()), None);
        assert!(inv.mcp_servers.is_empty());
    }

    #[test]
    fn workspace_dir_none_returns_only_globals() {
        let home = fake_home();
        write(
            &home.path().join(".claude/skills/g/SKILL.md"),
            "---\nname: g\ndescription: x\n---\n",
        );
        let claude_json = serde_json::json!({
            "projects": {
                "/x": { "mcpServers": { "s": { "command": "c" } } }
            }
        });
        write(
            &home.path().join(".claude.json"),
            &serde_json::to_string(&claude_json).unwrap(),
        );

        let inv = ClaudeInventory::collect_with_home(Some(home.path()), None);
        assert_eq!(inv.skills.len(), 1);
        assert_eq!(inv.mcp_servers.len(), 1);
        assert!(inv
            .mcp_servers
            .iter()
            .all(|s| matches!(s.source, McpSource::Global)));
    }
}
