use std::fs;
use std::sync::mpsc;
use std::time::Duration;

use tempfile::TempDir;

use super::text::{excerpt, file_stem, score, terms, title_and_snippet, version};
use super::*;
use crate::test_util::StateEnv;

/// A Host's notes kept in a fresh folder, and what it announced.
struct Folder {
    _env: StateEnv,
    dir: TempDir,
    notes: Arc<Notes>,
    announced: mpsc::Receiver<Vec<String>>,
}

fn folder() -> Folder {
    let env = StateEnv::new();
    let dir = TempDir::new().unwrap();
    let (tx, announced) = mpsc::channel();
    let tx = Mutex::new(tx);
    let notes = Notes::new(move |paths| {
        let _ = tx.lock().unwrap().send(paths);
    });
    notes.set_folder(Some(dir.path().to_path_buf())).unwrap();
    // The setting itself is announced; start from quiet.
    while announced.try_recv().is_ok() {}
    Folder {
        _env: env,
        dir,
        notes,
        announced,
    }
}

impl Folder {
    fn put(&self, rel: &str, text: &str) {
        let path = self.dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn get(&self, rel: &str) -> String {
        fs::read_to_string(self.dir.path().join(rel)).unwrap()
    }
}

// ------------------------------------------------------------------ text

#[test]
fn the_title_is_the_first_heading() {
    let (title, snippet) = title_and_snippet("# Groceries\n\n- eggs\n- **milk**\n", "file");
    assert_eq!(title, "Groceries");
    assert_eq!(snippet, "eggs milk");
}

#[test]
fn without_a_heading_the_title_is_the_file_name() {
    let (title, snippet) = title_and_snippet("just some words\nand more", "Thoughts");
    assert_eq!(title, "Thoughts");
    assert_eq!(snippet, "just some words and more");
}

#[test]
fn a_later_h1_still_titles_the_note() {
    let (title, _) = title_and_snippet("intro line\n\n# Real title\nbody", "f");
    assert_eq!(title, "Real title");
    // But a later ## is a section, not the note's name.
    let (title, _) = title_and_snippet("intro line\n\n## Details\nbody", "f");
    assert_eq!(title, "f");
}

#[test]
fn front_matter_is_skipped_and_its_title_used() {
    let text = "---\ntitle: \"From YAML\"\ntags: [a]\n---\nHello there\n";
    assert_eq!(
        title_and_snippet(text, "f"),
        ("From YAML".into(), "Hello there".into())
    );
    // A heading beats it.
    let text = "---\ntitle: From YAML\n---\n# Heading\nHi\n";
    assert_eq!(title_and_snippet(text, "f").0, "Heading");
}

#[test]
fn hashes_without_a_space_are_not_a_heading() {
    let (title, snippet) = title_and_snippet("#hashtag note\n", "f");
    assert_eq!(title, "f");
    assert_eq!(snippet, "#hashtag note");
}

#[test]
fn long_snippets_are_clipped() {
    let long = "word ".repeat(100);
    let (_, snippet) = title_and_snippet(&long, "f");
    assert!(snippet.chars().count() <= 161);
    assert!(snippet.ends_with('…'));
}

#[test]
fn every_term_must_match() {
    let t = terms("Milk  EGGS");
    assert_eq!(t, ["milk", "eggs"]);
    assert!(score(&t, "List", "list.md", "eggs and milk").is_some());
    assert!(score(&t, "List", "list.md", "eggs only").is_none());
    // A title hit outranks many body hits.
    let title = score(&terms("milk"), "Milk", "a.md", "x").unwrap();
    let body = score(&terms("milk"), "Other", "b.md", "milk milk milk").unwrap();
    assert!(title > body);
}

#[test]
fn the_excerpt_is_the_matching_line() {
    let text = "# T\nfirst line\nthe quick brown fox\n";
    assert_eq!(
        excerpt(&terms("BROWN"), "T", text),
        Some((3, "the quick brown fox".into()))
    );
    let long = format!("{}needle{}", "a".repeat(200), "b".repeat(200));
    let (_, ex) = excerpt(&terms("needle"), "", &long).unwrap();
    assert!(ex.starts_with('…') && ex.ends_with('…') && ex.contains("needle"));
}

#[test]
fn file_names_are_made_safe() {
    assert_eq!(file_stem("a/b: c?"), "a b c");
    assert_eq!(file_stem("  ..  "), "Untitled");
    assert_eq!(file_stem("../../etc/passwd"), "etc passwd");
}

#[test]
fn versions_follow_content() {
    assert_eq!(version(b"abc"), version(b"abc"));
    assert_ne!(version(b"abc"), version(b"abd"));
    assert_ne!(version(b""), version(b"\0"));
}

// ------------------------------------------------------------- the folder

#[test]
fn lists_notes_in_subfolders_newest_first() {
    let f = folder();
    f.put("old.md", "# Old\n");
    std::thread::sleep(Duration::from_millis(20));
    f.put("work/new.md", "# New\nfresh");
    f.put("work/empty/.keep", "");
    f.put("readme.txt", "not a note");
    f.put(".obsidian/x.md", "# hidden");

    let list = f.notes.list().unwrap();
    let paths: Vec<_> = list.notes.iter().map(|n| n.path.as_str()).collect();
    assert_eq!(paths, ["work/new.md", "old.md"]);
    assert_eq!(list.notes[0].title, "New");
    assert_eq!(list.notes[0].snippet, "fresh");
    assert_eq!(list.folders, ["work", "work/empty"]);
}

#[test]
fn the_list_notices_edits() {
    let f = folder();
    f.put("a.md", "# One\n");
    assert_eq!(f.notes.list().unwrap().notes[0].title, "One");
    f.put("a.md", "# Two, longer\n");
    assert_eq!(f.notes.list().unwrap().notes[0].title, "Two, longer");
}

#[test]
fn reads_and_writes_a_note() {
    let f = folder();
    f.put("a.md", "hello");
    let note = f.notes.read("a.md").unwrap();
    assert_eq!(note.content, "hello");
    let saved = f
        .notes
        .write("a.md", "hello, world", Some(&note.version))
        .unwrap();
    assert!(matches!(saved, Saved::Saved { .. }));
    assert_eq!(f.get("a.md"), "hello, world");
    // No temporary file left behind.
    let names: Vec<_> = fs::read_dir(f.dir.path()).unwrap().flatten().collect();
    assert_eq!(names.len(), 1);
}

#[test]
fn a_write_never_clobbers_a_change_made_on_disk() {
    let f = folder();
    f.put("a.md", "v1");
    let note = f.notes.read("a.md").unwrap();
    f.put("a.md", "changed in vim");

    let saved = f.notes.write("a.md", "mine", Some(&note.version)).unwrap();
    let Saved::Conflict {
        current: Some(current),
    } = saved
    else {
        panic!("expected a conflict, got {saved:?}");
    };
    assert_eq!(current.content, "changed in vim");
    assert_eq!(f.get("a.md"), "changed in vim");

    // Keeping mine, knowingly: written against the version now on disk.
    let saved = f
        .notes
        .write("a.md", "mine", Some(&current.version))
        .unwrap();
    assert!(matches!(saved, Saved::Saved { .. }));
    assert_eq!(f.get("a.md"), "mine");
}

#[test]
fn a_write_to_a_note_deleted_on_disk_is_a_conflict() {
    let f = folder();
    f.put("a.md", "v1");
    let note = f.notes.read("a.md").unwrap();
    fs::remove_file(f.dir.path().join("a.md")).unwrap();
    let saved = f.notes.write("a.md", "mine", Some(&note.version)).unwrap();
    assert_eq!(saved, Saved::Conflict { current: None });
    // Saving it again as new is allowed.
    assert!(matches!(
        f.notes.write("a.md", "mine", None).unwrap(),
        Saved::Saved { .. }
    ));
}

#[test]
fn a_new_note_never_lands_on_an_existing_file() {
    let f = folder();
    f.put("a.md", "theirs");
    let saved = f.notes.write("a.md", "mine", None).unwrap();
    assert!(matches!(saved, Saved::Conflict { current: Some(_) }));
    assert_eq!(f.get("a.md"), "theirs");
}

#[cfg(unix)]
#[test]
fn a_write_keeps_the_notes_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let f = folder();
    f.put("a.md", "x");
    let path = f.dir.path().join("a.md");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let note = f.notes.read("a.md").unwrap();
    f.notes.write("a.md", "y", Some(&note.version)).unwrap();
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn creates_numbered_untitled_notes() {
    let f = folder();
    let a = f.notes.create(None, None, "").unwrap();
    let b = f.notes.create(None, None, "").unwrap();
    let c = f
        .notes
        .create(Some("ideas"), Some("Big: plan"), "# Big plan\n")
        .unwrap();
    assert_eq!(a.path, "Untitled.md");
    assert_eq!(b.path, "Untitled 2.md");
    assert_eq!(c.path, "ideas/Big plan.md");
    assert_eq!(f.get("ideas/Big plan.md"), "# Big plan\n");
}

#[test]
fn renames_and_moves_without_clobbering() {
    let f = folder();
    f.put("a.md", "a");
    f.put("b.md", "b");
    assert_eq!(
        f.notes.rename("a.md", "archive/v1.2 plan").unwrap(),
        "archive/v1.2 plan.md"
    );
    assert_eq!(f.get("archive/v1.2 plan.md"), "a");
    assert!(f.notes.rename("b.md", "archive/v1.2 plan.md").is_err());
    assert_eq!(f.get("b.md"), "b");
    // Folders move too, but not into themselves.
    assert_eq!(f.notes.rename("archive", "old").unwrap(), "old");
    assert!(f.notes.rename("old", "old/inner").is_err());
}

#[test]
fn deletes_permanently_only_when_asked() {
    let f = folder();
    f.put("a.md", "a");
    assert_eq!(f.notes.delete("a.md", true).unwrap(), Deleted::Removed);
    assert!(!f.dir.path().join("a.md").exists());
    f.put("full/b.md", "b");
    assert!(f.notes.delete("full", true).is_err());
    f.put("x.txt", "not a note");
    assert!(f.notes.delete("x.txt", true).is_err());
}

#[test]
fn nothing_escapes_the_folder() {
    let f = folder();
    let outside = TempDir::new().unwrap();
    fs::write(outside.path().join("secret.md"), "secret").unwrap();
    let escape = format!(
        "../{}/secret.md",
        outside.path().file_name().unwrap().to_string_lossy()
    );
    let absolute = outside
        .path()
        .join("secret.md")
        .to_string_lossy()
        .into_owned();

    for bad in [
        escape.as_str(),
        absolute.as_str(),
        "a/../../x.md",
        ".git/config.md",
        "",
    ] {
        assert!(f.notes.read(bad).is_err(), "read {bad}");
        assert!(f.notes.write(bad, "x", None).is_err(), "write {bad}");
        assert!(f.notes.delete(bad, true).is_err(), "delete {bad}");
        assert!(
            f.notes.create(Some(bad), None, "").is_err() || bad.is_empty(),
            "create in {bad}"
        );
    }
    f.put("a.md", "a");
    assert!(f.notes.rename("a.md", &escape).is_err());
    assert!(
        f.notes.write("notes.txt", "x", None).is_err(),
        "only Markdown files"
    );
    assert_eq!(
        fs::read_to_string(outside.path().join("secret.md")).unwrap(),
        "secret"
    );
}

#[cfg(unix)]
#[test]
fn symlinks_out_of_the_folder_are_refused() {
    let f = folder();
    let outside = TempDir::new().unwrap();
    fs::write(outside.path().join("secret.md"), "secret").unwrap();
    std::os::unix::fs::symlink(outside.path(), f.dir.path().join("link")).unwrap();
    std::os::unix::fs::symlink(outside.path().join("secret.md"), f.dir.path().join("s.md"))
        .unwrap();

    assert!(f.notes.read("link/secret.md").is_err());
    assert!(f.notes.write("link/new.md", "x", None).is_err());
    assert!(f.notes.read("s.md").is_err());
    assert!(!outside.path().join("new.md").exists());
    // And the list doesn't wander out through them.
    assert!(f.notes.list().unwrap().notes.is_empty());
}

#[test]
fn searches_titles_and_text() {
    let f = folder();
    f.put("a.md", "# Garden\nplant tomatoes in may\n");
    f.put("b.md", "# Shopping\ntomatoes, basil\n");
    f.put("c.md", "# Unrelated\n");
    let hits = f.notes.search("tomatoes").unwrap();
    assert_eq!(hits.len(), 2);
    let hits = f.notes.search("garden tomatoes").unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "a.md");
    assert_eq!(hits[0].line, 2);
    assert_eq!(hits[0].excerpt, "plant tomatoes in may");
    assert!(f.notes.search("   ").unwrap().is_empty());
}

