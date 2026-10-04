//! Any CalDAV server (RFC 4791): Fastmail, iCloud, Nextcloud, Radicale and
//! the rest. A user name and password, usually an app password, over HTTPS.
//!
//! A sync asks for every calendar's ctag first, and leaves alone any calendar
//! whose ctag hasn't moved. For one that has, it lists the etags of what the
//! calendar holds and fetches only the objects whose etag changed.

use std::collections::{HashMap, HashSet};

use reqwest::{Method, StatusCode, Url};
use serde::{Deserialize, Serialize};

use super::{ical, AccountCache, CachedCalendar, Provider, Resource, SyncError};

const DAV: &str = "DAV:";
const CALDAV: &str = "urn:ietf:params:xml:ns:caldav";
const CS: &str = "http://calendarserver.org/ns/";
const APPLE: &str = "http://apple.com/ns/ical/";

/// How many objects one multiget asks for.
const BATCH: usize = 100;

/// How to sign in to a CalDAV server, as kept in `accounts.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Login {
    /// What the user gave: the server, their principal, or a calendar.
    pub url: String,
    pub username: String,
    pub password: String,
    /// Where their calendars are, once found.
    pub home: Option<String>,
}

/// One `<response>` of a multistatus: what it's about, and its properties
/// that came back 200.
#[derive(Debug, Default)]
struct Response {
    href: String,
    props: Vec<Prop>,
}

#[derive(Debug, Default)]
struct Prop {
    ns: String,
    name: String,
    text: String,
    /// The elements directly inside it, by namespace and name.
    children: Vec<(String, String)>,
    /// Any `<href>`s inside it.
    hrefs: Vec<String>,
    /// The `name` attributes of `<comp>`s inside it.
    comps: Vec<String>,
}

impl Response {
    fn prop(&self, ns: &str, name: &str) -> Option<&Prop> {
        self.props.iter().find(|p| p.ns == ns && p.name == name)
    }

    fn text(&self, ns: &str, name: &str) -> Option<String> {
        self.prop(ns, name)
            .map(|p| p.text.trim().to_string())
            .filter(|t| !t.is_empty())
    }

    fn is(&self, ns: &str, kind: &str) -> bool {
        self.prop(DAV, "resourcetype")
            .is_some_and(|p| p.children.iter().any(|(n, k)| n == ns && k == kind))
    }
}

fn multistatus(xml: &str) -> Result<Vec<Response>, SyncError> {
    let doc = roxmltree::Document::parse(xml)
        .map_err(|e| SyncError::Other(format!("the server's answer wasn't readable XML: {e}")))?;
    let is = |n: &roxmltree::Node, ns: &str, name: &str| {
        n.is_element() && n.tag_name().namespace() == Some(ns) && n.tag_name().name() == name
    };
    let mut out = Vec::new();
    for response in doc.descendants().filter(|n| is(n, DAV, "response")) {
        let href = response
            .children()
            .find(|n| is(n, DAV, "href"))
            .and_then(|n| n.text())
            .unwrap_or_default()
            .trim()
            .to_string();
        let mut props = Vec::new();
        for propstat in response.children().filter(|n| is(n, DAV, "propstat")) {
            let ok = propstat
                .children()
                .find(|n| is(n, DAV, "status"))
                .and_then(|n| n.text())
                .is_none_or(|s| s.contains(" 200 "));
            if !ok {
                continue;
            }
            for prop in propstat.children().filter(|n| is(n, DAV, "prop")) {
                for p in prop.children().filter(|n| n.is_element()) {
                    props.push(Prop {
                        ns: p.tag_name().namespace().unwrap_or_default().to_string(),
                        name: p.tag_name().name().to_string(),
                        text: p
                            .descendants()
                            .filter(|n| n.is_text())
                            .filter_map(|n| n.text())
                            .collect(),
                        children: p
                            .children()
                            .filter(|n| n.is_element())
                            .map(|n| {
                                (
                                    n.tag_name().namespace().unwrap_or_default().to_string(),
                                    n.tag_name().name().to_string(),
                                )
                            })
                            .collect(),
                        hrefs: p
                            .descendants()
                            .filter(|n| is(n, DAV, "href"))
                            .filter_map(|n| n.text())
                            .map(|t| t.trim().to_string())
                            .collect(),
                        comps: p
                            .descendants()
                            .filter(|n| is(n, CALDAV, "comp"))
                            .filter_map(|n| n.attribute("name"))
                            .map(str::to_string)
                            .collect(),
                    });
                }
            }
        }
        out.push(Response { href, props });
    }
    Ok(out)
}

