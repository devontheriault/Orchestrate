//! Reading what the server sends: headers into a [`Summary`], a whole message
//! into a [`Message`] with its HTML [`sanitize`]d, and a conversation into the
//! quoted plain text an Agent is handed. Parsing is mail-parser's, which
//! decodes the charsets, encoded words and transfer encodings mail is full of.

use std::collections::HashMap;

use base64::Engine;
use mail_parser::{HeaderValue, MessageParser, MimeHeaders, PartType};

use super::sanitize::sanitize;
use super::{Address, AttachmentInfo, Download, Message, Summary};

/// How much of a body the message list shows.
const SNIPPET_CHARS: usize = 180;

/// Inline images bigger than this are left out of the page rather than
/// inlined into it.
const INLINE_IMAGE_MAX: usize = 3 * 1024 * 1024;

/// How much of each message, and of a whole conversation, an Agent is handed.
const QUOTE_MESSAGE_MAX: usize = 20_000;
const QUOTE_THREAD_MAX: usize = 100_000;

/// What the list needs from a message the server described: its header, and
/// the first few kilobytes of its body for the snippet.
pub struct Listed<'a> {
    pub uid: u32,
    pub folder: &'a str,
    pub header: &'a [u8],
    pub text_start: &'a [u8],
    pub flags: &'a [String],
    pub labels: Vec<String>,
    pub gm_thread: Option<u64>,
    /// The server's arrival time, for a message whose Date is missing.
    pub arrived: Option<i64>,
}

pub fn summary(l: Listed<'_>) -> Summary {
    let mut bytes = Vec::with_capacity(l.header.len() + l.text_start.len());
    bytes.extend_from_slice(l.header);
    bytes.extend_from_slice(l.text_start);
    let parsed = MessageParser::default().parse(&bytes);

    let subject = parsed
        .as_ref()
        .and_then(|m| m.subject())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "(no subject)".into());
    let date = parsed
        .as_ref()
        .and_then(|m| m.date())
        .map(|d| d.to_timestamp())
        .or(l.arrived)
        .unwrap_or(0);
    let snippet = parsed
        .as_ref()
        .and_then(|m| m.body_preview(SNIPPET_CHARS * 2))
        .map(|s| one_line(&s, SNIPPET_CHARS))
        .unwrap_or_default();
    let attachments = parsed.as_ref().is_some_and(|m| {
        m.root_part().is_content_type("multipart", "mixed") || m.attachment_count() > 0
    });
    let flag = |f: &str| l.flags.iter().any(|x| x.eq_ignore_ascii_case(f));

    Summary {
        uid: l.uid,
        folder: l.folder.to_string(),
        message_id: parsed.as_ref().and_then(|m| m.message_id()).map(str::to_string),
        references: parsed.as_ref().map(references).unwrap_or_default(),
        gm_thread: l.gm_thread,
        subject,
        from: parsed.as_ref().and_then(|m| m.from()).and_then(first),
        to: parsed.as_ref().map(|m| all(m.to())).unwrap_or_default(),
        date,
        snippet,
        unread: !flag("\\Seen"),
        flagged: flag("\\Flagged"),
        attachments,
        labels: l.labels,
    }
}

/// The ids a message answers, oldest first: its References, then its
/// In-Reply-To if that isn't the last of them already.
fn references(m: &mail_parser::Message<'_>) -> Vec<String> {
    let mut out = ids(m.references());
    for id in ids(m.in_reply_to()) {
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

fn ids(v: &HeaderValue<'_>) -> Vec<String> {
    match v {
        HeaderValue::Text(t) => vec![t.to_string()],
        HeaderValue::TextList(l) => l.iter().map(|t| t.to_string()).collect(),
        _ => Vec::new(),
    }
}

fn first(a: &mail_parser::Address<'_>) -> Option<Address> {
    a.first().and_then(address)
}

fn all(a: Option<&mail_parser::Address<'_>>) -> Vec<Address> {
    a.map(|a| a.iter().filter_map(address).collect())
        .unwrap_or_default()
}

fn address(a: &mail_parser::Addr<'_>) -> Option<Address> {
    let email = a.address.as_ref()?.trim().to_string();
    let name = a
        .name
        .as_ref()
        .map(|n| n.trim().trim_matches('"').trim().to_string())
        .filter(|n| !n.is_empty() && *n != email);
    Some(Address { name, email })
}

/// Text squeezed onto one line, cut to `max` characters.
fn one_line(s: &str, max: usize) -> String {
    let joined = s.split_whitespace().collect::<Vec<_>>().join(" ");
    match joined.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &joined[..i]),
        None => joined,
    }
}