#[test]
fn the_folder_setting_lives_on_the_host() {
    let f = folder();
    let other = TempDir::new().unwrap();
    let target = other.path().join("made/here");
    let set = f.notes.set_folder(Some(target.clone())).unwrap();
    assert_eq!(set.folder, target.canonicalize().unwrap());
    // A fresh Notes reads the same setting back.
    let again = Notes::new(|_| {});
    assert_eq!(again.folder().unwrap().folder, set.folder);
    assert!(f.notes.set_folder(Some("relative/path".into())).is_err());
}

#[cfg(desktop)]
#[test]
fn changes_on_disk_are_announced() {
    let f = folder();
    f.notes.list().unwrap();
    f.put("from-vim.md", "# hi");
    let paths = f
        .announced
        .recv_timeout(Duration::from_secs(5))
        .expect("an announcement");
    assert!(paths.contains(&"from-vim.md".to_string()), "{paths:?}");

    // Our own temporary files never are; the note they become is.
    let note = f.notes.read("from-vim.md").unwrap();
    while f.announced.try_recv().is_ok() {}
    f.notes
        .write("from-vim.md", "# changed", Some(&note.version))
        .unwrap();
    let paths = f.announced.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(paths.iter().all(|p| !p.contains(".tmp")), "{paths:?}");
    assert!(paths.contains(&"from-vim.md".to_string()));
}
