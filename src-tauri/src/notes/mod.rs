//! Notes (ADR 0017): Markdown files in a folder, one file per note, and the
//! folder is the whole store. There is no database and no index of our own, so
//! the user's editor, `grep`, git, Syncthing or an Agent can all read and write
//! the same files.
//!
//! The folder is `~/Notes` unless the user picks another, and that choice is
//! kept on the Host, beside its other state. It is made on first use.
//!
//! Three promises hold for every call here:
//!
//! - **Nothing leaves the folder.** Every path a window or an Agent names is
//!   relative to it and resolved by [`resolve`], which refuses `..`, absolute
//!   paths, hidden names, and symlinks that point outside.
//! - **A write never clobbers.** Each read hands back the file's version, and
//!   a write names the version it was made from. If the file has changed on
//!   disk since, the write doesn't happen and the caller gets the file as it
//!   now is ([`Saved::Conflict`]). Writes go to a temporary file that is then
//!   renamed over the note, so a crash can't leave half a note.
//! - **Every window sees changes live.** The folder is watched, and any change
//!   — ours, another editor's, an Agent's — is announced as `notes-changed`.

pub mod mcp;
mod text;

#[cfg(test)]
mod tests;

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::paths;

type Result<T> = std::result::Result<T, String>;

/// How long the watcher gathers changes before announcing them, so one save
/// (often several filesystem events) is one announcement.
#[cfg(desktop)]
const SETTLE: Duration = Duration::from_millis(150);

/// How much of a note the list reads for its title and snippet.
const HEAD_BYTES: u64 = 16 * 1024;

/// The most hits a search answers with.
const MAX_HITS: usize = 200;

/// A note as the list shows it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NoteSummary {
    /// Relative to the folder, with `/` between parts on every system.
    pub path: String,
    pub title: String,
    pub snippet: String,
    /// Milliseconds since the Unix epoch.
    pub modified: i64,
}

/// Everything in the folder: its notes, newest first, and its subfolders,
/// so one with nothing in it yet still shows.
#[derive(Debug, Serialize, Deserialize)]
pub struct NoteList {
    pub folder: PathBuf,
    pub notes: Vec<NoteSummary>,
    pub folders: Vec<String>,
}

/// One note's whole text, and the version a write of it must name.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Note {
    pub path: String,
    pub content: String,
    pub version: String,
    pub modified: i64,
}

/// How a write came out. A conflict is an outcome, not an error: nothing was
/// written, and the window asks the user which version to keep.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Saved {
    Saved {
        version: String,
        modified: i64,
    },
    /// The note changed on disk since the version the write was made from.
    /// `current` is the note as it is now, or `None` if it is gone.
    Conflict {
        current: Option<Note>,
    },
}

/// How a delete came out. Where the system has no trash for the note (a
/// network drive, a phone), nothing is deleted and the window asks the user
/// whether to delete it for good.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Deleted {
    Trashed,
    Removed,
    NoTrash { reason: String },
}

/// One note a search matched.
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchHit {
    pub path: String,
    pub title: String,
    /// The first matching line, trimmed around the match. Empty when only the
    /// title or path matched.
    pub excerpt: String,
    /// That line's number, from 1; 0 with no excerpt.
    pub line: usize,
    pub modified: i64,
}

/// Where the notes are kept.
#[derive(Debug, Serialize)]
pub struct NotesFolder {
    pub folder: PathBuf,
    /// Where they'd be if the user hadn't picked: `~/Notes`.
    pub default: PathBuf,
}

/// The Host's notes setting, in `notes.json` in its state directory.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Settings {
    #[serde(default)]
    folder: Option<PathBuf>,
}

fn settings_path() -> Result<PathBuf> {
    Ok(paths::state_dir().map_err(err)?.join("notes.json"))
}

