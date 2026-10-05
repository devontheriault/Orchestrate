//! The Notes Space's MCP tools (see `crate::mcp` for how tools work). They
//! read the user's notes, and may write them, since a note is a local file the
//! user can see and undo (ADR 0017).
//!
//! The Host keeps every promise in `notes/mod.rs` for these calls as for a
//! window's: no path leaves the notes folder, and no write lands on a note
//! that changed since it was read. So a write here names the version it was
//! read at, and a conflict comes back to the model as a failed call telling it
//! to read again.
//!
//! There is no tool to delete a note. Renaming moves one note at a time, never
//! a folder, so whatever an Agent did to the folder can be seen note by note.

use chrono::{Local, SecondsFormat, TimeZone};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;

use super::{is_note, Note, NoteList, Saved, SearchHit};
use crate::mcp::{Host, Tool};

/// How many notes `list_notes` gives unless asked for more.
const LIST: usize = 50;

/// How many notes `search_notes` gives unless asked for more.
const HITS: usize = 20;

/// How many lines `read_note` gives unless asked for more.
const LINES: usize = 1000;

/// The most text `read_note` gives at once, in characters, however many lines
/// that is: about a tenth of a model's context, so one note can't fill it.
const CHARS: usize = 60_000;

pub fn tools() -> Vec<Tool> {
    vec![
        Tool::reads(
            "list_notes",
            "List the user's notes, newest first: each one's path, title, opening words and \
             when it last changed, plus the subfolders. Notes are Markdown files in the user's \
             notes folder, which is also given. Use search_notes to find notes about something.",
            list_notes,
        ),
        Tool::reads(
            "read_note",
            "Read one of the user's notes: its Markdown text, and the version write_note \
             needs to replace it. Long notes come a page at a time.",
            read_note,
        ),
        Tool::reads(
            "search_notes",
            "Find the user's notes that mention every word of a query, in their title, path \
             or text, best match first: each one's path, title and the line that matched.",
            search_notes,
        ),
        Tool::writes(
            "create_note",
            "Make a new note in the user's notes folder. It appears in their Notes Space at \
             once. Answers with the path it was saved at, which has a number added if a note \
             by that name was already there.",
            create_note,
        ),
        Tool::writes(
            "edit_note",
            "Change part of one of the user's notes: replace one passage of its text with \
             another, or add text at the end. The rest of the note is left as it is. Prefer \
             it to write_note for any change smaller than the whole note.",
            edit_note,
        ),
        Tool::writes(
            "write_note",
            "Replace the whole text of one of the user's notes, or make a note at an exact \
             path. Read the note first: the write is refused if the note changed since, \
             so it never overwrites the user's own edits.",
            write_note,
        ),
        Tool::writes(
            "rename_note",
            "Rename one of the user's notes, or move it to another subfolder of their notes \
             folder. It never replaces a note that's already there. Links to it from other \
             notes aren't updated.",
            rename_note,
        ),
    ]
}

/// When a note last changed, in the user's zone, from the Host's milliseconds.
fn when(millis: i64) -> String {
    Local
        .timestamp_millis_opt(millis)
        .single()
        .map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, false))
        .unwrap_or_default()
}

// ------------------------------------------------------------------- list

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ListArgs {
    /// Only the notes in this subfolder of the notes folder and those under it,
    /// like "Projects" or "Projects/Shop". Defaults to every note.
    pub(super) folder: Option<String>,
    /// At most this many notes, the most recently changed. Defaults to 50.
    pub(super) limit: Option<usize>,
}

/// The notes folder as the model reads it.
#[derive(Debug, Serialize)]
pub(super) struct Listing {
    /// Where the notes folder is on this machine.
    pub(super) folder: String,
    /// How many notes there are, in the subfolder if one was asked for.
    pub(super) total: usize,
    pub(super) notes: Vec<Listed>,
    /// What to do when not every note was shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) more: Option<String>,
    pub(super) subfolders: Vec<String>,
}

/// A note as the list shows it.
#[derive(Debug, Serialize)]
pub(super) struct Listed {
    pub(super) path: String,
    pub(super) title: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(super) snippet: String,
    pub(super) modified: String,
}

