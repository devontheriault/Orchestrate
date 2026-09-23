//! Files the user hands an Agent alongside a prompt — dropped on the window,
//! picked from a dialog, or pasted from the clipboard.
//!
//! An Attachment is a path, not a copy: `claude` reads it with its own tools,
//! which already understand code, images and PDFs, so all we do is name the
//! files in the prompt and let the Turn read outside its Worktree. Pasted
//! images have no path of their own, so they are written to the state
//! directory first and attached from there.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::domain::new_id;
use crate::error::{Error, Result};
use crate::paths;

/// The prompt `claude` is actually handed: the user's text with the attached
/// files listed under it by absolute path, so the Agent knows to read them.
pub fn for_claude(prompt: &str, attachments: &[PathBuf]) -> String {
    if attachments.is_empty() {
        return prompt.to_owned();
    }
    let mut out = prompt.trim_end().to_owned();
    out.push_str("\n\nAttached files (read them with your tools):");
    for path in attachments {
        out.push_str("\n- ");
        out.push_str(&path.display().to_string());
    }
    out
}

/// The directories to hand `claude --add-dir` so it may read the attachments.
/// A YOLO Turn could read them anyway; a plan-mode Turn is refused anything
/// outside its working directories, with no one to ask.
pub fn dirs(attachments: &[PathBuf]) -> Vec<PathBuf> {
    attachments
        .iter()
        .filter_map(|p| p.parent().map(Path::to_path_buf))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Refuse a Turn whose attachments aren't there to read — a file moved since it
/// was queued, say — so the user hears it now rather than from the Agent.
pub fn check(attachments: &[PathBuf]) -> Result<()> {
    for path in attachments {
        if !path.is_absolute() || !path.is_file() {
            return Err(Error::AttachmentMissing { path: path.clone() });
        }
    }
    Ok(())
}

/// The extensions the composer and transcript draw a thumbnail for, and so the
/// only files [`preview`] will read. The webview decides whether it can
/// actually draw them; one it can't (HEIC, often) falls back to an icon.
const PREVIEWABLE: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "avif", "heic",
];

/// An attached image's bytes, for its thumbnail. Anything that isn't an image
/// by its extension is refused, so this can't be used to read arbitrary files.
pub fn preview(path: &Path) -> Result<Vec<u8>> {
    let is_image = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| PREVIEWABLE.contains(&e.to_ascii_lowercase().as_str()));
    if !is_image || !path.is_absolute() || !path.is_file() {
        return Err(Error::AttachmentMissing {
            path: path.to_owned(),
        });
    }
    std::fs::read(path).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })
}

/// Write pasted bytes to the state directory and return where they landed.
/// Each paste gets its own folder, so the file keeps the name it was pasted
/// with — which is what the Agent and the transcript will call it.
pub fn save(name: &str, bytes: &[u8]) -> Result<PathBuf> {
    let dir = paths::attachments_dir()?.join(new_id());
    std::fs::create_dir_all(&dir).map_err(|source| Error::Io {
        path: dir.clone(),
        source,
    })?;
    let path = dir.join(file_name(name));
    std::fs::write(&path, bytes).map_err(|source| Error::Io {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

/// A pasted file's name, cut down to its last component so it can't climb out
/// of its folder. Clipboard images often come unnamed.
fn file_name(name: &str) -> String {
    Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.trim().is_empty())
        .unwrap_or("pasted")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::StateEnv;

    #[test]
    fn no_attachments_leaves_the_prompt_alone() {
        assert_eq!(for_claude("fix it\n", &[]), "fix it\n");
    }

    #[test]
    fn attachments_are_listed_under_the_prompt() {
        let prompt = for_claude(
            "fix this\n",
            &["/tmp/shot.png".into(), "/home/me/notes.md".into()],
        );
        assert_eq!(
            prompt,
            "fix this\n\nAttached files (read them with your tools):\n- /tmp/shot.png\n- /home/me/notes.md"
        );
    }

    #[test]
    fn dirs_are_the_distinct_parents() {
        let dirs = dirs(&["/a/x".into(), "/b/y".into(), "/a/z".into()]);
        assert_eq!(dirs, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
    }

    #[test]
    fn a_missing_or_relative_attachment_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let there = dir.path().join("there.txt");
        std::fs::write(&there, "hi").unwrap();
        assert!(check(&[there]).is_ok());
        assert!(check(&[dir.path().join("gone.txt")]).is_err());
        assert!(check(&["there.txt".into()]).is_err());
        assert!(
            check(&[dir.path().to_path_buf()]).is_err(),
            "a folder is not a file"
        );
    }

    #[test]
    fn only_images_are_previewed() {
        let dir = tempfile::tempdir().unwrap();
        let shot = dir.path().join("shot.PNG");
        std::fs::write(&shot, b"png").unwrap();
        let notes = dir.path().join("notes.md");
        std::fs::write(&notes, "secret").unwrap();
        assert_eq!(preview(&shot).unwrap(), b"png");
        assert!(preview(&notes).is_err());
        assert!(preview(&dir.path().join("gone.png")).is_err());
    }

    #[test]
    fn a_paste_is_saved_under_its_own_name() {
        let _env = StateEnv::new();
        let path = save("../../escape.png", b"png").unwrap();
        assert_eq!(path.file_name().unwrap(), "escape.png");
        assert!(path.starts_with(paths::attachments_dir().unwrap()));
        assert_eq!(std::fs::read(&path).unwrap(), b"png");

        let unnamed = save("", b"x").unwrap();
        assert_eq!(unnamed.file_name().unwrap(), "pasted");
        assert_ne!(
            unnamed.parent(),
            path.parent(),
            "each paste has its own folder"
        );
    }
}
