//! A window's link to its Host: connects (starting the Host if nothing is
//! listening), passes the webview's calls through, hands the Host's events back
//! to the webview, and reconnects whenever the Host goes away — as it does on
//! every update.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, oneshot, watch};

use super::protocol::{build_id, Frame, Hello, Request, PROTOCOL};

/// How long a call waits for a Host to connect before giving up.
const READY_TIMEOUT: Duration = Duration::from_secs(15);

/// How long a freshly started Host gets to open its socket.
const START_TIMEOUT: Duration = Duration::from_secs(10);

const LOST: &str = "lost the connection to the Host";

/// Where the window stands with its Host, as the webview shows it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Status {
    /// Not connected yet, or reconnecting. `error` says why the last attempt
    /// failed, if one has.
    Connecting {
        error: Option<String>,
    },
    Connected {
        instance: String,
        version: String,
    },
    /// The Host is an older build this window can't talk to. It has been asked
    /// to make way, and will once its running Turns end.
    Updating {
        version: String,
    },
    /// The Host is a newer build than this window. The window is the one to
    /// update, so the Host is left alone.
    Outdated {
        version: String,
    },
}

/// Hands a Host event to the webview, by event name.
pub type Notify = Arc<dyn Fn(&str, Value) + Send + Sync>;

/// Starts a Host for this window's state directory when none is listening.
pub type Start = Arc<dyn Fn() -> io::Result<()> + Send + Sync>;

#[derive(Clone)]
pub struct HostLink {
    inner: Arc<Inner>,
}

struct Inner {
    socket: PathBuf,
    start: Start,
    notify: Notify,
    status: watch::Sender<Status>,
    conn: watch::Sender<Option<Arc<Conn>>>,
    next_id: AtomicU64,
}

/// Where a call's answer goes.
type Reply = oneshot::Sender<Result<Value, String>>;

/// One live connection.
struct Conn {
    out: mpsc::UnboundedSender<String>,
    /// Calls waiting on a reply, by request id. `None` once the connection is
    /// gone, so a call can't wait on a reply that will never come.
    pending: Mutex<Option<HashMap<u64, Reply>>>,
}

impl Conn {
    fn wait_for(&self, id: u64) -> Result<oneshot::Receiver<Result<Value, String>>, String> {
        let (tx, rx) = oneshot::channel();
        match self.pending.lock().unwrap().as_mut() {
            Some(pending) => {
                pending.insert(id, tx);
                Ok(rx)
            }
            None => Err(LOST.into()),
        }
    }

    fn reply(&self, id: u64, result: Result<Value, String>) {
        let waiting = self
            .pending
            .lock()
            .unwrap()
            .as_mut()
            .and_then(|p| p.remove(&id));
        if let Some(tx) = waiting {
            let _ = tx.send(result);
        }
    }

    /// Fail every call still waiting, and any made from now on.
    fn close(&self) {
        let waiting = self.pending.lock().unwrap().take().unwrap_or_default();
        for (_, tx) in waiting {
            let _ = tx.send(Err(LOST.into()));
        }
    }
}

fn request(id: u64, method: &str, args: Value) -> String {
    let mut s = serde_json::to_string(&Request {
        id,
        call: json!({ "method": method, "args": args }),
    })
    .expect("requests always serialize");
    s.push('\n');
    s
}

/// A request whose reply nobody waits for.
const UNANSWERED: u64 = 0;

impl HostLink {
    /// A link to the Host listening on `socket`, not yet connected: [`run`]
    /// does that. `start` is called when nothing is listening.
    ///
    /// [`run`]: Self::run
    pub fn new(socket: PathBuf, start: Start, notify: Notify) -> Self {
        Self {
            inner: Arc::new(Inner {
                socket,
                start,
                notify,
                status: watch::channel(Status::Connecting { error: None }).0,
                conn: watch::channel(None).0,
                next_id: AtomicU64::new(UNANSWERED + 1),
            }),
        }
    }

    pub fn status(&self) -> Status {
        self.inner.status.borrow().clone()
    }

    /// Stay connected for as long as the window lives.
    pub async fn run(self) {
        let mut backoff = Duration::from_millis(200);
        loop {
            match self.connect().await {
                Ok(stream) => {
                    backoff = Duration::from_millis(200);
                    self.session(stream).await;
                    self.set_status(Status::Connecting { error: None });
                }
                Err(error) => {
                    self.set_status(Status::Connecting { error: Some(error) });
                    backoff = (backoff * 2).min(Duration::from_secs(5));
                }
            }
            tokio::time::sleep(backoff).await;
        }
    }

    /// Ask the Host something, as `{"method": method, "args": args}`.
    pub async fn call(&self, method: &str, args: Value) -> Result<Value, String> {
        let conn = self.ready().await?;
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let reply = conn.wait_for(id)?;
        if conn.out.send(request(id, method, args)).is_err() {
            conn.reply(id, Err(LOST.into()));
        }
        reply.await.unwrap_or_else(|_| Err(LOST.into()))
    }

    /// The live connection, waiting a while for one if the Host is between
    /// connections. Fails at once when waiting can't help.
    async fn ready(&self) -> Result<Arc<Conn>, String> {
        match self.status() {
            Status::Updating { .. } => {
                return Err(
                    "the Host is restarting to update once its running agents finish; \
                     try again then"
                        .into(),
                )
            }
            Status::Outdated { version } => {
                return Err(format!(
                    "the Host runs a newer version ({version}); update this app to use it"
                ))
            }
            _ => {}
        }
        let mut conn = self.inner.conn.subscribe();
        let waited = tokio::time::timeout(READY_TIMEOUT, conn.wait_for(Option::is_some)).await;
        match waited {
            Ok(Ok(c)) => Ok(c.clone().expect("waited for a connection")),
            _ => Err(match self.status() {
                Status::Connecting { error: Some(e) } => format!("can't reach the Host: {e}"),
                _ => "can't reach the Host".into(),
            }),
        }
    }