fn propfind(props: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:c="{CALDAV}" xmlns:cs="{CS}" xmlns:a="{APPLE}"><d:prop>{props}</d:prop></d:propfind>"#
    )
}

/// One CalDAV account.
pub struct CalDav<'a> {
    http: &'a reqwest::Client,
    login: &'a mut Login,
}

impl<'a> CalDav<'a> {
    pub fn new(http: &'a reqwest::Client, login: &'a mut Login) -> Self {
        Self { http, login }
    }

    fn url(&self, href: &str, base: &str) -> Result<Url, SyncError> {
        Url::parse(base)
            .and_then(|b| b.join(href))
            .map_err(|e| SyncError::Other(format!("not a URL: {href} ({e})")))
    }

    async fn request(
        &self,
        method: &str,
        url: &str,
        depth: &str,
        body: String,
    ) -> Result<String, SyncError> {
        let method = Method::from_bytes(method.as_bytes()).expect("a valid method");
        let mut req = self
            .http
            .request(method, url)
            .header("content-type", "application/xml; charset=utf-8")
            .header("depth", depth)
            .body(body);
        if !self.login.username.is_empty() {
            req = req.basic_auth(&self.login.username, Some(&self.login.password));
        }
        let res = req
            .send()
            .await
            .map_err(|e| SyncError::Other(format!("could not reach {url}: {e}")))?;
        let status = res.status();
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(SyncError::Auth(
                "the server refused the user name or password".into(),
            ));
        }
        if !status.is_success() {
            return Err(SyncError::Other(format!("{url} answered {status}")));
        }
        res.text()
            .await
            .map_err(|e| SyncError::Other(format!("{url}: {e}")))
    }

    /// Find where the user's calendars are, from whatever URL they gave: a
    /// server, their principal, the calendar home, or one calendar.
    pub async fn discover(&mut self) -> Result<(), SyncError> {
        let asks = propfind("<d:current-user-principal/><d:resourcetype/><c:calendar-home-set/>");
        let given = self.login.url.clone();
        let mut tried = vec![given.clone()];
        if let Ok(u) = Url::parse(&given) {
            if let Ok(wk) = u.join("/.well-known/caldav") {
                tried.push(wk.to_string());
            }
        }
        let mut last = SyncError::Other(format!("{given} isn't a CalDAV server"));
        for url in tried {
            let rs = match self.request("PROPFIND", &url, "0", asks.clone()).await {
                Ok(text) => multistatus(&text)?,
                Err(e @ SyncError::Auth(_)) => return Err(e),
                Err(e) => {
                    last = e;
                    continue;
                }
            };
            let Some(r) = rs.first() else { continue };
            if r.is(CALDAV, "calendar") {
                self.login.home = Some(url);
                return Ok(());
            }
            if let Some(home) = r
                .prop(CALDAV, "calendar-home-set")
                .and_then(|p| p.hrefs.first())
            {
                self.login.home = Some(self.url(home, &url)?.to_string());
                return Ok(());
            }
            let Some(principal) = r
                .prop(DAV, "current-user-principal")
                .and_then(|p| p.hrefs.first())
            else {
                continue;
            };
            let principal = self.url(principal, &url)?.to_string();
            let text = self
                .request(
                    "PROPFIND",
                    &principal,
                    "0",
                    propfind("<c:calendar-home-set/>"),
                )
                .await?;
            if let Some(home) = multistatus(&text)?
                .first()
                .and_then(|r| r.prop(CALDAV, "calendar-home-set"))
                .and_then(|p| p.hrefs.first())
            {
                self.login.home = Some(self.url(home, &principal)?.to_string());
                return Ok(());
            }
        }
        Err(last)
    }

    /// The calendars that hold Calendar events: their URL, name, colour and
    /// ctag.
    async fn calendars(
        &self,
        home: &str,
    ) -> Result<Vec<(String, String, Option<String>, Option<String>)>, SyncError> {
        let text = self
            .request(
                "PROPFIND",
                home,
                "1",
                propfind(
                    "<d:resourcetype/><d:displayname/><cs:getctag/><a:calendar-color/>\
                     <c:supported-calendar-component-set/>",
                ),
            )
            .await?;
        let mut out = Vec::new();
        for r in multistatus(&text)? {
            if !r.is(CALDAV, "calendar") {
                continue;
            }
            let comps = r
                .prop(CALDAV, "supported-calendar-component-set")
                .map(|p| p.comps.clone())
                .unwrap_or_default();
            if !comps.is_empty() && !comps.iter().any(|c| c.eq_ignore_ascii_case("VEVENT")) {
                continue;
            }
            let url = self.url(&r.href, home)?.to_string();
            let name = r.text(DAV, "displayname").unwrap_or_else(|| {
                url.trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or("Calendar")
                    .to_string()
            });
            // Apple's colours are `#rrggbbaa`; the alpha is always opaque.
            let color = r
                .text(APPLE, "calendar-color")
                .map(|c| c.chars().take(7).collect::<String>());
            out.push((url, name, color, r.text(CS, "getctag")));
        }
        Ok(out)
    }

    /// What a calendar holds, by URL, with each object's etag.
    async fn etags(&self, calendar: &str) -> Result<HashMap<String, String>, SyncError> {
        let text = self
            .request(
                "PROPFIND",
                calendar,
                "1",
                propfind("<d:getetag/><d:resourcetype/>"),
            )
            .await?;
        let mut out = HashMap::new();
        for r in multistatus(&text)? {
            if r.is(DAV, "collection") {
                continue;
            }
            if let Some(etag) = r.text(DAV, "getetag") {
                out.insert(self.url(&r.href, calendar)?.to_string(), etag);
            }
        }
        Ok(out)
    }

    /// The objects at `urls`, with their etags.
    async fn multiget(
        &self,
        calendar: &str,
        urls: &[String],
    ) -> Result<Vec<(String, Option<String>, String)>, SyncError> {
        let hrefs: String = urls
            .iter()
            .filter_map(|u| Url::parse(u).ok())
            .map(|u| format!("<d:href>{}</d:href>", xml_escape(u.path())))
            .collect();
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<c:calendar-multiget xmlns:d="DAV:" xmlns:c="{CALDAV}"><d:prop><d:getetag/><c:calendar-data/></d:prop>{hrefs}</c:calendar-multiget>"#
        );
        let text = self.request("REPORT", calendar, "1", body).await?;
        let mut out = Vec::new();
        for r in multistatus(&text)? {
            if let Some(data) = r.prop(CALDAV, "calendar-data").map(|p| p.text.clone()) {
                out.push((
                    self.url(&r.href, calendar)?.to_string(),
                    r.text(DAV, "getetag"),
                    data,
                ));
            }
        }
        Ok(out)
    }

    /// Bring one calendar level, if its ctag says it moved.
    async fn sync_calendar(
        &self,
        cal: &mut CachedCalendar,
        ctag: Option<String>,
    ) -> Result<bool, SyncError> {
        if ctag.is_some() && cal.sync_token == ctag {
            return Ok(false);
        }
        let etags = self.etags(&cal.id).await?;
        let mut changed = false;
        let before = cal.resources.len();
        cal.resources.retain(|href, _| etags.contains_key(href));
        changed |= cal.resources.len() != before;
        let stale: Vec<String> = etags
            .iter()
            .filter(|(href, etag)| {
                cal.resources.get(*href).and_then(|r| r.etag.as_ref()) != Some(*etag)
            })
            .map(|(href, _)| href.clone())
            .collect();
        for batch in stale.chunks(BATCH) {
            for (href, etag, data) in self.multiget(&cal.id, batch).await? {
                cal.resources.insert(
                    href.clone(),
                    Resource {
                        etag: etag.or_else(|| etags.get(&href).cloned()),
                        items: ical::parse_events(&data),
                    },
                );
                changed = true;
            }
        }
        cal.sync_token = ctag;
        Ok(changed)
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

impl Provider for CalDav<'_> {
    async fn sync(&mut self, cache: &mut AccountCache) -> Result<bool, SyncError> {
        if self.login.home.is_none() {
            self.discover().await?;
        }
        let home = self.login.home.clone().unwrap_or_default();
        let listed = self.calendars(&home).await?;
        let mut changed = false;
        let mut calendars = Vec::with_capacity(listed.len());
        let kept: HashSet<&String> = listed.iter().map(|(url, ..)| url).collect();
        changed |= cache.calendars.iter().any(|c| !kept.contains(&c.id));
        for (url, name, color, ctag) in listed {
            let mut cal = cache
                .calendars
                .iter()
                .find(|c| c.id == url)
                .cloned()
                .unwrap_or_else(|| CachedCalendar {
                    id: url.clone(),
                    selected: true,
                    ..Default::default()
                });
            if (&cal.name, &cal.color) != (&name, &color) {
                (cal.name, cal.color) = (name, color);
                changed = true;
            }
            changed |= self.sync_calendar(&mut cal, ctag).await?;
            calendars.push(cal);
        }
        calendars.sort_by(|a, b| a.name.cmp(&b.name));
        cache.calendars = calendars;
        Ok(changed)
    }
}
