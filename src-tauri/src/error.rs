use std::io;
use std::path::PathBuf;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("i/o error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("no state directory available (no $HOME?)")]
    NoStateDir,

    #[error("git `{command}` failed: {stderr}")]
    Git { command: String, stderr: String },

    #[error("{path} is not a Git repository with a commit yet; set it up before adding it")]
    NotSetUp { path: PathBuf },

    #[error("cannot attach {path}: it is not a file, or no longer exists")]
    AttachmentMissing { path: PathBuf },

    #[error("could not read the clipboard: {0}")]
    Clipboard(String),

    #[error("agent not found: {0}")]
    AgentNotFound(String),

    #[error("agent {0} is still working; wait for it to finish or stop it first")]
    AgentBusy(String),

    #[error("cannot continue agent {id}: {why}")]
    NotResumable { id: String, why: String },

    #[error("worktree is gone at {path} (already reaped?)")]
    WorktreeMissing { path: PathBuf },

    #[error(
        "cannot tell which commit `{branch}` was cut from: its project is no longer registered"
    )]
    NoBaseCommit { branch: String },

    #[error("nothing to commit in {path}")]
    NothingToCommit { path: PathBuf },

    #[error("commit message is empty")]
    EmptyCommitMessage,

    #[error("{path} has uncommitted work; commit it before merging")]
    WorktreeDirty { path: PathBuf },

    #[error("the project has uncommitted changes on {branch}; commit or stash them first")]
    ProjectDirty { branch: String },

    #[error("nothing to merge: `{branch}` is already in `{target}`")]
    NothingToMerge { branch: String, target: String },

    #[error(
        "merging into `{target}` hit conflicts in {}; the project is unchanged",
        files.join(", ")
    )]
    MergeConflict { target: String, files: Vec<String> },

    #[error("the conflicts with `{target}` are not resolved yet: {why}")]
    Unresolved { target: String, why: String },

    #[error("not signed in to Claude — run `claude` once, or set ANTHROPIC_API_KEY")]
    NoCredential,

    #[error("could not ask Anthropic which models are available: {0}")]
    Models(String),
}

pub type Result<T> = std::result::Result<T, Error>;
