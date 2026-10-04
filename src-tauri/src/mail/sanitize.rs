//! Mail's HTML is written by whoever sent it, so it is untrusted. Only HTML
//! that has been through [`sanitize`] ever leaves the Host, and the window
//! then shows it in a sandboxed frame with no scripts, no access to the app,
//! and a content policy of its own (`src/lib/mail/frame.ts`). Two walls, so
//! that a slip in one is caught by the other.
//!
//! What gets through is a fixed list of formatting tags and attributes.
//! Scripts, styles sheets, forms, frames, and anything else that runs, loads or
//! submits are removed, contents and all. Links go only to the web, mail and
//! phone numbers, and always open outside the app. Remote images are left out
//! unless the user asked for them, since loading one tells the sender the mail
//! was read, and when, and from where. Images sent inside the message (`cid:`)
//! are always shown, from the message's own bytes.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// A message's HTML, made safe.
#[derive(Debug, Clone, PartialEq)]
pub struct Clean {
    pub html: String,
    /// Remote images left out, which the user can choose to load.
    pub remote_images: usize,
}

/// Tags a message's text may use beyond ammonia's own safe list: the ones old
/// newsletters still lay themselves out with.
const EXTRA_TAGS: &[&str] = &["font", "center", "big"];

/// Attributes any allowed tag may keep. The presentational ones are how mail
/// is laid out, since its authors can't count on a style sheet.
const ATTRIBUTES: &[&str] = &[
    "style", "align", "valign", "width", "height", "bgcolor", "color", "border", "dir",
    "cellpadding", "cellspacing", "face", "size", "colspan", "rowspan", "nowrap",
];

/// Tags removed with everything inside them, not just unwrapped.
const DROPPED_WITH_CONTENT: &[&str] = &["script", "style", "title", "template", "noscript"];

/// CSS properties a `style` attribute may keep. Nothing that positions over
/// the page, and nothing that takes an image.
const STYLE_PROPERTIES: &[&str] = &[
    "color", "background-color", "background", "font", "font-family", "font-size",
    "font-weight", "font-style", "font-variant", "line-height", "text-align",
    "text-decoration", "text-transform", "text-indent", "letter-spacing", "word-spacing",
    "vertical-align", "white-space", "word-break", "overflow-wrap", "word-wrap", "margin",
    "margin-top", "margin-right", "margin-bottom", "margin-left", "padding", "padding-top",
    "padding-right", "padding-bottom", "padding-left", "border", "border-top", "border-right",
    "border-bottom", "border-left", "border-color", "border-style", "border-width",
    "border-radius", "border-collapse", "border-spacing", "width", "height", "max-width",
    "min-width", "max-height", "min-height", "display", "table-layout", "list-style-type",
    "direction", "opacity", "box-sizing", "float", "clear", "overflow",
];

/// CSS functions a value may call. None of them can fetch anything.
const STYLE_FUNCTIONS: &[&str] = &[
    "rgb", "rgba", "hsl", "hsla", "calc", "linear-gradient", "radial-gradient",
];

/// Inline images a message may carry, by `data:` type. SVG is left out:
/// it is a document, not a picture.
const DATA_IMAGES: &[&str] = &[
    "data:image/png;", "data:image/jpeg;", "data:image/jpg;", "data:image/gif;",
    "data:image/webp;",
];

/// Clean `html`. `inline` maps a Content-ID to the `data:` URL of the part it
/// names. Remote images are kept only if `images`.
pub fn sanitize(html: &str, inline: &HashMap<String, String>, images: bool) -> Clean {
    let remote = Arc::new(AtomicUsize::new(0));
    let counted = remote.clone();
    let inline = Arc::new(inline.clone());

    let mut b = ammonia::Builder::default();
    b.add_tags(EXTRA_TAGS)
        .add_generic_attributes(ATTRIBUTES)
        .clean_content_tags(DROPPED_WITH_CONTENT.iter().copied().collect::<HashSet<_>>())
        .url_schemes(["http", "https", "mailto", "tel", "cid", "data"].into())
        .url_relative(ammonia::UrlRelative::Deny)
        .link_rel(Some("noopener noreferrer"))
        .set_tag_attribute_value("a", "target", "_blank")
        .strip_comments(true)
        .attribute_filter(move |element, attribute, value| {
            filter(element, attribute, value, &inline, images, &counted)
        });
    let html = b.clean(html).to_string();
    Clean {
        html,
        remote_images: remote.load(Ordering::Relaxed),
    }
}

fn filter<'u>(
    element: &str,
    attribute: &str,
    value: &'u str,
    inline: &HashMap<String, String>,
    images: bool,
    remote: &AtomicUsize,
) -> Option<Cow<'u, str>> {
    let v = value.trim();
    let lower = v.to_ascii_lowercase();
    match (element, attribute) {
        ("img", "src") => {
            if let Some(id) = lower.strip_prefix("cid:") {
                let id = id.trim_matches(|c| c == '<' || c == '>');
                return inline.get(id).map(|url| Cow::Owned(url.clone()));
            }
            if DATA_IMAGES.iter().any(|p| lower.starts_with(p)) {
                return Some(Cow::Borrowed(v));
            }
            if lower.starts_with("https://") || lower.starts_with("http://") {
                if images {
                    return Some(Cow::Borrowed(v));
                }
                remote.fetch_add(1, Ordering::Relaxed);
            }
            None
        }
        (_, "href") => ["https://", "http://", "mailto:", "tel:"]
            .iter()
            .any(|s| lower.starts_with(s))
            .then_some(Cow::Borrowed(v)),
        (_, "style") => clean_style(v).map(Cow::Owned),
        _ => Some(Cow::Borrowed(value)),
    }
}

/// Keep the declarations of a `style` attribute that can only change how
/// things look. Anything that could fetch, run or hide what it does is
/// dropped: `url()` and every other function not on the list, `@import`, and
/// CSS escapes and comments, which can spell any of those without the letters.
pub fn clean_style(style: &str) -> Option<String> {
    if style.contains('\\') || style.contains("/*") || style.contains('<') {
        return None;
    }
    let kept: Vec<String> = style
        .split(';')
        .filter_map(|decl| {
            let (prop, value) = decl.split_once(':')?;
            let prop = prop.trim().to_ascii_lowercase();
            let value = value.trim();
            if !STYLE_PROPERTIES.contains(&prop.as_str()) || value.is_empty() {
                return None;
            }
            safe_value(&value.to_ascii_lowercase()).then(|| format!("{prop}: {value}"))
        })
        .collect();
    (!kept.is_empty()).then(|| kept.join("; "))
}

fn safe_value(value: &str) -> bool {
    if value.contains('@') || value.contains("expression") || value.contains("javascript") {
        return false;
    }
    // Every `name(` must be a function on the list.
    let mut rest = value;
    while let Some(open) = rest.find('(') {
        let name = rest[..open]
            .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
            .next()
            .unwrap_or("");
        if !STYLE_FUNCTIONS.contains(&name) {
            return false;
        }
        rest = &rest[open + 1..];
    }
    true
}
