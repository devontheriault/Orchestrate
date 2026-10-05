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
fn snippets_read_as_plain_words() {
    let (_, snippet) = title_and_snippet(
        "# T\nSee *this* and [the ADR](https://x.dev) ![pic](a.png)\n",
        "f",
    );
    assert_eq!(snippet, "See this and the ADR pic");
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

// ------------------------------------------------------------- MCP tools

#[test]
fn a_page_of_a_note_is_whole_lines_and_moves_on() {
    let text = "one\ntwo\nthree\nfour";
    assert_eq!(mcp::page(text, 1, 1000), (text.to_string(), 4));
    assert_eq!(mcp::page(text, 2, 2), ("two\nthree\n".to_string(), 3));
    assert_eq!(mcp::page(text, 9, 2), (String::new(), 8));
    // A page stops short of a huge line, but always gives at least one.
    let huge = format!("a\n{}\nb\n", "x".repeat(70_000));
    let (first, last) = mcp::page(&huge, 1, 1000);
    assert_eq!((first.as_str(), last), ("a\n", 1));
    let (second, last) = mcp::page(&huge, 2, 1000);
    assert_eq!((second.len(), last), (70_001, 2));
}

fn note(content: &str) -> Note {
    Note {
        path: "Plans/Shop.md".into(),
        content: content.into(),
        version: "v1".into(),
        modified: 0,
    }
}

#[test]
fn a_read_note_says_where_it_is_and_how_to_read_on() {
    let text = (1..=5).map(|i| format!("line {i}\n")).collect::<String>();
    let whole = mcp::shown(&note(&text), 1, 1000);
    assert!(
        whole.starts_with("path: Plans/Shop.md\nversion: v1\nmodified: "),
        "{whole}"
    );
    assert!(
        whole.ends_with("lines: all 5\n\nline 1\nline 2\nline 3\nline 4\nline 5\n"),
        "{whole}"
    );

    let part = mcp::shown(&note(&text), 2, 2);
    assert!(
        part.contains("lines: 2 to 3 of 5. Read on with from_line 4\n\nline 2\nline 3\n"),
        "{part}"
    );
    assert!(mcp::shown(&note(&text), 9, 2).contains("lines: none from 9: the note has 5\n"));
    assert!(mcp::shown(&note(""), 1, 10).contains("lines: none, the note is empty\n"));
}

#[test]
fn an_edit_replaces_one_passage_or_adds_to_the_end() {
    let e = |text, old, new| mcp::edited("n.md", text, old, new);
    assert_eq!(e("a\nb\nc\n", Some("b"), "B").unwrap(), "a\nB\nc\n");
    assert_eq!(e("a\nb", None, "c\n").unwrap(), "a\nb\nc\n");
    assert_eq!(e("a\n", Some(""), "c\n").unwrap(), "a\nc\n");
    assert_eq!(e("", None, "first\n").unwrap(), "first\n");
    assert!(e("a\nb\n", Some("z"), "Z")
        .unwrap_err()
        .contains("isn't in n.md"));
    assert!(e("b\nb\n", Some("b"), "B").unwrap_err().contains("2 times"));
    // A Windows note matches the model's plain line breaks, and keeps its own.
    assert_eq!(
        e("a\r\nb\r\nc\r\n", Some("a\nb"), "A\nB").unwrap(),
        "A\r\nB\r\nc\r\n"
    );
}

#[test]
fn a_note_moved_into_a_folder_keeps_its_name() {
    assert_eq!(
        mcp::destination("Inbox/Idea.md", "Projects/"),
        "Projects/Idea.md"
    );
    assert_eq!(
        mcp::destination("Idea.md", "Projects/Better"),
        "Projects/Better"
    );
}

#[test]
fn the_list_can_keep_to_a_subfolder_and_says_when_theres_more() {
    let summary = |path: &str| NoteSummary {
        path: path.into(),
        title: path.into(),
        snippet: String::new(),
        modified: 0,
    };
    let list = || NoteList {
        folder: "/home/u/Notes".into(),
        notes: ["Work/a.md", "Workshop.md", "Work/Deep/b.md", "c.md"]
            .map(summary)
            .into(),
        folders: vec!["Work".into(), "Work/Deep".into()],
    };
    let all = mcp::listing(list(), None, 50).unwrap();
    assert_eq!((all.total, all.notes.len()), (4, 4));
    assert!(all.more.is_none());

    let work = mcp::listing(list(), Some("/Work/"), 1).unwrap();
    assert_eq!(work.total, 2, "not Workshop.md");
    assert_eq!(work.notes[0].path, "Work/a.md");
    assert!(work.more.unwrap().contains("1 most recently changed of 2"));
    assert_eq!(work.subfolders, ["Work/Deep"]);

    assert!(mcp::listing(list(), Some("Play"), 50).is_err());
    assert!(mcp::listing(list(), Some(".."), 50).is_err());
}

/// A Host serving this state directory's socket in this process, with its
/// notes in a fresh folder, and the tools' handle on it.
struct NotesHost {
    _env: StateEnv,
    dir: TempDir,
    serving: tokio::task::JoinHandle<()>,
    host: crate::mcp::Host,
}

impl Drop for NotesHost {
    fn drop(&mut self) {
        self.serving.abort();
    }
}

async fn notes_host() -> NotesHost {
    let env = StateEnv::new();
    crate::paths::ensure_dirs().unwrap();
    let (rt, rx) = crate::runtime::AgentRuntime::with_bin("true");
    let host = crate::host::Host::new(rt, rx, vec![]);
    let socket = crate::paths::host_socket().unwrap();
    let listener = crate::host::local::Listener::bind(&socket).unwrap();
    let serving = tokio::spawn(crate::host::serve(host, listener, std::future::ready(None)));
    let dir = TempDir::new().unwrap();
    let host = crate::mcp::Host::local().await.unwrap();
    host.call(
        "set_notes_folder",
        serde_json::json!({ "folder": dir.path() }),
    )
    .await
    .unwrap();
    NotesHost {
        _env: env,
        dir,
        serving,
        host,
    }
}

fn read_args(path: &str) -> mcp::ReadArgs {
    mcp::ReadArgs {
        path: path.into(),
        from_line: None,
        lines: None,
    }
}

fn version_in(shown: &str) -> String {
    shown
        .lines()
        .find_map(|l| l.strip_prefix("version: "))
        .unwrap()
        .to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_reads_and_writes_notes_through_the_host() {
    let NotesHost { dir, host, .. } = &notes_host().await;
    fs::create_dir_all(dir.path().join("Projects")).unwrap();
    fs::write(
        dir.path().join("Projects/Shop.md"),
        "# Shop launch\n\nShip the cart on Friday.\n",
    )
    .unwrap();
    fs::write(dir.path().join("Groceries.md"), "# Groceries\n- eggs\n").unwrap();
    let on_disk = |rel: &str| fs::read_to_string(dir.path().join(rel)).unwrap();

    let listed = mcp::list_notes(
        host.clone(),
        mcp::ListArgs {
            folder: None,
            limit: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(listed.total, 2);
    assert_eq!(listed.subfolders, ["Projects"]);
    assert_eq!(
        listed.folder,
        dir.path().canonicalize().unwrap().display().to_string()
    );

    let found = mcp::search_notes(
        host.clone(),
        mcp::SearchArgs {
            query: "cart friday".into(),
            limit: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(found.total, 1);
    let hit = &found.notes[0];
    assert_eq!(
        (hit.path.as_str(), hit.title.as_str(), hit.line),
        ("Projects/Shop.md", "Shop launch", Some(3))
    );

    let shown = mcp::read_note(host.clone(), read_args("Projects/Shop.md"))
        .await
        .unwrap();
    assert!(
        shown.ends_with("\n\n# Shop launch\n\nShip the cart on Friday.\n"),
        "{shown}"
    );
    let stale = version_in(&shown);

    // An edit lands on the note as it is now, whatever the model last read.
    let edit = |old: Option<&str>, new: &str| {
        mcp::edit_note(
            host.clone(),
            mcp::EditArgs {
                path: "Projects/Shop.md".into(),
                old_text: old.map(Into::into),
                new_text: new.into(),
            },
        )
    };
    let said = edit(Some("on Friday"), "on Monday").await.unwrap();
    assert!(said.starts_with("Saved Projects/Shop.md"), "{said}");
    edit(None, "- [ ] tell Ada\n").await.unwrap();
    assert_eq!(
        on_disk("Projects/Shop.md"),
        "# Shop launch\n\nShip the cart on Monday.\n- [ ] tell Ada\n"
    );

    // A whole write from a version that's since changed is refused, and so
    // is a new note on top of an old one. Nothing is written either time.
    let write = |path: &str, version: Option<String>| {
        mcp::write_note(
            host.clone(),
            mcp::WriteArgs {
                path: path.into(),
                content: "# Replaced\n".into(),
                version,
            },
        )
    };
    let refused = write("Projects/Shop.md", Some(stale)).await.unwrap_err();
    assert!(refused.contains("changed since you read it"), "{refused}");
    let refused = write("Groceries.md", None).await.unwrap_err();
    assert!(refused.contains("already a note"), "{refused}");
    assert_eq!(on_disk("Groceries.md"), "# Groceries\n- eggs\n");
    // From the version just read, it goes through.
    let now = mcp::read_note(host.clone(), read_args("Groceries.md"))
        .await
        .unwrap();
    write("Groceries.md", Some(version_in(&now))).await.unwrap();
    assert_eq!(on_disk("Groceries.md"), "# Replaced\n");

    let made = mcp::create_note(
        host.clone(),
        mcp::CreateArgs {
            title: "Shop launch".into(),
            content: "# Shop launch\nagain\n".into(),
            folder: Some("Projects".into()),
        },
    )
    .await
    .unwrap();
    assert!(
        made.starts_with("Saved as Projects/Shop launch.md"),
        "{made}"
    );

    let rename = |path: &str, to: &str| {
        mcp::rename_note(
            host.clone(),
            mcp::RenameArgs {
                path: path.into(),
                to: to.into(),
            },
        )
    };
    let moved = rename("Groceries.md", "Archive/").await.unwrap();
    assert_eq!(moved, "Moved Groceries.md to Archive/Groceries.md.");
    assert_eq!(on_disk("Archive/Groceries.md"), "# Replaced\n");
    // Never onto a note that's there already.
    assert!(rename("Projects/Shop.md", "Projects/Shop launch.md")
        .await
        .is_err());
    // One note at a time, never a folder.
    let refused = rename("Projects", "Old").await.unwrap_err();
    assert!(refused.contains("never a folder"), "{refused}");
    assert!(dir.path().join("Projects/Shop.md").exists());
}

/// The Host refuses every path outside the folder (see `nothing_escapes_the_folder`);
/// this checks no tool finds a way round that, whichever argument it comes in.
#[tokio::test(flavor = "multi_thread")]
async fn no_tool_reaches_outside_the_notes_folder() {
    let NotesHost { dir, host, .. } = &notes_host().await;
    let outside = TempDir::new().unwrap();
    let secret = outside.path().join("secret.md");
    fs::write(&secret, "secret").unwrap();
    fs::write(dir.path().join("a.md"), "a").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
    let up_dir = format!(
        "../{}",
        outside.path().file_name().unwrap().to_string_lossy()
    );
    let up = format!("{up_dir}/secret.md");
    let absolute = secret.display().to_string();

    for bad in [
        up.as_str(),
        absolute.as_str(),
        "a/../../x.md",
        "link/secret.md",
        ".hidden/x.md",
    ] {
        let read = mcp::read_note(host.clone(), read_args(bad)).await;
        assert!(read.is_err(), "read {bad}: {read:?}");
        let wrote = mcp::write_note(
            host.clone(),
            mcp::WriteArgs {
                path: bad.into(),
                content: "x".into(),
                version: None,
            },
        )
        .await;
        assert!(wrote.is_err(), "write {bad}");
        let edit = mcp::edit_note(
            host.clone(),
            mcp::EditArgs {
                path: bad.into(),
                old_text: None,
                new_text: "x".into(),
            },
        )
        .await;
        assert!(edit.is_err(), "edit {bad}");
        let rename = |path: &str, to: &str| {
            mcp::rename_note(
                host.clone(),
                mcp::RenameArgs {
                    path: path.into(),
                    to: to.into(),
                },
            )
        };
        assert!(rename("a.md", bad).await.is_err(), "rename to {bad}");
        assert!(rename(bad, "stolen.md").await.is_err(), "rename from {bad}");
    }
    let create = |title: &str, folder: Option<&str>| {
        mcp::create_note(
            host.clone(),
            mcp::CreateArgs {
                title: title.into(),
                content: "x".into(),
                folder: folder.map(Into::into),
            },
        )
    };
    for bad in [up_dir.as_str(), outside.path().to_str().unwrap(), "link"] {
        let made = create("x", Some(bad)).await;
        assert!(made.is_err(), "create in {bad}: {made:?}");
    }
    // A title is only ever a name, never a path.
    let made = create("../../escape", None).await.unwrap();
    assert!(made.starts_with("Saved as escape.md"), "{made}");

    assert_eq!(fs::read_to_string(&secret).unwrap(), "secret");
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 1);
    assert_eq!(fs::read_to_string(dir.path().join("a.md")).unwrap(), "a");
}