/// A whole message, opened. Its HTML is sanitized here, with remote images
/// left out unless `images`, so nothing the sender wrote reaches the window
/// unchecked.
pub fn message(raw: &[u8], folder: &str, uid: u32, images: bool) -> Option<Message> {
    let m = MessageParser::default().parse(raw)?;

    // Images the HTML names by Content-ID, carried in the message itself.
    let mut inline = HashMap::new();
    let mut inline_parts = Vec::new();
    for (i, part) in m.parts.iter().enumerate() {
        let (Some(cid), Some(ct)) = (part.content_id(), part.content_type()) else {
            continue;
        };
        if !ct.ctype().eq_ignore_ascii_case("image") {
            continue;
        }
        let bytes = part.contents();
        if bytes.len() > INLINE_IMAGE_MAX {
            continue;
        }
        let mime = format!("image/{}", ct.subtype().unwrap_or("png")).to_ascii_lowercase();
        let url = format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        );
        inline.insert(cid.trim_matches(|c| c == '<' || c == '>').to_ascii_lowercase(), url);
        inline_parts.push(i as u32);
    }

    let html_part = m
        .html_body
        .first()
        .and_then(|i| m.parts.get(*i as usize))
        .filter(|p| matches!(p.body, PartType::Html(_)));
    let clean = html_part
        .and_then(|p| p.text_contents())
        .map(|html| sanitize(html, &inline, images));
    let text = m.body_text(0).map(|t| t.to_string()).unwrap_or_default();

    // The HTML shows the inline images it uses; the rest are files.
    let attachments = m
        .attachments
        .iter()
        .enumerate()
        .filter_map(|(index, id)| {
            let part = m.parts.get(*id as usize)?;
            let shown_inline = inline_parts.contains(id) && clean.is_some();
            if shown_inline && !is_attachment_disposition(part) {
                return None;
            }
            Some(AttachmentInfo {
                index,
                name: attachment_name(part, index),
                mime: mime_of(part),
                size: part.contents().len(),
            })
        })
        .collect();

    Some(Message {
        uid,
        folder: folder.to_string(),
        subject: m.subject().unwrap_or("(no subject)").trim().to_string(),
        from: m.from().and_then(first),
        to: all(m.to()),
        cc: all(m.cc()),
        date: m.date().map(|d| d.to_timestamp()).unwrap_or(0),
        remote_images: clean.as_ref().map_or(0, |c| c.remote_images),
        html: clean.map(|c| c.html),
        text,
        attachments,
    })
}

fn is_attachment_disposition(part: &mail_parser::MessagePart<'_>) -> bool {
    part.content_disposition()
        .is_some_and(|d| d.ctype().eq_ignore_ascii_case("attachment"))
}

fn mime_of(part: &mail_parser::MessagePart<'_>) -> String {
    match part.content_type() {
        Some(ct) => match ct.subtype() {
            Some(sub) => format!("{}/{}", ct.ctype(), sub),
            None => ct.ctype().to_string(),
        }
        .to_ascii_lowercase(),
        None if matches!(part.body, PartType::Message(_)) => "message/rfc822".into(),
        None => "application/octet-stream".into(),
    }
}

/// The attachment's own name, made safe to save under: no folders, nothing
/// hidden, nothing a shell or a file system would read as more than a name.
fn attachment_name(part: &mail_parser::MessagePart<'_>, index: usize) -> String {
    let given = part
        .attachment_name()
        .map(str::to_string)
        .or_else(|| match &part.body {
            PartType::Message(m) => m.subject().map(|s| format!("{s}.eml")),
            _ => None,
        })
        .unwrap_or_default();
    safe_file_name(&given).unwrap_or_else(|| format!("attachment-{}", index + 1))
}

