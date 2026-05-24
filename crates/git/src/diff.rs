use anyhow::Result;
use git2::{DiffOptions, Repository};
use serde::Serialize;
use std::cell::RefCell;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
pub struct FileDiff {
    pub path: PathBuf,
    pub status: String, // "added" | "modified" | "deleted" | "renamed"
    pub patch: String,  // unified diff text
}

/// Diff `worktree_path` (HEAD + uncommitted) against the tip of `base_branch` in `repo_path`.
pub fn diff_against_base(
    repo_path: &Path,
    worktree_path: &Path,
    base_branch: &str,
) -> Result<Vec<FileDiff>> {
    let repo = Repository::open(repo_path)?;
    let base_ref = repo.find_branch(base_branch, git2::BranchType::Local)?;
    let base_commit = base_ref.get().peel_to_commit()?;
    let base_tree = base_commit.tree()?;

    // Use the worktree as the comparison target.
    let wt_repo = Repository::open(worktree_path)?;
    let head_tree = wt_repo.head()?.peel_to_tree()?;

    let mut opts = DiffOptions::new();
    opts.include_untracked(true).recurse_untracked_dirs(true);

    let diff = wt_repo.diff_tree_to_workdir_with_index(Some(&head_tree), Some(&mut opts))?;
    let base_to_head = repo.diff_tree_to_tree(Some(&base_tree), Some(&head_tree), None)?;

    let out: RefCell<Vec<FileDiff>> = RefCell::new(Vec::new());

    let collect = |d: git2::Diff| -> Result<()> {
        d.foreach(
            &mut |delta, _| {
                let path = delta
                    .new_file()
                    .path()
                    .or_else(|| delta.old_file().path())
                    .map(PathBuf::from)
                    .unwrap_or_default();
                out.borrow_mut().push(FileDiff {
                    path,
                    status: status_to_str(delta.status()).into(),
                    patch: String::new(),
                });
                true
            },
            None,
            None,
            Some(&mut |_, _, line| {
                let mut v = out.borrow_mut();
                if let Some(last) = v.last_mut() {
                    let origin = line.origin();
                    if matches!(origin, '+' | '-' | ' ') {
                        last.patch.push(origin);
                    }
                    last.patch.push_str(&String::from_utf8_lossy(line.content()));
                }
                true
            }),
        )?;
        Ok(())
    };

    collect(base_to_head)?;
    collect(diff)?;
    Ok(out.into_inner())
}

fn status_to_str(s: git2::Delta) -> &'static str {
    match s {
        git2::Delta::Added => "added",
        git2::Delta::Modified => "modified",
        git2::Delta::Deleted => "deleted",
        git2::Delta::Renamed => "renamed",
        git2::Delta::Copied => "copied",
        git2::Delta::Untracked => "added",
        _ => "modified",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_repo::Fixture;
    use crate::worktree;

    #[test]
    fn diff_reports_modified_file() {
        let fx = Fixture::new().unwrap();
        let base = fx.repo.head().unwrap().shorthand().unwrap().to_string();
        let wt = fx.dir.path().join("wt");
        worktree::create(fx.dir.path(), &wt, "feat/x").unwrap();

        // edit and commit inside the worktree
        std::fs::write(wt.join("README.md"), "hello\nworld\n").unwrap();
        let wt_repo = git2::Repository::open(&wt).unwrap();
        let sig = git2::Signature::now("t", "t@x").unwrap();
        let mut index = wt_repo.index().unwrap();
        index.add_path(std::path::Path::new("README.md")).unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = wt_repo.find_tree(tree_id).unwrap();
        let parent = wt_repo.head().unwrap().peel_to_commit().unwrap();
        wt_repo
            .commit(Some("HEAD"), &sig, &sig, "edit", &tree, &[&parent])
            .unwrap();

        let diffs = diff_against_base(fx.dir.path(), &wt, &base).unwrap();
        assert!(diffs.iter().any(|d| d.path == std::path::PathBuf::from("README.md")));
    }
}
