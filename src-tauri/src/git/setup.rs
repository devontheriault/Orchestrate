//! Making a folder something Agents can branch from.

use std::path::Path;

use super::{head_commit, stdout};
use crate::error::{Error, Result};

/// Whether Agents can branch from the folder at `path`: it is a repository
/// with a commit at `HEAD`. A folder that isn't a repository, and one that has
/// been `git init`ed but never committed to, both need [`set_up`] first.
pub async fn has_commits(path: &Path) -> bool {
    head_commit(path).await.is_ok()
}

/// What [`set_up`] ignores in a folder with no `.gitignore` of its own:
/// secrets, dependencies and build output — nothing anyone wants in a first
/// commit they never looked at.
const DEFAULT_GITIGNORE: &str = "\
# Secrets
.env
.env.*
!.env.example

# Dependencies
node_modules/
.venv/
venv/
vendor/

# Build output and caches
target/
dist/
build/
out/
__pycache__/
*.pyc
.cache/

# Editors and OS
.DS_Store
Thumbs.db
.idea/
*.swp
*.log
";

/// Make the folder at `path` something Agents can branch from: `git init` it
/// if it isn't a repository, add a `.gitignore` if it has none, and commit what
/// is there, so every Agent's Worktree starts with the user's files. A folder
/// whose files are all ignored still gets a (possibly empty) first commit.
pub async fn set_up(path: &Path) -> Result<()> {
    if stdout(path, &["rev-parse", "--is-inside-work-tree"])
        .await
        .is_err()
    {
        stdout(path, &["init"]).await?;
    }

    let ignore = path.join(".gitignore");
    if !ignore.exists() {
        std::fs::write(&ignore, DEFAULT_GITIGNORE).map_err(|source| Error::Io {
            path: ignore.clone(),
            source,
        })?;
    }

    stdout(path, &["add", "-A"]).await?;

    // git refuses to commit without an identity, which someone new to git may
    // never have set. Theirs is used whenever they have one.
    let mut args: Vec<&str> = Vec::new();
    if stdout(path, &["config", "user.name"]).await.is_err() {
        args.extend(["-c", "user.name=Claude Wrapper"]);
    }
    if stdout(path, &["config", "user.email"]).await.is_err() {
        args.extend(["-c", "user.email=claude-wrapper@localhost"]);
    }
    args.extend(["commit", "--allow-empty", "-m", "Initial commit"]);
    stdout(path, &args).await?;
    Ok(())
}