/// A subfolder as the model may name it: no slashes at either end.
fn subfolder(folder: Option<&str>) -> Option<&str> {
    folder
        .map(|f| f.trim().trim_matches('/'))
        .filter(|f| !f.is_empty() && *f != ".")
}

/// Whether `path` lies in `folder`, or under it.
fn within(path: &str, folder: &str) -> bool {
    path.strip_prefix(folder)
        .is_some_and(|rest| rest.starts_with('/'))
}

pub(super) fn listing(
    list: NoteList,
    folder: Option<&str>,
    limit: usize,
) -> Result<Listing, String> {
    let folder = subfolder(folder);
    if let Some(f) = folder {
        if !list.folders.iter().any(|known| known == f) {
            return Err(format!(
                "there's no subfolder “{f}” in the notes folder; list_notes without a folder \
                 names them all."
            ));
        }
    }
    let mut notes: Vec<_> = list
        .notes
        .into_iter()
        .filter(|n| folder.is_none_or(|f| within(&n.path, f)))
        .collect();
    let total = notes.len();
    notes.truncate(limit);
    let more = (total > notes.len()).then(|| {
        format!(
            "Showing the {} most recently changed of {total}. Ask for a higher limit or a \
             subfolder, or use search_notes.",
            notes.len()
        )
    });
    Ok(Listing {
        folder: list.folder.display().to_string(),
        total,
        notes: notes
            .into_iter()
            .map(|n| Listed {
                modified: when(n.modified),
                path: n.path,
                title: n.title,
                snippet: n.snippet,
            })
            .collect(),
        more,
        subfolders: list
            .folders
            .into_iter()
            .filter(|s| folder.is_none_or(|f| within(s, f)))
            .collect(),
    })
}

pub(super) async fn list_notes(host: Host, a: ListArgs) -> Result<Listing, String> {
    let list: NoteList = host.call_as("notes_list", json!({})).await?;
    listing(list, a.folder.as_deref(), a.limit.unwrap_or(LIST))
}

// ------------------------------------------------------------------- read

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ReadArgs {
    /// The note's path in the notes folder, as list_notes or search_notes gives
    /// it, like "Projects/Shop plan.md".
    pub(super) path: String,
    /// The first line to read, counting from 1. Defaults to 1. search_notes
    /// gives the line each match is on.
    pub(super) from_line: Option<usize>,
    /// At most this many lines. Defaults to 1000, and a page is cut shorter if
    /// its lines are very long.
    pub(super) lines: Option<usize>,
}

/// Lines `from` (from 1) to at most `count` of them, cut short once they come to
/// [`CHARS`]: the text, and the number of the last line given. Always at
/// least one line, however long, so paging moves on.
pub(super) fn page(text: &str, from: usize, count: usize) -> (String, usize) {
    let from = from.max(1);
    let mut out = String::new();
    let mut last = from - 1;
    for line in text.split_inclusive('\n').skip(from - 1).take(count.max(1)) {
        if last >= from && out.chars().count() + line.chars().count() > CHARS {
            break;
        }
        out.push_str(line);
        last += 1;
    }
    (out, last)
}

/// A note as the model reads it: a few lines about it, a blank line, then
/// its text, or the page of it asked for.
pub(super) fn shown(note: &Note, from: usize, count: usize) -> String {
    let total = note.content.split_inclusive('\n').count();
    let from = from.max(1);
    let (text, last) = page(&note.content, from, count);
    let lines = if total == 0 {
        "none, the note is empty".to_string()
    } else if from > total {
        format!("none from {from}: the note has {total}")
    } else if from == 1 && last == total {
        format!("all {total}")
    } else if last < total {
        format!(
            "{from} to {last} of {total}. Read on with from_line {}",
            last + 1
        )
    } else {
        format!("{from} to {last} of {total}")
    };
    format!(
        "path: {}\nversion: {}\nmodified: {}\nlines: {lines}\n\n{text}",
        note.path,
        note.version,
        when(note.modified)
    )
}

