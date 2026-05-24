//! Parse `git worktree add <path>` invocations out of a Bash tool command line.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeAdd {
    pub path: String,
    pub branch: Option<String>,
}

/// Return `Some(WorktreeAdd)` if the command (or any `;` / `&&` / `|` segment
/// of it) is a `git worktree add ...` invocation, otherwise `None`.
/// Best-effort — handles the common forms used in Claude Code sessions.
pub fn parse_worktree_add(command: &str) -> Option<WorktreeAdd> {
    for segment in split_compound(command) {
        if let Some(parsed) = parse_single(segment) {
            return Some(parsed);
        }
    }
    None
}

fn split_compound(command: &str) -> Vec<&str> {
    command
        .split([';', '|'])
        .flat_map(|s| s.split("&&"))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

fn parse_single(segment: &str) -> Option<WorktreeAdd> {
    let tokens = shell_words::split(segment).ok()?;
    let mut it = tokens.iter();
    if it.next()? != "git" {
        return None;
    }
    if it.next()? != "worktree" {
        return None;
    }
    if it.next()? != "add" {
        return None;
    }

    let mut branch: Option<String> = None;
    let mut positionals: Vec<String> = Vec::new();
    while let Some(t) = it.next() {
        match t.as_str() {
            "-b" | "-B" | "--branch" => {
                if let Some(b) = it.next() {
                    branch = Some(b.clone());
                }
            }
            "--detach" | "--force" | "--lock" | "--guess-remote" | "--no-checkout" | "--quiet" => {
                // flags with no argument
            }
            s if s.starts_with("--") => {
                // unknown long flag, possibly with embedded value — skip just this token
            }
            _ => positionals.push(t.clone()),
        }
    }

    let path = positionals.into_iter().next()?;
    Some(WorktreeAdd { path, branch })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_path() {
        assert_eq!(
            parse_worktree_add("git worktree add ../wt"),
            Some(WorktreeAdd {
                path: "../wt".into(),
                branch: None
            })
        );
    }

    #[test]
    fn dash_b_then_path() {
        assert_eq!(
            parse_worktree_add("git worktree add -b feat/x ../wt"),
            Some(WorktreeAdd {
                path: "../wt".into(),
                branch: Some("feat/x".into())
            })
        );
    }

    #[test]
    fn path_then_dash_b() {
        assert_eq!(
            parse_worktree_add("git worktree add ../wt -b feat/x"),
            Some(WorktreeAdd {
                path: "../wt".into(),
                branch: Some("feat/x".into())
            })
        );
    }

    #[test]
    fn quoted_path_with_space() {
        assert_eq!(
            parse_worktree_add(r#"git worktree add "../my space" -b feat/x"#),
            Some(WorktreeAdd {
                path: "../my space".into(),
                branch: Some("feat/x".into())
            })
        );
    }

    #[test]
    fn compound_command_finds_worktree_segment() {
        assert_eq!(
            parse_worktree_add("cd /tmp && git worktree add ../wt -b feat/x && ls"),
            Some(WorktreeAdd {
                path: "../wt".into(),
                branch: Some("feat/x".into())
            })
        );
    }

    #[test]
    fn ignores_unrelated_commands() {
        assert_eq!(parse_worktree_add("git status"), None);
        assert_eq!(parse_worktree_add("git worktree list"), None);
        assert_eq!(parse_worktree_add("ls"), None);
    }
}
