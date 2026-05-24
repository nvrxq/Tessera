use anyhow::Result;
use git2::{Repository, WorktreeAddOptions};
use std::path::{Path, PathBuf};

/// Create a new worktree at `worktree_path` for `branch_name`, branching from HEAD of `repo_path`.
pub fn create(repo_path: &Path, worktree_path: &Path, branch_name: &str) -> Result<PathBuf> {
    let repo = Repository::open(repo_path)?;
    let head_commit = repo.head()?.peel_to_commit()?;
    let branch = repo.branch(branch_name, &head_commit, false)?;
    let reference = branch.into_reference();

    let mut opts = WorktreeAddOptions::new();
    opts.reference(Some(&reference));

    let worktree_name = worktree_path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("worktree path has no file name"))?;

    let wt = repo.worktree(worktree_name, worktree_path, Some(&opts))?;
    Ok(wt.path().to_path_buf())
}

/// Remove a worktree by `worktree_path`, regardless of state. Caller is responsible for asking
/// the user about dirty-state.
pub fn delete(repo_path: &Path, worktree_path: &Path, force: bool) -> Result<()> {
    let repo = Repository::open(repo_path)?;
    let worktree_name = worktree_path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("worktree path has no file name"))?;
    let wt = repo.find_worktree(worktree_name)?;

    let mut prune_opts = git2::WorktreePruneOptions::new();
    prune_opts.working_tree(true).valid(force).locked(force);
    wt.prune(Some(&mut prune_opts))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_repo::Fixture;

    #[test]
    fn create_makes_worktree_directory() {
        let fx = Fixture::new().unwrap();
        let wt = fx.dir.path().join("wt-feat");
        let result = create(fx.dir.path(), &wt, "feat/x").unwrap();
        assert!(result.exists());
        assert!(wt.join("README.md").exists());
    }

    #[test]
    fn delete_removes_worktree() {
        let fx = Fixture::new().unwrap();
        let wt = fx.dir.path().join("wt-feat");
        create(fx.dir.path(), &wt, "feat/x").unwrap();
        delete(fx.dir.path(), &wt, true).unwrap();
        assert!(!wt.exists());
    }

    #[test]
    fn create_with_existing_branch_name_errors() {
        let fx = Fixture::new().unwrap();
        let wt1 = fx.dir.path().join("wt1");
        create(fx.dir.path(), &wt1, "dupe").unwrap();
        let wt2 = fx.dir.path().join("wt2");
        let err = create(fx.dir.path(), &wt2, "dupe").unwrap_err();
        let s = format!("{err}");
        assert!(s.contains("dupe") || s.contains("reference"), "got: {s}");
    }
}
