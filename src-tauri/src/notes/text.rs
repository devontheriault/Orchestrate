//! What the list and the search read off a note's text: its title, a line or
//! two to recognise it by, and where a search matched. Pure, so it is tested
//! without a folder.

/// How much of a note the snippet under its title holds, in characters.
const SNIPPET: usize = 160;

/// How much of the matching line a search hit shows, in characters.
const EXCERPT: usize = 140;

/// The note's text without YAML front matter, and the `title:` that front
/// matter gave, if any. Front matter is what Obsidian, Jekyll and friends put
/// at the top of a note; it isn't prose, so it is no title or snippet.
fn front_matter(text: &str) -> (&str, Option<String>) {
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return (text, None);
    };
    let mut title = None;
    let mut at = 0;
    for line in rest.split_inclusive('\n') {
        at += line.len();
        let bare = line.trim_end();
        if bare == "---" || bare == "..." {
            return (&rest[at..], title);
        }
        if let Some(t) = bare.strip_prefix("title:") {
            let t = t.trim().trim_matches(|c| c == '"' || c == '\'').trim();
            if !t.is_empty() {
                title = Some(t.to_string());
            }
        }
    }
    // Never closed: it wasn't front matter after all.
    (text, None)
}

/// An ATX heading's text: `## Plans` is `Plans`.
fn heading(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let hashes = t.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &t[hashes..];
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return None;
    }
    let text = rest.trim().trim_end_matches('#').trim();
    (!text.is_empty()).then_some(text)
}

/// A line of Markdown as plain words: list markers, quote marks, checkboxes
/// and emphasis taken off, so a snippet reads as text.
fn plain(line: &str) -> String {
    let mut t = line.trim();
    loop {
        let before = t;
        for marker in [
            "> ", "- [ ] ", "- [x] ", "* [ ] ", "* [x] ", "- ", "* ", "+ ",
        ] {
            if let Some(rest) = t.strip_prefix(marker) {
                t = rest.trim_start();
            }
        }
        let digits = t.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && t[digits..].starts_with(". ") {
            t = t[digits + 2..].trim_start();
        }
        if t == before {
            break;
        }
    }
    unlink(t)
        .replace(['*', '`'], "")
        .replace("__", "")
        .replace("~~", "")
}

/// `[text](url)` as its text, and `![alt](src)` as its alt.
fn unlink(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let Some(close) = after.find("](") else { break };
        let Some(end) = after[close + 2..].find(')') else {
            break;
        };
        out.push_str(rest[..open].trim_end_matches('!'));
        out.push_str(&after[..close]);
        rest = &after[close + 2 + end + 1..];
    }
    out.push_str(rest);
    out
}

/// Cut `s` to at most `max` characters, with an ellipsis if anything went.
fn clip(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", s[..at].trim_end()),
        None => s.to_string(),
    }
}

/// A note's title and snippet. The title is the first line when that is a
/// heading, else the first `# ` heading anywhere, else the front matter's
/// `title:`, else `fallback` (the file name). The snippet is the first prose
/// after it.
pub fn title_and_snippet(text: &str, fallback: &str) -> (String, String) {
    let (body, fm_title) = front_matter(text);
    let mut lines = body.lines().filter(|l| !l.trim().is_empty());
    let first = lines.next();
    let title_line = first.filter(|l| heading(l).is_some()).or_else(|| {
        body.lines()
            .find(|l| l.trim_start().starts_with("# ") && heading(l).is_some())
    });

    let title = title_line
        .and_then(heading)
        .map(str::to_string)
        .or(fm_title)
        .unwrap_or_else(|| fallback.to_string());

    let mut snippet = String::new();
    for line in body.lines() {
        if snippet.chars().count() > SNIPPET {
            break;
        }
        if Some(line) == title_line || heading(line).is_some() {
            continue;
        }
        let words = plain(line);
        if words.is_empty() || words.starts_with("```") {
            continue;
        }
        if !snippet.is_empty() {
            snippet.push(' ');
        }
        snippet.push_str(&words);
    }
    (title, clip(&snippet, SNIPPET))
}

/// The words of a search, lowercased. Every one has to appear for a note to
/// match, in any order.
pub fn terms(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(str::to_lowercase)
        .filter(|t| !t.is_empty())
        .collect()
}

/// How well a note matches `terms`, or `None` if one of them is missing.
/// A word in the title counts for far more than one in the text, since the
/// title is what the user remembers a note by.
pub fn score(terms: &[String], title: &str, path: &str, text: &str) -> Option<u32> {
    let title = title.to_lowercase();
    let path = path.to_lowercase();
    let text = text.to_lowercase();
    let mut score = 0u32;
    for term in terms {
        let in_title = title.contains(term.as_str());
        let in_path = path.contains(term.as_str());
        let in_text = text.matches(term.as_str()).take(20).count() as u32;
        if !in_title && !in_path && in_text == 0 {
            return None;
        }
        score += in_text + if in_title { 50 } else { 0 } + if in_path { 10 } else { 0 };
    }
    Some(score)
}

/// The line of `text` that holds the most of `terms` (the first, of equals),
/// as its 1-based number and an excerpt centred on the match. The heading
/// that is the note's `title` is passed over, since the hit already shows it.
/// `None` when only the title or path matched.
pub fn excerpt(terms: &[String], title: &str, text: &str) -> Option<(usize, String)> {
    let mut best: Option<(usize, usize, usize, &str)> = None;
    for (i, line) in text.lines().enumerate() {
        if heading(line) == Some(title) {
            continue;
        }
        let lower = line.to_lowercase();
        let found: Vec<usize> = terms
            .iter()
            .filter_map(|t| lower.find(t.as_str()))
            .collect();
        let Some(&at) = found.iter().min() else {
            continue;
        };
        if best.is_none_or(|(_, n, _, _)| found.len() > n) {
            // Where the match sits, in characters. Lowercasing can change the
            // length of a few characters, so this is a guide, kept in range.
            best = Some((i, found.len(), lower[..at].chars().count(), line));
        }
    }
    let (i, _, hit, line) = best?;
    let line = line.trim_end();
    let chars = line.chars().count();
    let start = hit.min(chars).saturating_sub(EXCERPT / 3);
    let mut out: String = line.chars().skip(start).take(EXCERPT).collect();
    out = out.trim().to_string();
    if start > 0 {
        out.insert(0, '…');
    }
    if start + EXCERPT < chars {
        out.push('…');
    }
    Some((i + 1, out))
}

/// A name the user typed, made safe to be a file name on every system the
/// folder might be synced to.
pub fn file_stem(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let squeezed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = squeezed.trim_matches(|c: char| c == '.' || c.is_whitespace());
    let stem: String = trimmed.chars().take(80).collect();
    let stem = stem.trim().to_string();
    if stem.is_empty() {
        "Untitled".into()
    } else {
        stem
    }
}

/// A stamp of a file's exact contents: the version a write is checked
/// against, so it never lands on a file that changed since it was read.
/// FNV-1a, which is stable across builds, unlike std's hasher.
pub fn version(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}{:x}", bytes.len())
}