fn load_settings() -> Settings {
    settings_path()
        .ok()
        .and_then(|p| fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn default_folder() -> Result<PathBuf> {
    dirs::home_dir()
        .map(|h| h.join("Notes"))
        .ok_or_else(|| "no home directory to keep notes in".to_string())
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn millis(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Whether a file is a note, by its extension.
fn is_note(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
}

/// A name the folder keeps to itself: `.git`, `.obsidian`, our own temporary
/// files. Never listed, watched for or reachable.
fn hidden(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

/// `rel`, checked: only plain names, none hidden, nothing absolute or `..`.
fn checked(rel: &str) -> Result<PathBuf> {
    let rel = rel.trim();
    if rel.is_empty() {
        return Err("no note named".into());
    }
    let path = Path::new(rel);
    for part in path.components() {
        match part {
            Component::Normal(name) if !hidden(name) => {}
            Component::Normal(_) => return Err(format!("{rel}: hidden files aren't notes")),
            _ => return Err(format!("{rel} is outside the notes folder")),
        }
    }
    Ok(path.to_path_buf())
}

/// `rel` inside `root`, which must be canonical. Refuses anything that would
/// land outside it, including through a symlink: the deepest part of the path
/// that exists has to resolve inside the folder.
fn resolve(root: &Path, rel: &str) -> Result<PathBuf> {
    let joined = root.join(checked(rel)?);
    let mut probe = joined.as_path();
    loop {
        if probe.symlink_metadata().is_ok() {
            let real = probe.canonicalize().map_err(|e| format!("{rel}: {e}"))?;
            if !real.starts_with(root) {
                return Err(format!("{rel} is outside the notes folder"));
            }
            return Ok(joined);
        }
        match probe.parent() {
            Some(up) if up.starts_with(root) => probe = up,
            _ => return Err(format!("{rel} is outside the notes folder")),
        }
    }
}

/// `path` relative to `root`, `/`-separated whatever the system.
fn relative(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let parts: Vec<_> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Write a note all at once, keeping its permissions: a reader sees the old
/// note or the new one, never half of either.
fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    crate::storage::write_whole(path, bytes, false)
}

/// Tell every window which notes changed, by path relative to the folder.
pub type Announce = Box<dyn Fn(Vec<String>) + Send + Sync>;

/// The Host's notes: where the folder is, what is known about the files in it,
/// and the watcher on it.
pub struct Notes {
    announce: Arc<Announce>,
    state: Mutex<State>,
    /// Held across a write's check and its rename, so two writes from this
    /// Host can't both pass the check against the same version.
    writing: Mutex<()>,
}

#[derive(Default)]
struct State {
    /// The folder as last resolved, canonical. Watched while it's set.
    root: Option<PathBuf>,
    #[cfg(desktop)]
    watcher: Option<notify::RecommendedWatcher>,
    /// What the list last read of each note, kept while its modified time and
    /// size stand, so listing a few thousand notes reads only those that moved.
    seen: HashMap<PathBuf, (SystemTime, u64, NoteSummary)>,
}

impl Notes {
    pub fn new(announce: impl Fn(Vec<String>) + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            announce: Arc::new(Box::new(announce)),
            state: Mutex::new(State::default()),
            writing: Mutex::new(()),
        })
    }

    /// The folder, made if it doesn't exist yet, and watched from here on.
    fn root(&self) -> Result<PathBuf> {
        let folder = match load_settings().folder {
            Some(f) => f,
            None => default_folder()?,
        };
        fs::create_dir_all(&folder)
            .map_err(|e| format!("could not make the notes folder {}: {e}", folder.display()))?;
        let root = folder
            .canonicalize()
            .map_err(|e| format!("could not open the notes folder {}: {e}", folder.display()))?;
        let mut state = self.state.lock().map_err(err)?;
        if state.root.as_deref() != Some(&root) {
            state.seen.clear();
            #[cfg(desktop)]
            {
                state.watcher = watch(&root, self.announce.clone())
                    .map_err(|e| eprintln!("notes: not watching {}: {e}", root.display()))
                    .ok();
            }
            state.root = Some(root.clone());
        }
        Ok(root)
    }

    pub fn folder(&self) -> Result<NotesFolder> {
        Ok(NotesFolder {
            folder: self.root()?,
            default: default_folder()?,
        })
    }

    /// Keep notes in `folder` from now on, or with `None` in `~/Notes`. The
    /// notes already written stay where they are.
    pub fn set_folder(&self, folder: Option<PathBuf>) -> Result<NotesFolder> {
        let folder = match folder {
            Some(f) => {
                let f = expand_home(&f)?;
                if !f.is_absolute() {
                    return Err(format!("{} isn't a full path", f.display()));
                }
                fs::create_dir_all(&f)
                    .map_err(|e| format!("could not make {}: {e}", f.display()))?;
                Some(f)
            }
            None => None,
        };
        let path = settings_path()?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(err)?;
        }
        let json = serde_json::to_vec_pretty(&Settings { folder }).map_err(err)?;
        write_atomically(&path, &json).map_err(err)?;
        let folder = self.folder()?;
        (self.announce)(Vec::new());
        Ok(folder)
    }

    /// Every note in the folder, newest first, and every subfolder.
    pub fn list(&self) -> Result<NoteList> {
        let root = self.root()?;
        let mut files = Vec::new();
        let mut folders = Vec::new();
        walk(&root, &root, &mut files, &mut folders);

        let mut state = self.state.lock().map_err(err)?;
        let mut notes = Vec::with_capacity(files.len());
        let mut seen = HashMap::with_capacity(files.len());
        for (path, modified, size) in files {
            let summary = match state.seen.remove(&path) {
                Some((m, s, summary)) if m == modified && s == size => summary,
                _ => {
                    let Some(rel) = relative(&root, &path) else {
                        continue;
                    };
                    let head = read_head(&path).unwrap_or_default();
                    let (title, snippet) = text::title_and_snippet(&head, &stem(&path));
                    NoteSummary {
                        path: rel,
                        title,
                        snippet,
                        modified: millis(modified),
                    }
                }
            };
            notes.push(summary.clone());
            seen.insert(path, (modified, size, summary));
        }
        state.seen = seen;
        drop(state);

        notes.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.path.cmp(&b.path)));
        folders.sort();
        Ok(NoteList {
            folder: root,
            notes,
            folders,
        })
    }

    pub fn read(&self, rel: &str) -> Result<Note> {
        let root = self.root()?;
        let path = note_path(&root, rel)?;
        read_note(&root, &path).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => format!("{rel} isn't in the notes folder"),
            _ => format!("could not read {rel}: {e}"),
        })
    }

    /// Write a note's whole text, if it is still at `version` on disk — the
    /// version it was read at. `None` means a new note, written only if no
    /// file has that name yet.
    pub fn write(&self, rel: &str, content: &str, version: Option<&str>) -> Result<Saved> {
        let root = self.root()?;
        let path = note_path(&root, rel)?;
        let _writing = self.writing.lock().map_err(err)?;
        match fs::read(&path) {
            Ok(bytes) => {
                if version != Some(text::version(&bytes).as_str()) {
                    return Ok(Saved::Conflict {
                        current: Some(note_from(&root, &path, &bytes)?),
                    });
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                if version.is_some() {
                    return Ok(Saved::Conflict { current: None });
                }
            }
            Err(e) => return Err(format!("could not read {rel}: {e}")),
        }
        write_atomically(&path, content.as_bytes())
            .map_err(|e| format!("could not save {rel}: {e}"))?;
        let modified = fs::metadata(&path)
            .and_then(|m| m.modified())
            .map(millis)
            .unwrap_or(0);
        Ok(Saved::Saved {
            version: text::version(content.as_bytes()),
            modified,
        })
    }

    /// Start a note in `dir` (the folder itself if `None`), named after `name`
    /// or "Untitled", with a number added if that name is taken.
    pub fn create(&self, dir: Option<&str>, name: Option<&str>, content: &str) -> Result<Note> {
        let root = self.root()?;
        let dir = match dir.map(str::trim).filter(|d| !d.is_empty()) {
            Some(d) => resolve(&root, d)?,
            None => root.clone(),
        };
        fs::create_dir_all(&dir).map_err(err)?;
        let stem = text::file_stem(name.unwrap_or(""));
        for n in 1.. {
            let file = if n == 1 {
                format!("{stem}.md")
            } else {
                format!("{stem} {n}.md")
            };
            let path = dir.join(file);
            // `create_new`, so a file made between looking and writing is
            // never overwritten: the next number is tried instead.
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut f) => {
                    f.write_all(content.as_bytes()).map_err(err)?;
                    return read_note(&root, &path).map_err(err);
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("could not make a note: {e}")),
            }
        }
        unreachable!()
    }

    /// Rename or move a note or a folder. Never onto something that exists.
    /// Answers with where it went.
    pub fn rename(&self, from: &str, to: &str) -> Result<String> {
        let root = self.root()?;
        let source = resolve(&root, from)?;
        let meta = source
            .symlink_metadata()
            .map_err(|_| format!("{from} isn't in the notes folder"))?;
        let mut target = resolve(&root, to)?;
        // Appended rather than set: "v1.2 plan" is a name, not a ".2 plan" file.
        if meta.is_file() && !is_note(&target) {
            let mut name = target.file_name().unwrap_or_default().to_os_string();
            name.push(".md");
            target.set_file_name(name);
        }
        if target == source {
            return Ok(relative(&root, &target).unwrap_or_default());
        }
        if target.starts_with(&source) {
            return Err(format!("can't move {from} inside itself"));
        }
        let _writing = self.writing.lock().map_err(err)?;
        if target.symlink_metadata().is_ok() {
            let name = relative(&root, &target).unwrap_or_default();
            return Err(format!("there's already a {name}"));
        }
        if let Some(dir) = target.parent() {
            fs::create_dir_all(dir).map_err(err)?;
        }
        fs::rename(&source, &target).map_err(|e| format!("could not move {from}: {e}"))?;
        Ok(relative(&root, &target).unwrap_or_default())
    }

    /// Delete a note, or an empty folder: to the system's trash, or with
    /// `permanent` for good, which the window only asks for once the user has
    /// confirmed it, after the trash couldn't take it.
    pub fn delete(&self, rel: &str, permanent: bool) -> Result<Deleted> {
        let root = self.root()?;
        let path = resolve(&root, rel)?;
        let meta = path
            .symlink_metadata()
            .map_err(|_| format!("{rel} isn't in the notes folder"))?;
        if meta.is_dir() {
            let empty = fs::read_dir(&path).map_err(err)?.next().is_none();
            if !empty {
                return Err(format!("{rel} still has notes in it"));
            }
        } else if !is_note(&path) {
            return Err(format!("{rel} isn't a note"));
        }
        if permanent {
            let removed = if meta.is_dir() {
                fs::remove_dir(&path)
            } else {
                fs::remove_file(&path)
            };
            removed.map_err(|e| format!("could not delete {rel}: {e}"))?;
            return Ok(Deleted::Removed);
        }
        Ok(match to_trash(&path) {
            Ok(()) => Deleted::Trashed,
            Err(reason) => Deleted::NoTrash { reason },
        })
    }

    /// The notes holding every word of `query`, best first.
    pub fn search(&self, query: &str) -> Result<Vec<SearchHit>> {
        let terms = text::terms(query);
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let list = self.list()?;
        let root = list.folder;
        let mut hits: Vec<(u32, SearchHit)> = list
            .notes
            .into_iter()
            .filter_map(|n| {
                let bytes = fs::read(root.join(&n.path)).ok()?;
                let content = String::from_utf8_lossy(&bytes);
                let score = text::score(&terms, &n.title, &n.path, &content)?;
                let (line, excerpt) = text::excerpt(&terms, &n.title, &content).unwrap_or_default();
                Some((
                    score,
                    SearchHit {
                        path: n.path,
                        title: n.title,
                        excerpt,
                        line,
                        modified: n.modified,
                    },
                ))
            })
            .collect();
        hits.sort_by(|(a, x), (b, y)| b.cmp(a).then(y.modified.cmp(&x.modified)));
        hits.truncate(MAX_HITS);
        Ok(hits.into_iter().map(|(_, h)| h).collect())
    }
}

