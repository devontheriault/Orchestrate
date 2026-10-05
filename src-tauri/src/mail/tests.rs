use std::collections::HashMap;

use super::account::{self, PasswordSetup, Saved, Security};
use super::imap::{
    decode_folder_name, destination, folders_from, internal_date, quoted, search_criteria, uid_set,
};
use super::parse::{self, safe_file_name, without_quoted_reply, Listed};
use super::sanitize::{clean_style, sanitize};
use super::{oauth, thread, Role, Summary};
use crate::test_util::StateEnv;

fn clean(html: &str) -> String {
    sanitize(html, &HashMap::new(), false).html
}

/// Nothing in `out` that could run, load or submit anything.
fn assert_inert(out: &str) {
    let lower = out.to_ascii_lowercase();
    for bad in [
        "<script",
        "javascript:",
        "onerror",
        "onload",
        "onclick",
        "onmouseover",
        "<iframe",
        "<object",
        "<embed",
        "<form",
        "<input",
        "<button",
        "<meta",
        "<base",
        "<link",
        "<svg",
        "<math",
        "<style",
        "expression(",
        "url(",
        "srcset",
        "ping=",
        "formaction",
        "<video",
        "<audio",
        "data:text",
        "vbscript:",
    ] {
        assert!(!lower.contains(bad), "{bad:?} got through in {out}");
    }
}

#[test]
fn scripts_go_with_their_contents() {
    let out = clean("<p>hi</p><script>alert(1)</script><script src=https://x/y.js></script>");
    assert_eq!(out, "<p>hi</p>");
}

#[test]
fn event_handlers_are_stripped() {
    let out = clean(
        r#"<img src=x onerror=alert(1)><body onload=alert(1)><div onclick="steal()" onmouseover=x>t</div>"#,
    );
    assert_inert(&out);
    assert!(out.contains(">t</div>"));
}