pub(super) async fn read_note(host: Host, a: ReadArgs) -> Result<String, String> {
    let note: Note = host
        .call_as("notes_read", json!({ "path": a.path }))
        .await?;
    Ok(shown(
        &note,
        a.from_line.unwrap_or(1),
        a.lines.unwrap_or(LINES),
    ))
}

// ----------------------------------------------------------------- search

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct SearchArgs {
    /// The words to look for, like "dentist" or "shop launch plan". A note
    /// matches when every word is somewhere in its title, path or text, in any
    /// order and any case. Plain words, not a pattern.
    pub(super) query: String,
    /// At most this many notes, the best matches. Defaults to 20.
    pub(super) limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(super) struct Found {
    /// How many notes matched, up to 200.
    pub(super) total: usize,
    pub(super) notes: Vec<Hit>,
}

/// A note a search matched, as the model reads it.
#[derive(Debug, Serialize)]
pub(super) struct Hit {
    pub(super) path: String,
    pub(super) title: String,
    /// The line that matched best, for read_note's from_line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) line: Option<usize>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(super) excerpt: String,
    pub(super) modified: String,
}

pub(super) async fn search_notes(host: Host, a: SearchArgs) -> Result<Found, String> {
    if a.query.trim().is_empty() {
        return Err("search_notes needs some words to look for.".into());
    }
    let hits: Vec<SearchHit> = host
        .call_as("notes_search", json!({ "query": a.query }))
        .await?;
    let total = hits.len();
    Ok(Found {
        total,
        notes: hits
            .into_iter()
            .take(a.limit.unwrap_or(HITS))
            .map(|h| Hit {
                modified: when(h.modified),
                line: (h.line > 0).then_some(h.line),
                path: h.path,
                title: h.title,
                excerpt: h.excerpt,
            })
            .collect(),
    })
}

// ----------------------------------------------------------------- create

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct CreateArgs {
    /// What the note is called. Its file name is made from this.
    pub(super) title: String,
    /// The note's text, in Markdown. Start it with "# " and the title for the
    /// note to show that title wherever it is opened.
    pub(super) content: String,
    /// A subfolder of the notes folder to put it in, like "Projects/Shop",
    /// made if it isn't there. Defaults to the notes folder itself.
    pub(super) folder: Option<String>,
}

pub(super) async fn create_note(host: Host, a: CreateArgs) -> Result<String, String> {
    let note: Note = host
        .call_as(
            "notes_create",
            json!({ "dir": a.folder, "name": a.title, "content": a.content }),
        )
        .await?;
    Ok(format!(
        "Saved as {} (version {}). It's in the user's Notes Space now.",
        note.path, note.version
    ))
}

// ------------------------------------------------------------------ write

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct WriteArgs {
    /// The note's path in the notes folder, like "Projects/Shop plan.md".
    pub(super) path: String,
    /// The note's whole new text, in Markdown. Everything in the note now is
    /// replaced by it, so read the whole note first.
    pub(super) content: String,
    /// The version read_note gave, so that the user's own edits since are never
    /// overwritten. Leave it out only to make a new note at exactly this path,
    /// which is refused if there's a note there already.
    pub(super) version: Option<String>,
}

/// A write's outcome, said to the model. A conflict is a failed call, since
/// nothing was written and the model has to read again.
fn saved(path: &str, version: Option<&str>, outcome: Saved) -> Result<String, String> {
    match (outcome, version) {
        (Saved::Saved { version, .. }, _) => Ok(format!("Saved {path} (version {version}).")),
        (Saved::Conflict { current: Some(_) }, None) => Err(format!(
            "there's already a note at {path}, so nothing was written. To change it, read it \
             with read_note and use edit_note, or write_note with its version; for a new \
             note, use create_note or another path."
        )),
        (Saved::Conflict { current: Some(_) }, Some(_)) => Err(format!(
            "{path} changed since you read it, most likely edited by the user, so nothing was \
             written. Read it again with read_note and make your change to the text as it \
             is now."
        )),
        (Saved::Conflict { current: None }, _) => Err(format!(
            "{path} is no longer in the notes folder, so nothing was written. It may have \
             been renamed or deleted; list_notes or search_notes will find it if it was moved."
        )),
    }
}

pub(super) async fn write_note(host: Host, a: WriteArgs) -> Result<String, String> {
    let outcome: Saved = host
        .call_as(
            "notes_write",
            json!({ "path": a.path, "content": a.content, "version": a.version }),
        )
        .await?;
    saved(&a.path, a.version.as_deref(), outcome)
}

// ------------------------------------------------------------------- edit

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct EditArgs {
    /// The note's path in the notes folder, like "Projects/Shop plan.md".
    pub(super) path: String,
    /// The passage to replace, exactly as it is in the note, including its line
    /// breaks. It must appear only once, so give enough of it to tell it apart.
    /// Leave it out to add `new_text` at the end of the note instead.
    pub(super) old_text: Option<String>,
    /// What goes in its place, or at the end of the note.
    pub(super) new_text: String,
}

/// `text` with `old` replaced by `new`, or with `new` added at the end on a
/// line of its own when there is no `old`. `old` must appear exactly once.
pub(super) fn edited(
    path: &str,
    text: &str,
    old: Option<&str>,
    new: &str,
) -> Result<String, String> {
    let Some(old) = old.filter(|o| !o.is_empty()) else {
        let mut out = text.to_string();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(new);
        return Ok(out);
    };
    // A note saved on Windows has CRLF line breaks, which the model won't
    // have typed: match it with them, and write the new text the same way.
    let crlf = text.contains("\r\n") && !old.contains("\r\n");
    let (old, new) = if crlf {
        (old.replace('\n', "\r\n"), new.replace('\n', "\r\n"))
    } else {
        (old.to_string(), new.to_string())
    };
    match text.matches(old.as_str()).count() {
        1 => Ok(text.replacen(old.as_str(), &new, 1)),
        0 => Err(format!(
            "that passage isn't in {path}, so nothing was changed. Read it with read_note \
             and copy the passage exactly."
        )),
        n => Err(format!(
            "that passage is in {path} {n} times, so nothing was changed. Give more of the \
             text around the one you mean."
        )),
    }
}

pub(super) async fn edit_note(host: Host, a: EditArgs) -> Result<String, String> {
    let note: Note = host
        .call_as("notes_read", json!({ "path": a.path }))
        .await?;
    let content = edited(
        &note.path,
        &note.content,
        a.old_text.as_deref(),
        &a.new_text,
    )?;
    // Written against the version just read, so an edit the user makes in
    // between is never lost: the model is told to try again instead.
    let outcome: Saved = host
        .call_as(
            "notes_write",
            json!({ "path": note.path, "content": content, "version": note.version }),
        )
        .await?;
    saved(&note.path, Some(&note.version), outcome)
}

// ----------------------------------------------------------------- rename

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct RenameArgs {
    /// The note's path in the notes folder now, like "Inbox/Idea.md".
    pub(super) path: String,
    /// Where it goes, like "Projects/Shop/Idea.md" or "Projects/Shop/" to keep
    /// its name. Subfolders are made as needed, and .md is added if left off.
    pub(super) to: String,
}

/// Where a note goes: a path ending in `/` is a folder to move it into,
/// keeping its name.
pub(super) fn destination(from: &str, to: &str) -> String {
    let to = to.trim();
    if !to.ends_with('/') {
        return to.to_string();
    }
    let name = from.trim().rsplit('/').next().unwrap_or_default();
    format!("{to}{name}")
}

pub(super) async fn rename_note(host: Host, a: RenameArgs) -> Result<String, String> {
    if !is_note(Path::new(a.path.trim())) {
        return Err(format!(
            "{} isn't a note: rename_note moves one note, ending in .md, at a time, and \
             never a folder.",
            a.path
        ));
    }
    let to = destination(&a.path, &a.to);
    let went: String = host
        .call_as("notes_rename", json!({ "from": a.path, "to": to }))
        .await?;
    Ok(format!("Moved {} to {went}.", a.path.trim()))
}