/// `rel` as a note inside `root`: a Markdown file, by its extension.
fn note_path(root: &Path, rel: &str) -> Result<PathBuf> {
    let path = resolve(root, rel)?;
    if !is_note(&path) {
        return Err(format!("{rel} isn't a note: notes end in .md"));
    }
    Ok(path)
}

fn read_note(root: &Path, path: &Path) -> io::Result<Note> {
    let bytes = fs::read(path)?;
    note_from(root, path, &bytes).map_err(io::Error::other)
}

fn note_from(root: &Path, path: &Path, bytes: &[u8]) -> Result<Note> {
    let modified = fs::metadata(path)
        .and_then(|m| m.modified())
        .map(millis)
        .unwrap_or(0);
    Ok(Note {
        path: relative(root, path).ok_or("not in the notes folder")?,
        content: String::from_utf8_lossy(bytes).into_owned(),
        version: text::version(bytes),
        modified,
    })
}

/// The start of a note, enough for its title and snippet.
fn read_head(path: &Path) -> io::Result<String> {
    let mut buf = Vec::new();
    fs::File::open(path)?
        .take(HEAD_BYTES)
        .read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Gather every note under `dir`, with its modified time and size, and every
/// folder. Hidden names are skipped, and so are symlinked folders, which could
/// loop or lead out; a symlinked note counts if it resolves inside `root`.
fn walk(
    root: &Path,
    dir: &Path,
    files: &mut Vec<(PathBuf, SystemTime, u64)>,
    folders: &mut Vec<String>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if hidden(&entry.file_name()) {
            continue;
        }
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if let Some(rel) = relative(root, &path) {
                folders.push(rel);
            }
            walk(root, &path, files, folders);
            continue;
        }
        if !is_note(&path) {
            continue;
        }
        if kind.is_symlink()
            && !path
                .canonicalize()
                .is_ok_and(|real| real.starts_with(root) && real.is_file())
        {
            continue;
        }
        if let Ok(meta) = fs::metadata(&path) {
            let modified = meta.modified().unwrap_or(UNIX_EPOCH);
            files.push((path, modified, meta.len()));
        }
    }
}