#[test]
fn script_links_are_dropped_however_they_are_spelled() {
    for href in [
        "javascript:alert(1)",
        "JaVaScRiPt:alert(1)",
        " javascript:alert(1)",
        "&#106;avascript:alert(1)",
        "java\tscript:alert(1)",
        "jav&#x09;ascript:alert(1)",
        "vbscript:msgbox(1)",
        "data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==",
        "//evil.example/x",
        "/relative/path",
        "file:///etc/passwd",
    ] {
        let out = clean(&format!(r#"<a href="{href}">x</a>"#));
        assert!(!out.contains("href"), "{href} kept: {out}");
        assert_inert(&out);
    }
}

#[test]
fn web_links_open_outside_the_app() {
    let out =
        clean(r#"<a href="https://example.com/a?b=1" target="_top" ping="https://t/p">x</a>"#);
    assert!(out.contains(r#"href="https://example.com/a?b=1""#), "{out}");
    assert!(out.contains(r#"target="_blank""#), "{out}");
    assert!(out.contains(r#"rel="noopener noreferrer""#), "{out}");
    assert!(!out.contains("_top") && !out.contains("ping"), "{out}");
    assert!(clean(r#"<a href="mailto:a@b.c">m</a>"#).contains("mailto:a@b.c"));
}

#[test]
fn active_and_framing_tags_are_removed() {
    let out = clean(concat!(
        r#"<meta http-equiv="refresh" content="0;url=https://evil"><base href="https://evil/">"#,
        r#"<link rel=stylesheet href=https://evil/x.css><iframe src=https://evil></iframe>"#,
        r#"<object data=x.swf></object><embed src=x.swf><form action=https://evil><input name=p>"#,
        r#"<button formaction=https://evil>go</button></form><svg onload=alert(1)><circle/></svg>"#,
        r#"<math><mtext>x</mtext></math><video poster=https://t/p.png src=v.mp4></video>"#,
        r#"<audio src=a.mp3></audio><input type=image src=https://t/i.png>"#,
        r#"<style>@import url(https://evil/x.css); body{background:url(https://t/b.png)}</style>"#,
        r#"<template><img src=x onerror=alert(1)></template><noscript><img src=https://t/n.gif></noscript>"#,
    ));
    assert_inert(&out);
}

#[test]
fn mutation_tricks_stay_inert() {
    for html in [
        r#"<noscript><p title="</noscript><img src=x onerror=alert(1)>"></noscript>"#,
        r#"<svg><style><img src=x onerror=alert(1)></style></svg>"#,
        r#"<math><mi><mglyph><style><img src=x onerror=alert(1)>"#,
        r#"<div><a href="https://ok"><img src="https://t/x" alt="</a><script>alert(1)</script>"></a></div>"#,
        r#"<img src="https://t/x"" onerror="alert(1)">"#,
        r#"<<script>script>alert(1)<</script>/script>"#,
        r#"<!--><script>alert(1)</script>-->"#,
        r#"<a href="https://ok" style="x:expression(alert(1))">x</a>"#,
        "<scr\0ipt>alert(1)</scr\0ipt>",
    ] {
        let out = clean(html);
        let lower = out.to_ascii_lowercase();
        assert!(!lower.contains("<script"), "{html} -> {out}");
        assert!(!lower.contains("onerror="), "{html} -> {out}");
        assert!(!lower.contains("expression("), "{html} -> {out}");
    }
}

#[test]
fn remote_images_are_left_out_and_counted() {
    let html = r#"<img src="https://news.example/hero.png" alt="Hero"><img src="http://t.example/open.gif" width=1 height=1><img src=" HTTPS://x.example/y.png">"#;
    let blocked = sanitize(html, &HashMap::new(), false);
    assert_eq!(blocked.remote_images, 3);
    assert!(!blocked.html.contains("http"), "{}", blocked.html);
    assert!(blocked.html.contains(r#"alt="Hero""#));

    let loaded = sanitize(html, &HashMap::new(), true);
    assert_eq!(loaded.remote_images, 0);
    assert!(loaded.html.contains("https://news.example/hero.png"));
}

#[test]
fn images_hidden_in_styles_and_attributes_are_dropped_even_when_loading_images() {
    let html = concat!(
        r#"<table background="https://t.example/bg.png"><tr><td style="background-image:url(https://t.example/a.png);color:red">x</td></tr></table>"#,
        r#"<div style="background: URL( 'https://t.example/b.png' )">y</div>"#,
        r#"<div style="background:u\72l(https://t.example/c.png)">z</div>"#,
        r#"<div style="list-style-image:url(x);border-image:url(y)">w</div>"#,
        r#"<div style="background-image:image-set('https://t.example/d.png' 1x)">v</div>"#,
        r#"<img srcset="https://t.example/e.png 2x" src="data:image/png;base64,iVBORw0KGgo=">"#,
    );
    let out = sanitize(html, &HashMap::new(), true).html;
    assert!(!out.contains("t.example"), "{out}");
    assert!(out.contains("color: red"), "{out}");
    assert!(out.contains("data:image/png;base64,iVBORw0KGgo="), "{out}");
}

#[test]
fn only_picture_data_urls_are_images() {
    for src in [
        "data:image/svg+xml;base64,PHN2ZyBvbmxvYWQ9YWxlcnQoMSk+",
        "data:text/html,<script>alert(1)</script>",
    ] {
        let out = clean(&format!(r#"<img src="{src}">"#));
        assert!(!out.contains("data:"), "{src} kept: {out}");
    }
}

#[test]
fn inline_images_come_from_the_message() {
    let mut inline = HashMap::new();
    inline.insert(
        "logo@shop".to_string(),
        "data:image/png;base64,AAAA".to_string(),
    );
    let out = sanitize(
        r#"<img src="cid:logo@shop"><img src="CID:<LOGO@SHOP>"><img src="cid:missing">"#,
        &inline,
        false,
    );
    assert_eq!(
        out.html.matches("data:image/png;base64,AAAA").count(),
        2,
        "{}",
        out.html
    );
    assert!(!out.html.contains("cid:"), "{}", out.html);
    assert_eq!(out.remote_images, 0);
}

#[test]
fn styles_keep_only_what_changes_how_things_look() {
    assert_eq!(
        clean_style("color: #333; font-size: 14px; position: fixed; z-index: 9999").as_deref(),
        Some("color: #333; font-size: 14px")
    );
    assert_eq!(
        clean_style("background: linear-gradient(#fff, rgb(0,0,0))").as_deref(),
        Some("background: linear-gradient(#fff, rgb(0,0,0))")
    );
    for hostile in [
        "background:url(https://t/x.png)",
        "width:expression(alert(1))",
        "color:red/**/;background:url(x)",
        "behavior:url(x.htc)",
        "-moz-binding:url(x.xml#xss)",
        "background:\\75rl(x)",
        "font-family:</style><script>",
        "background-image:image-set('x.png' 1x)",
        "content:attr(href)",
        "color:var(--x)",
        "@import 'x.css'",
    ] {
        let kept = clean_style(hostile);
        assert!(
            kept.as_deref()
                .is_none_or(|k| !k.contains('(') && !k.contains('@')),
            "{hostile} -> {kept:?}"
        );
    }
}

#[test]
fn newsletters_keep_their_layout() {
    let out = clean(
        r##"<table width="600" cellpadding="24" bgcolor="#fff" align="center"><tr><td valign="top" style="font-family:Georgia,serif;color:#b7410e"><font face="Arial" color="red">Hi</font><center>c</center></td></tr></table>"##,
    );
    for kept in [
        r#"width="600""#,
        r#"cellpadding="24""#,
        r##"bgcolor="#fff""##,
        r#"align="center""#,
        r#"valign="top""#,
        "font-family: Georgia,serif",
        "<font",
        r#"face="Arial""#,
        "<center>",
    ] {
        assert!(out.contains(kept), "lost {kept}: {out}");
    }
}

#[test]
fn titles_and_heads_leave_no_text_behind() {
    let out =
        clean("<html><head><title>Secret title</title></head><body><p>Body</p></body></html>");
    assert_eq!(out, "<p>Body</p>");
}

fn summary(uid: u32, date: i64, id: &str, refs: &[&str]) -> Summary {
    Summary {
        uid,
        folder: "INBOX".into(),
        message_id: Some(id.into()),
        references: refs.iter().map(|s| s.to_string()).collect(),
        gm_thread: None,
        subject: format!("s{uid}"),
        from: None,
        to: Vec::new(),
        date,
        snippet: String::new(),
        unread: uid.is_multiple_of(2),
        flagged: false,
        attachments: false,
        labels: Vec::new(),
    }
}

#[test]
fn replies_join_the_conversation_they_answer() {
    let threads = thread::group(vec![
        summary(1, 100, "a@x", &[]),
        summary(2, 200, "b@x", &["a@x"]),
        summary(3, 150, "c@x", &[]),
        summary(4, 300, "d@x", &["a@x", "b@x"]),
    ]);
    assert_eq!(threads.len(), 2);
    assert_eq!(threads[0].id, "m:a@x");
    let uids: Vec<u32> = threads[0].messages.iter().map(|m| m.uid).collect();
    assert_eq!(uids, [1, 2, 4], "oldest first");
    assert_eq!(threads[0].date, 300);
    assert_eq!(threads[0].unread, 2);
    assert_eq!(threads[1].messages[0].uid, 3);
}

#[test]
fn replies_to_a_missing_message_are_one_conversation() {
    let threads = thread::group(vec![
        summary(5, 100, "r1@x", &["root@x"]),
        summary(6, 200, "r2@x", &["root@x"]),
    ]);
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].id, "m:root@x");
}

#[test]
fn the_same_subject_is_not_the_same_conversation() {
    let mut a = summary(1, 100, "a@x", &[]);
    let mut b = summary(2, 200, "b@x", &[]);
    a.subject = "Invoice".into();
    b.subject = "Invoice".into();
    assert_eq!(thread::group(vec![a, b]).len(), 2);
}

#[test]
fn gmail_threads_are_taken_at_gmails_word() {
    let mut a = summary(1, 100, "a@x", &[]);
    let mut b = summary(2, 200, "b@x", &[]);
    let mut c = summary(3, 300, "c@x", &["a@x"]);
    a.gm_thread = Some(7);
    b.gm_thread = Some(7);
    c.gm_thread = Some(8);
    let threads = thread::group(vec![a, b, c]);
    assert_eq!(threads.len(), 2);
    assert_eq!(threads[1].id, "gm:7");
    assert_eq!(threads[1].messages.len(), 2);
}

#[test]
fn uid_sets_fold_runs() {
    assert_eq!(uid_set(&[7, 1, 2, 3, 4, 9, 10, 3]), "1:4,7,9:10");
    assert_eq!(uid_set(&[5]), "5");
}

#[test]
fn searches_cannot_break_out_of_their_string() {
    let typed = "invoice\" OR ALL\r\nA1 DELETE INBOX\\";
    let q = quoted(typed);
    assert!(!q.contains('\r') && !q.contains('\n'), "{q}");
    assert_eq!(q, r#""invoice\" OR ALLA1 DELETE INBOX\\""#);
    let c = search_criteria(typed, false);
    assert!(!c.contains('\n'));
    assert!(search_criteria("rust\nweekly", true).starts_with("X-GM-RAW \"rustweekly\""));
}

#[test]
fn searches_need_every_word() {
    assert_eq!(
        search_criteria(r#"from:priya roadmap "q4 review""#, false),
        r#"FROM "priya" TEXT "roadmap" TEXT "q4 review""#
    );
    assert_eq!(
        search_criteria("subject:Entwürfe", false),
        r#"CHARSET UTF-8 SUBJECT "Entwürfe""#
    );
    assert_eq!(
        search_criteria("has:attachment invoice", true),
        r#"X-GM-RAW "has:attachment invoice""#
    );
}

#[test]
fn folder_names_decode_from_modified_utf7() {
    assert_eq!(
        decode_folder_name("Projekte/Entw&APw-rfe"),
        "Projekte/Entwürfe"
    );
    assert_eq!(decode_folder_name("Tom &- Jerry"), "Tom & Jerry");
    assert_eq!(decode_folder_name("&ZeVnLIqe-"), "日本語");
    assert_eq!(decode_folder_name("Broken &abc"), "Broken &abc");
}

#[test]
fn folders_get_their_roles_from_the_server_or_their_names() {
    let l = |p: &str, a: &[&str]| {
        (
            p.to_string(),
            Some("/".to_string()),
            a.iter().map(|s| s.to_string()).collect(),
        )
    };
    let gmail = folders_from(vec![
        l("INBOX", &[]),
        l("[Gmail]", &["\\Noselect"]),
        l("[Gmail]/All Mail", &["\\All"]),
        l("[Gmail]/Sent Mail", &["\\Sent"]),
        l("[Gmail]/Trash", &["\\Trash"]),
        l("[Gmail]/Important", &["\\Important"]),
        l("Receipts", &[]),
        l("Receipts/2026", &[]),
    ]);
    let names: Vec<&str> = gmail.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Inbox",
            "Important",
            "Sent Mail",
            "All Mail",
            "Trash",
            "Receipts",
            "2026"
        ]
    );
    assert_eq!(gmail[6].depth, 1);
    assert_eq!(
        destination(&gmail, Role::Archive).as_deref(),
        Some("[Gmail]/All Mail")
    );
    assert_eq!(
        destination(&gmail, Role::Trash).as_deref(),
        Some("[Gmail]/Trash")
    );

    let plain = folders_from(vec![
        l("INBOX", &[]),
        l("Archive", &[]),
        l("Deleted Items", &[]),
        l("Sent", &[]),
    ]);
    assert_eq!(
        destination(&plain, Role::Archive).as_deref(),
        Some("Archive")
    );
    assert_eq!(
        destination(&plain, Role::Trash).as_deref(),
        Some("Deleted Items")
    );
    let bare = folders_from(vec![l("INBOX", &[])]);
    assert_eq!(destination(&bare, Role::Archive), None);
}

#[test]
fn internal_dates_read() {
    assert_eq!(
        internal_date("04-Oct-2026 13:41:52 +0000"),
        Some(1_791_121_312)
    );
    assert_eq!(
        internal_date(" 4-Oct-2026 15:41:52 +0200"),
        Some(1_791_121_312)
    );
}

const NEWSLETTER: &str = concat!(
    "From: \"This Week in Rust\" <news@rust.example>\r\n",
    "To: Devon <dev@example.test>\r\n",
    "Subject: =?utf-8?q?Issue_612_=E2=80=94_async?=\r\n",
    "Date: Sun, 04 Oct 2026 09:00:00 +0000\r\n",
    "Message-ID: <n612@rust.example>\r\n",
    "MIME-Version: 1.0\r\n",
    "Content-Type: multipart/mixed; boundary=\"outer\"\r\n",
    "\r\n",
    "--outer\r\n",
    "Content-Type: multipart/related; boundary=\"rel\"\r\n",
    "\r\n",
    "--rel\r\n",
    "Content-Type: text/html; charset=utf-8\r\n",
    "\r\n",
    "<p onclick=x>Hello <img src=\"cid:logo\"> <img src=\"https://t.example/p.gif\"></p><script>bad()</script>\r\n",
    "--rel\r\n",
    "Content-Type: image/png\r\n",
    "Content-ID: <logo>\r\n",
    "Content-Transfer-Encoding: base64\r\n",
    "\r\n",
    "iVBORw0KGgo=\r\n",
    "--rel--\r\n",
    "--outer\r\n",
    "Content-Type: application/pdf; name=\"../../etc/report.pdf\"\r\n",
    "Content-Disposition: attachment; filename=\"../../etc/report.pdf\"\r\n",
    "Content-Transfer-Encoding: base64\r\n",
    "\r\n",
    "JVBERi0xLjQK\r\n",
    "--outer--\r\n",
);

#[test]
fn opened_messages_carry_only_sanitized_html() {
    let m = parse::message(NEWSLETTER.as_bytes(), "INBOX", 9, false).unwrap();
    assert_eq!(m.subject, "Issue 612 — async");
    assert_eq!(
        m.from.as_ref().unwrap().name.as_deref(),
        Some("This Week in Rust")
    );
    let html = m.html.unwrap();
    assert_inert(&html);
    assert!(html.contains("data:image/png;base64,"), "{html}");
    assert!(!html.contains("t.example"), "{html}");
    assert_eq!(m.remote_images, 1);
    assert_eq!(
        m.attachments.len(),
        1,
        "the inline logo isn't a file: {:?}",
        m.attachments
    );
    assert_eq!(m.attachments[0].name, "report.pdf");
    assert_eq!(m.attachments[0].mime, "application/pdf");

    let d = parse::attachment(NEWSLETTER.as_bytes(), m.attachments[0].index).unwrap();
    assert_eq!(d.name, "report.pdf");
    assert_eq!(d.data, "JVBERi0xLjQK");
}

#[test]
fn listed_messages_are_summarised_from_their_start() {
    let (header, text) = NEWSLETTER.split_once("\r\n\r\n").unwrap();
    let header = format!("{header}\r\n\r\n");
    let flags = vec!["\\Seen".to_string()];
    let s = parse::summary(Listed {
        uid: 9,
        folder: "INBOX",
        header: header.as_bytes(),
        text_start: &text.as_bytes()[..200],
        flags: &flags,
        labels: Vec::new(),
        gm_thread: None,
        arrived: None,
    });
    assert_eq!(s.subject, "Issue 612 — async");
    assert_eq!(s.message_id.as_deref(), Some("n612@rust.example"));
    assert!(!s.unread);
    assert!(s.attachments);
    assert_eq!(s.date, 1_791_104_400);
}

#[test]
fn file_names_are_only_names() {
    assert_eq!(
        safe_file_name("../../.ssh/authorized_keys").as_deref(),
        Some("authorized_keys")
    );
    assert_eq!(
        safe_file_name("C:\\Windows\\evil.exe").as_deref(),
        Some("evil.exe")
    );
    assert_eq!(safe_file_name(".bashrc").as_deref(), Some("bashrc"));
    assert_eq!(
        safe_file_name("a\u{0}b\nc.txt").as_deref(),
        Some("a_b_c.txt")
    );
    assert_eq!(safe_file_name("..").as_deref(), None);
    assert_eq!(
        safe_file_name("Report: Q4?.pdf").as_deref(),
        Some("Report_ Q4_.pdf")
    );
}

#[test]
fn replies_drop_the_copy_of_what_they_answer() {
    let reply = "Sounds good.\n\nOn Thu, 1 Oct 2026, Priya Raman <p@x> wrote:\n> Hi\n> there\n";
    assert_eq!(without_quoted_reply(reply), "Sounds good.");
    let wrapped = "Yes.\n\nOn Thu, 1 Oct 2026 at 10:00, Priya Raman\n<p@x> wrote:\n\n> Hi\n";
    assert_eq!(without_quoted_reply(wrapped), "Yes.");
    let inline = "> question?\nanswer\n";
    assert_eq!(without_quoted_reply(inline), "> question?\nanswer");
}

#[test]
fn agents_get_the_conversation_quoted() {
    let first = parse::message(NEWSLETTER.as_bytes(), "INBOX", 9, false).unwrap();
    let text = parse::quote_thread(&[first]);
    assert!(text.starts_with("Subject: Issue 612 — async\n"), "{text}");
    assert!(
        text.contains("From: This Week in Rust <news@rust.example>"),
        "{text}"
    );
    assert!(text.contains("Attachments: report.pdf"), "{text}");
    assert!(text.contains("> Hello"), "{text}");
    assert!(!text.contains("bad()"), "{text}");
}

#[test]
fn plain_imap_only_reaches_this_machine() {
    let setup = |server: &str, security| PasswordSetup {
        email: "me@example.test".into(),
        server: server.into(),
        port: 143,
        security,
        username: None,
        password: "pw".into(),
    };
    assert!(Saved::password(setup("127.0.0.1", Security::Plain)).is_ok());
    assert!(Saved::password(setup("localhost", Security::Plain)).is_ok());
    assert!(Saved::password(setup("[::1]", Security::Plain)).is_ok());
    assert!(Saved::password(setup("imap.example.com", Security::Plain)).is_err());
    assert!(Saved::password(setup("127.0.0.1.evil.example", Security::Plain)).is_err());
    assert!(Saved::password(setup("imap.example.com", Security::Tls)).is_ok());
}

#[cfg(unix)]
#[test]
fn credentials_are_readable_only_by_the_user() {
    use std::os::unix::fs::PermissionsExt;
    let _env = StateEnv::new();
    let saved = Saved::password(PasswordSetup {
        email: "me@example.test".into(),
        server: "imap.example.test".into(),
        port: 993,
        security: Security::Tls,
        username: None,
        password: "hunter2".into(),
    })
    .unwrap();
    account::save(&saved).unwrap();
    let dir = account::dir().unwrap();
    let file = dir.join("account.json");
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o700
    );

    // Widened by something else, it is narrowed again on reading.
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(account::load().unwrap(), Some(saved));
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );

    // What the window is told carries no secret.
    let info = serde_json::to_string(&account::load().unwrap().unwrap().info()).unwrap();
    assert!(!info.contains("hunter2"));

    account::remove().unwrap();
    assert_eq!(account::load().unwrap(), None);
}

#[test]
fn an_id_token_says_which_account_signed_in() {
    // {"email":"me@gmail.com"}
    let token = "eyJhbGciOiJub25lIn0.eyJlbWFpbCI6Im1lQGdtYWlsLmNvbSJ9.sig";
    assert_eq!(oauth::email_of(token).as_deref(), Some("me@gmail.com"));
    assert_eq!(oauth::email_of("garbage"), None);
}

/// Against a real IMAP server, seeded by `dev/seed.py`. Run it with GreenMail
/// up: `ORCHESTRATE_TEST_IMAP=127.0.0.1:31430 cargo test mail -- --ignored`.
#[tokio::test]
#[ignore]
async fn reads_a_real_server() {
    use super::imap::Imap;
    use super::MailStore;
    let Ok(addr) = std::env::var("ORCHESTRATE_TEST_IMAP") else {
        return;
    };
    let (server, port) = addr.split_once(':').unwrap();
    let _env = StateEnv::new();
    let imap = Imap::new(
        Saved::password(PasswordSetup {
            email: "dev@example.test".into(),
            server: server.into(),
            port: port.parse().unwrap(),
            security: Security::Plain,
            username: None,
            password: "devpass".into(),
        })
        .unwrap(),
    );
    let folders = imap.folders().await.unwrap();
    let inbox = folders
        .iter()
        .find(|f| f.role == Some(Role::Inbox))
        .unwrap();
    assert!(inbox.total >= 8, "{folders:?}");
    assert!(
        folders.iter().any(|f| f.name.ends_with("Entwürfe")),
        "{folders:?}"
    );

    let threads = thread::group(imap.recent("INBOX", 300).await.unwrap());
    let roadmap = threads
        .iter()
        .find(|t| t.subject.starts_with("Q4 roadmap"))
        .unwrap();
    assert_eq!(roadmap.messages.len(), 3, "{roadmap:?}");
    // Listing again comes from the cache, and says the same.
    let again = thread::group(imap.recent("INBOX", 300).await.unwrap());
    assert_eq!(again.len(), threads.len());

    let found = imap.search("Archive", "kumquat-3", 50).await.unwrap();
    assert_eq!(found.len(), 1, "{found:?}");

    let news = threads
        .iter()
        .find(|t| t.subject.contains("Rust 612"))
        .unwrap();
    let raw = imap.raw("INBOX", news.messages[0].uid).await.unwrap();
    let m = parse::message(&raw, "INBOX", news.messages[0].uid, false).unwrap();
    assert!(m.remote_images >= 3, "{}", m.remote_images);
    assert_inert(m.html.as_deref().unwrap());
}

#[test]
fn a_header_without_its_blank_line_still_ends() {
    // GreenMail sends BODY[HEADER] without the blank line that ends it.
    let header = "From: a@b.c\r\nSubject: Hi\r\nMIME-Version: 1.0\r\nContent-Type: multipart/alternative; boundary=\"XX\"\r\n";
    let text = "--XX\r\nContent-Type: text/plain\r\n\r\nThe real text.\r\n--XX\r\nContent-Type: text/html\r\n\r\n<p>x</p>\r\n--XX--\r\n";
    let s = parse::summary(Listed {
        uid: 1,
        folder: "INBOX",
        header: header.as_bytes(),
        text_start: text.as_bytes(),
        flags: &[],
        labels: Vec::new(),
        gm_thread: None,
        arrived: None,
    });
    assert_eq!(s.snippet, "The real text.");
}

#[test]
fn snippets_leave_out_what_a_reply_quotes() {
    let header = "From: a@b.c\r\nSubject: Re: Hi\r\n\r\n";
    let text =
        "Yes, Thursday works.\r\n\r\nMarco\r\n\r\nOn Wed, Priya wrote:\r\n> Shall we meet?\r\n";
    let s = parse::summary(Listed {
        uid: 1,
        folder: "INBOX",
        header: header.as_bytes(),
        text_start: text.as_bytes(),
        flags: &[],
        labels: Vec::new(),
        gm_thread: None,
        arrived: None,
    });
    assert_eq!(s.snippet, "Yes, Thursday works. Marco");
}

fn microsoft(tenant: Option<&str>) -> account::OAuthSetup {
    serde_json::from_value(serde_json::json!({
        "email": "me@outlook.com",
        "clientId": " 00000000-1111-2222-3333-444444444444 ",
        "tenant": tenant,
    }))
    .unwrap()
}

#[test]
fn outlook_signs_in_with_microsoft() {
    let setup: account::Setup = serde_json::from_value(serde_json::json!({
        "auth": "microsoft",
        "email": "me@outlook.com",
        "clientId": "abc",
    }))
    .unwrap();
    assert!(matches!(setup, account::Setup::Microsoft(_)));

    let url = oauth::auth_url(
        account::OAuthProvider::Microsoft,
        &microsoft(None),
        "http://localhost:4711",
        "CHALLENGE",
        "STATE",
    )
    .unwrap();
    let parsed = url::Url::parse(&url).unwrap();
    assert_eq!(parsed.host_str(), Some("login.microsoftonline.com"));
    assert_eq!(parsed.path(), "/common/oauth2/v2.0/authorize");
    let q: HashMap<_, _> = parsed.query_pairs().into_owned().collect();
    assert_eq!(q["client_id"], "00000000-1111-2222-3333-444444444444");
    assert_eq!(q["redirect_uri"], "http://localhost:4711");
    assert!(q["scope"].contains("https://outlook.office.com/IMAP.AccessAsUser.All"));
    assert!(q["scope"].contains("offline_access"));
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(q["login_hint"], "me@outlook.com");
    assert!(!q.contains_key("client_secret"));

    let work = oauth::auth_url(
        account::OAuthProvider::Microsoft,
        &microsoft(Some("contoso.onmicrosoft.com")),
        "http://localhost:1",
        "c",
        "s",
    )
    .unwrap();
    assert!(work.starts_with("https://login.microsoftonline.com/contoso.onmicrosoft.com/"));
    // The directory goes into the URL's path, so it can't carry anything else.
    for hostile in ["evil.example/x?", "../common", "a b"] {
        assert!(oauth::endpoints(account::OAuthProvider::Microsoft, Some(hostile)).is_err());
    }
}

#[test]
fn token_requests_carry_a_secret_only_where_there_is_one() {
    use account::OAuthProvider::{Google, Microsoft};
    use oauth::Grant;
    let ms = oauth::token_form(Microsoft, "id", None, Grant::Refresh("r1"));
    let ms: HashMap<_, _> = ms.into_iter().collect();
    assert!(!ms.contains_key("client_secret"));
    assert_eq!(ms["grant_type"], "refresh_token");
    assert_eq!(ms["refresh_token"], "r1");
    assert!(ms["scope"].contains("IMAP.AccessAsUser.All"));

    let g = oauth::token_form(
        Google,
        "id",
        Some("sec"),
        Grant::Code {
            code: "c",
            verifier: "v",
            redirect: "http://127.0.0.1:9",
        },
    );
    let g: HashMap<_, _> = g.into_iter().collect();
    assert_eq!(g["client_secret"], "sec");
    assert_eq!(g["code_verifier"], "v");
    assert!(!g.contains_key("scope"));
}

#[test]
fn microsoft_accounts_name_their_address_as_preferred_username() {
    // {"preferred_username":"me@contoso.com","name":"Me"}
    let token = "x.eyJwcmVmZXJyZWRfdXNlcm5hbWUiOiJtZUBjb250b3NvLmNvbSIsIm5hbWUiOiJNZSJ9.sig";
    assert_eq!(oauth::email_of(token).as_deref(), Some("me@contoso.com"));
}

#[test]
fn outlook_accounts_read_from_office365() {
    let saved = Saved::oauth(
        account::OAuthProvider::Microsoft,
        "me@outlook.com".into(),
        &microsoft(None),
        "refresh".into(),
    );
    assert_eq!(saved.server, "outlook.office365.com");
    assert_eq!(saved.port, 993);
    assert_eq!(saved.security, Security::Tls);
    assert_eq!(saved.info().auth, "microsoft");
    let shown = serde_json::to_string(&saved.info()).unwrap();
    assert!(!shown.contains("refresh"));
}

#[test]
fn refusals_say_what_to_try() {
    use super::imap::login_hint;
    assert!(login_hint("outlook.office365.com", true).contains("sign in with Microsoft"));
    assert!(login_hint("Outlook.Office365.com", false).contains("IMAP is on"));
    assert!(login_hint("imap.gmail.com", true).contains("app password"));
    assert_eq!(login_hint("imap.fastmail.com", true), "");
}

#[test]
fn outlooks_folders_are_known_by_name() {
    let l = |p: &str| (p.to_string(), Some("/".to_string()), Vec::new());
    let folders = folders_from(vec![
        l("INBOX"),
        l("Sent Items"),
        l("Deleted Items"),
        l("Junk Email"),
        l("Archive"),
        l("Drafts"),
        l("Conversation History"),
    ]);
    let role = |p: &str| folders.iter().find(|f| f.path == p).unwrap().role;
    assert_eq!(role("Sent Items"), Some(Role::Sent));
    assert_eq!(role("Deleted Items"), Some(Role::Trash));
    assert_eq!(role("Junk Email"), Some(Role::Junk));
    assert_eq!(role("Conversation History"), None);
    assert_eq!(
        destination(&folders, Role::Archive).as_deref(),
        Some("Archive")
    );
}

/// XOAUTH2 end to end, against GreenMail, which takes the password as the
/// token. Run as `reads_a_real_server` is.
#[tokio::test]
#[ignore]
async fn signs_in_with_xoauth2() {
    use super::imap::Imap;
    use super::MailStore;
    let Ok(addr) = std::env::var("ORCHESTRATE_TEST_IMAP") else {
        return;
    };
    let (server, port) = addr.split_once(':').unwrap();
    let _env = StateEnv::new();
    let mut saved = Saved::oauth(
        account::OAuthProvider::Microsoft,
        "dev@example.test".into(),
        &microsoft(None),
        "refresh".into(),
    );
    saved.server = server.into();
    saved.port = port.parse().unwrap();
    saved.security = Security::Plain;
    oauth::hold(&saved, "devpass");
    let folders = Imap::new(saved).folders().await.unwrap();
    assert!(folders.iter().any(|f| f.role == Some(Role::Inbox)));
}
