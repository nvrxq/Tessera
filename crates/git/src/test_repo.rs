//! Helpers to spin up a real on-disk git repo for tests.

use anyhow::Result;
use git2::{Repository, Signature};
use std::path::Path;
use tempfile::TempDir;

pub struct Fixture {
    pub dir: TempDir,
    #[allow(dead_code)]
    pub repo: Repository,
}

impl Fixture {
    pub fn new() -> Result<Self> {
        let dir = tempfile::tempdir()?;
        let repo = Repository::init(dir.path())?;
        {
            let sig = Signature::now("test", "test@example.com")?;

            let path = dir.path().join("README.md");
            std::fs::write(&path, "hello\n")?;
            let mut index = repo.index()?;
            index.add_path(Path::new("README.md"))?;
            let tree_id = index.write_tree()?;
            let tree = repo.find_tree(tree_id)?;
            repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])?;
            index.write()?;
        }
        Ok(Fixture { dir, repo })
    }
}