/// `~/x` as the home directory's `x`.
fn expand_home(path: &Path) -> Result<PathBuf> {
    match path.strip_prefix("~") {
        Ok(rest) => Ok(dirs::home_dir().ok_or("no home directory")?.join(rest)),
        Err(_) => Ok(path.to_path_buf()),
    }
}

#[cfg(desktop)]
fn to_trash(path: &Path) -> std::result::Result<(), String> {
    trash::delete(path).map_err(err)
}

#[cfg(mobile)]
fn to_trash(_: &Path) -> std::result::Result<(), String> {
    Err("this device has no trash".into())
}

/// Watch `root` and everything under it, announcing what changed once each
/// burst of changes settles. Lives as long as the returned watcher.
#[cfg(desktop)]
fn watch(root: &Path, announce: Arc<Announce>) -> notify::Result<notify::RecommendedWatcher> {
    use notify::{EventKind, RecursiveMode, Watcher};
    use std::sync::mpsc;

    let (tx, rx) = mpsc::channel::<PathBuf>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        // Opening and reading a file changes nothing anyone can see.
        if matches!(event.kind, EventKind::Access(a) if a != notify::event::AccessKind::Close(notify::event::AccessMode::Write))
        {
            return;
        }
        for path in event.paths {
            let _ = tx.send(path);
        }
    })?;
    watcher.watch(root, RecursiveMode::Recursive)?;

    let root = root.to_path_buf();
    std::thread::Builder::new()
        .name("notes-watch".into())
        .spawn(move || {
            // Ends when the watcher, and the sender inside it, are dropped.
            while let Ok(first) = rx.recv() {
                let mut changed = BTreeSet::new();
                let mut take = |p: PathBuf| {
                    let Some(rel) = p.strip_prefix(&root).ok() else {
                        return;
                    };
                    if rel.components().any(|c| hidden(c.as_os_str())) {
                        return;
                    }
                    if let Some(rel) = relative(&root, &p) {
                        changed.insert(rel);
                    }
                };
                take(first);
                std::thread::sleep(SETTLE);
                while let Ok(p) = rx.try_recv() {
                    take(p);
                }
                if !changed.is_empty() {
                    announce(changed.into_iter().collect());
                }
            }
        })
        .map_err(|e| notify::Error::generic(&e.to_string()))?;
    Ok(watcher)
}