pub fn safe_file_name(name: &str) -> Option<String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .map(|c| if c.is_control() || ":*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim().to_string();
    (!cleaned.is_empty()).then_some(cleaned)
}

/// One attachment's bytes, by its place in the message.
pub fn attachment(raw: &[u8], index: usize) -> Option<Download> {
    let m = MessageParser::default().parse(raw)?;
    let id = *m.attachments.get(index)?;
    let part = m.parts.get(id as usize)?;
    let bytes: Vec<u8> = match &part.body {
        // A forwarded message is saved as the message it is.
        PartType::Message(inner) => inner.raw_message().to_vec(),
        _ => part.contents().to_vec(),
    };
    Some(Download {
        name: attachment_name(part, index),
        mime: mime_of(part),
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

/// A conversation as an Agent reads it: each message's headers, then its text
/// quoted with `> `, oldest first. A reply's own copy of what it answers is
/// cut, since that message is in the conversation already.
pub fn quote_thread(messages: &[Message]) -> String {
    let mut out = String::new();
    if let Some(first) = messages.first() {
        out.push_str(&format!("Subject: {}\n", first.subject));
    }
    for (i, m) in messages.iter().enumerate() {
        out.push('\n');
        if messages.len() > 1 {
            out.push_str(&format!("Message {} of {}\n", i + 1, messages.len()));
        }
        if let Some(from) = &m.from {
            out.push_str(&format!("From: {}\n", show(from)));
        }
        if !m.to.is_empty() {
            let to: Vec<String> = m.to.iter().map(show).collect();
            out.push_str(&format!("To: {}\n", to.join(", ")));
        }
        if m.date > 0 {
            out.push_str(&format!("Date: {}\n", when(m.date)));
        }
        if !m.attachments.is_empty() {
            let names: Vec<&str> = m.attachments.iter().map(|a| a.name.as_str()).collect();
            out.push_str(&format!("Attachments: {}\n", names.join(", ")));
        }
        out.push('\n');
        let body = if messages.len() > 1 {
            without_quoted_reply(&m.text)
        } else {
            m.text.trim().to_string()
        };
        let body = cut(&body, QUOTE_MESSAGE_MAX);
        for line in body.lines() {
            if line.is_empty() {
                out.push_str(">\n");
            } else {
                out.push_str("> ");
                out.push_str(line);
                out.push('\n');
            }
        }
        if out.len() > QUOTE_THREAD_MAX {
            out = cut(&out, QUOTE_THREAD_MAX);
            out.push_str("\n[the rest of the conversation is left out]\n");
            break;
        }
    }
    out
}

fn show(a: &Address) -> String {
    match &a.name {
        Some(name) => format!("{name} <{}>", a.email),
        None => a.email.clone(),
    }
}

fn when(unix: i64) -> String {
    let format = time::macros::format_description!("[year]-[month]-[day] [hour]:[minute] UTC");
    time::OffsetDateTime::from_unix_timestamp(unix)
        .ok()
        .and_then(|t| t.format(&format).ok())
        .unwrap_or_default()
}

fn cut(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[cut]", &s[..end])
}

/// A reply's text without the copy of the message it answers: the quoted
/// lines at its end, and the "On …, … wrote:" line that introduces them.
pub fn without_quoted_reply(text: &str) -> String {
    let lines: Vec<&str> = text.trim_end().lines().collect();
    let mut end = lines.len();
    while end > 0 && (lines[end - 1].starts_with('>') || lines[end - 1].trim().is_empty()) {
        end -= 1;
    }
    if end == lines.len() {
        return text.trim().to_string();
    }
    if lines[end - 1].trim_end().ends_with("wrote:") {
        end -= 1;
        // The introduction may wrap onto a second line.
        if !lines[end].starts_with("On ") && end > 0 && lines[end - 1].starts_with("On ") {
            end -= 1;
        }
    }
    lines[..end].join("\n").trim().to_string()
}