    fn set_status(&self, status: Status) {
        if *self.inner.status.borrow() == status {
            return;
        }
        let payload = serde_json::to_value(&status).unwrap_or_default();
        // Stored first, so a webview that asks on hearing this gets the same.
        self.inner.status.send_replace(status);
        (self.inner.notify)("host-status", payload);
    }

    /// Connect to the Host, starting it first if nothing is listening.
    async fn connect(&self) -> Result<UnixStream, String> {
        if let Ok(stream) = UnixStream::connect(&self.inner.socket).await {
            return Ok(stream);
        }
        let start = self.inner.start.clone();
        tokio::task::spawn_blocking(move || start())
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("could not start the Host: {e}"))?;

        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            match UnixStream::connect(&self.inner.socket).await {
                Ok(stream) => return Ok(stream),
                Err(e) if Instant::now() >= deadline => {
                    return Err(format!("the Host didn't start listening: {e}"))
                }
                Err(_) => {}
            }
        }
    }

    /// One connection, from the Host's Hello until it hangs up.
    async fn session(&self, stream: UnixStream) {
        let (read, mut write) = stream.into_split();
        let mut lines = BufReader::new(read).lines();
        let hello = match lines.next_line().await {
            Ok(Some(l)) => match serde_json::from_str::<Frame>(&l) {
                Ok(Frame::Hello(hello)) => hello,
                _ => return,
            },
            _ => return,
        };

        let (out, mut outgoing) = mpsc::unbounded_channel::<String>();
        let writer = tokio::spawn(async move {
            while let Some(l) = outgoing.recv().await {
                if write.write_all(l.as_bytes()).await.is_err() {
                    break;
                }
            }
        });

        match compare(&hello) {
            Fit::Same => {}
            // An older build: still one this window can talk to while it
            // finishes its Turns, then the service brings up ours.
            Fit::Older => {
                let _ = out.send(request(UNANSWERED, "shutdown_when_idle", json!({})));
            }
            Fit::Incompatible { ours_is_newer } => {
                if ours_is_newer {
                    let _ = out.send(request(UNANSWERED, "shutdown_when_idle", json!({})));
                    self.set_status(Status::Updating {
                        version: hello.version,
                    });
                } else {
                    self.set_status(Status::Outdated {
                        version: hello.version,
                    });
                }
                // Nothing to say to it; wait for it to go.
                while let Ok(Some(_)) = lines.next_line().await {}
                writer.abort();
                return;
            }
        }

        let conn = Arc::new(Conn {
            out,
            pending: Mutex::new(Some(HashMap::new())),
        });
        self.inner.conn.send_replace(Some(conn.clone()));
        self.set_status(Status::Connected {
            instance: hello.instance,
            version: hello.version,
        });

        while let Ok(Some(l)) = lines.next_line().await {
            match serde_json::from_str::<Frame>(&l) {
                Ok(Frame::Reply { id, outcome }) => conn.reply(id, outcome.into()),
                Ok(Frame::Event { name, payload }) => (self.inner.notify)(&name, payload),
                Ok(Frame::Hello(_)) | Err(_) => {}
            }
        }

        self.inner.conn.send_replace(None);
        conn.close();
        writer.abort();
    }
}

/// How a Host's build sits against this window's.
#[derive(Debug, PartialEq, Eq)]
enum Fit {
    Same,
    /// Same protocol, older binary.
    Older,
    /// A different protocol. Only the newer side asks the other to go, so two
    /// windows of different builds can't take turns restarting one Host.
    Incompatible {
        ours_is_newer: bool,
    },
}

fn compare(hello: &Hello) -> Fit {
    fit(hello, PROTOCOL, &build_id())
}

fn fit(hello: &Hello, protocol: u32, build: &str) -> Fit {
    let older = || hello.build.parse::<u128>().unwrap_or(0) < build.parse::<u128>().unwrap_or(0);
    if hello.protocol != protocol {
        Fit::Incompatible {
            ours_is_newer: hello.protocol < protocol,
        }
    } else if hello.build != build && older() {
        Fit::Older
    } else {
        Fit::Same
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello(protocol: u32, build: &str) -> Hello {
        Hello {
            protocol,
            version: "0.1.0".into(),
            build: build.into(),
            instance: "i".into(),
        }
    }

    #[test]
    fn same_build_fits() {
        assert_eq!(fit(&hello(1, "100"), 1, "100"), Fit::Same);
    }

    #[test]
    fn an_older_build_is_asked_to_make_way() {
        assert_eq!(fit(&hello(1, "99"), 1, "100"), Fit::Older);
    }

    #[test]
    fn a_newer_host_is_left_alone() {
        assert_eq!(fit(&hello(1, "101"), 1, "100"), Fit::Same);
    }

    #[test]
    fn a_different_protocol_names_the_side_to_update() {
        assert_eq!(
            fit(&hello(1, "99"), 2, "100"),
            Fit::Incompatible {
                ours_is_newer: true
            }
        );
        assert_eq!(
            fit(&hello(3, "101"), 2, "100"),
            Fit::Incompatible {
                ours_is_newer: false
            }
        );
    }
}
