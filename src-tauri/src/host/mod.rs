//! The Host: the long-lived process that owns this machine's Agents, so they
//! keep working whether or not a window is open (ADR 0009). It is this same
//! binary run with `--host`, and it never opens a window.
//!
//! Windows reach it over a Unix socket in the state directory, or a named
//! pipe named after it on Windows (see [`local`]) — the same one, and so the
//! same Host, as long as they agree on the state directory. A connection
//! carries calls one way and replies and events the other (see [`protocol`]);
//! every connection hears every Agent's events.
//!
//! The window's half lives here too: [`client`] is how a window talks to a
//! Host, and [`service`] is how it starts one that isn't running.

mod calls;
pub mod client;
pub mod hosts;
pub mod local;
pub mod protocol;
pub mod service;
pub mod tailnet;

#[cfg(test)]
mod tests;

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc, watch};

use crate::domain::new_id;
use crate::paths;
use crate::runtime::{self, AgentRuntime, RuntimeEvent};
use protocol::{line, Frame, Hello, Request};
use tailnet::Vet;

/// Where a connection came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Peer {
    /// This machine, over the Host's socket.
    Local,
    /// Another of the user's machines, over the tailnet.
    Remote(std::net::SocketAddr),
}

/// How many event lines a connection may fall behind before it is dropped.
/// A window that lags this far is better off reconnecting and reading the
/// logs afresh than being fed a backlog.
const EVENT_BACKLOG: usize = 4096;

/// How often a Host waiting to update checks whether its Turns have ended.
const IDLE_POLL: Duration = Duration::from_secs(1);

/// How often a Host not yet listening on the tailnet tries again.
const TAILNET_RETRY: Duration = Duration::from_secs(10);

pub struct Host {
    runtime: AgentRuntime,
    /// Ids of the Orphans adopted when this Host started, so a window can
    /// surface them. Emptied once the user dismisses the banner, so another
    /// window, or a reload, doesn't raise it again.
    startup_orphans: Mutex<Vec<String>>,
    /// Every event, as the line each connection writes. Connections subscribe.
    frames: broadcast::Sender<Arc<str>>,
    hello: Hello,
    /// Set once a window has asked this Host to make way for a newer build.
    draining: AtomicBool,
    /// Flipped to `true` to make [`serve`] return.
    stopping: Arc<watch::Sender<bool>>,
    /// The notes folder (ADR 0017), whose changes go out as `notes-changed`.
    notes: Arc<crate::notes::Notes>,
}

impl Host {
    /// A Host over `runtime`, relaying what it reports to every connection.
    pub fn new(
        runtime: AgentRuntime,
        mut events: mpsc::UnboundedReceiver<RuntimeEvent>,
        startup_orphans: Vec<String>,
    ) -> Arc<Self> {
        let (frames, _) = broadcast::channel(EVENT_BACKLOG);
        let relay = frames.clone();
        tokio::spawn(async move {
            while let Some(ev) = events.recv().await {
                let frame = match ev {
                    RuntimeEvent::AgentEvent { agent_id, event } => Frame::Event {
                        name: "agent-event".into(),
                        payload: json!({ "agent_id": agent_id, "event": event }),
                    },
                    RuntimeEvent::StateChanged { agent, .. } => Frame::Event {
                        name: "agent-state-changed".into(),
                        payload: serde_json::to_value(*agent).unwrap_or_default(),
                    },
                };
                // No subscribers is fine: nobody is watching right now.
                let _ = relay.send(Arc::from(line(&frame)));
            }
        });
        // Mail's own events (ADR 0017), relayed the same way.
        let relay = frames.clone();
        let mut mail = crate::mail::events();
        tokio::spawn(async move {
            while let Ok(payload) = mail.recv().await {
                let frame = Frame::Event {
                    name: "mail-changed".into(),
                    payload,
                };
                let _ = relay.send(Arc::from(line(&frame)));
            }
        });
        let notes = {
            let relay = frames.clone();
            crate::notes::Notes::new(move |paths| {
                let frame = Frame::Event {
                    name: "notes-changed".into(),
                    payload: json!({ "paths": paths }),
                };
                let _ = relay.send(Arc::from(line(&frame)));
            })
        };
        Arc::new(Self {
            runtime,
            startup_orphans: Mutex::new(startup_orphans),
            frames,
            hello: Hello::new(new_id()),
            draining: AtomicBool::new(false),
            stopping: Arc::new(watch::channel(false).0),
            notes,
        })
    }

    /// Stop serving: [`serve`] Orphans whatever is still running and returns.
    pub fn stop(&self) {
        self.stopping.send_replace(true);
    }

    /// Exit the moment no Turn is running, taking no new ones from then on.
    /// Asked by a window from a newer build, so the service can start that
    /// build instead — without cutting off any Agent mid-Turn to do it.
    fn shut_down_when_idle(&self) {
        if self.draining.swap(true, Ordering::SeqCst) {
            return;
        }
        eprintln!("host: a newer build asked to take over; exiting once no turn is running");
        let runtime = self.runtime.clone();
        let stopping = self.stopping.clone();
        tokio::spawn(async move {
            while !runtime.close_if_idle().await {
                tokio::time::sleep(IDLE_POLL).await;
            }
            stopping.send_replace(true);
        });
    }
}

/// Accept windows until the Host is stopped, then Orphan any Agent still
/// working, as a closing app used to: this machine's on `local`, and other
/// machines' on the listener `remote` gives, if it gives one, each let in only
/// if its `Vet` says so.
pub async fn serve<R>(host: Arc<Host>, mut local: local::Listener, remote: R)
where
    R: Future<Output = Option<(TcpListener, Vet)>> + Send + 'static,
{
    let mut stopping = host.stopping.subscribe();
    let remote = tokio::spawn(serve_remote(host.clone(), remote));
    loop {
        tokio::select! {
            accepted = local.accept() => match accepted {
                Ok(stream) => {
                    tokio::spawn(connection(host.clone(), stream, Peer::Local));
                }
                Err(e) => eprintln!("host: could not accept a window: {e}"),
            },
            _ = stopping.wait_for(|s| *s) => break,
        }
    }
    remote.abort();
    host.runtime.shutdown().await;
}

/// Accept windows from the user's other machines, once `remote` has a
/// listener for them.
async fn serve_remote(host: Arc<Host>, remote: impl Future<Output = Option<(TcpListener, Vet)>>) {
    let Some((tcp, vet)) = remote.await else {
        return;
    };
    loop {
        match tcp.accept().await {
            Ok((stream, peer)) => {
                tokio::spawn(admit(host.clone(), stream, peer, vet.clone()));
            }
            Err(e) => eprintln!("host: could not accept a window: {e}"),
        }
    }
}

/// Let a window from another machine in, if `vet` says it's the user's own;
/// otherwise tell it why not and hang up.
async fn admit(
    host: Arc<Host>,
    mut stream: tokio::net::TcpStream,
    peer: std::net::SocketAddr,
    vet: Vet,
) {
    match vet(peer).await {
        Ok(()) => connection(host, stream, Peer::Remote(peer)).await,
        Err(why) => {
            eprintln!("host: refused {peer}: {why}");
            let _ = stream
                .write_all(line(&Frame::Refused(why)).as_bytes())
                .await;
        }
    }
}

/// One window's connection, from its Hello to its hanging up.
async fn connection<S>(host: Arc<Host>, stream: S, peer: Peer)
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let (read, mut write) = tokio::io::split(stream);
    let (out, mut outgoing) = mpsc::unbounded_channel::<Arc<str>>();

    // Subscribed before the Hello goes out, so a window that reads the Agent
    // list on hearing it misses nothing that changes after.
    let mut events = host.frames.subscribe();
    let _ = out.send(Arc::from(line(&Frame::Hello(host.hello.clone()))));

    let writer = tokio::spawn(async move {
        while let Some(l) = outgoing.recv().await {
            if write.write_all(l.as_bytes()).await.is_err() {
                break;
            }
        }
    });

    let forward = {
        let out = out.clone();
        async move {
            loop {
                match events.recv().await {
                    Ok(l) => {
                        if out.send(l).is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        eprintln!("host: a window fell {n} events behind; dropping it");
                        break;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    };

    let answer = {
        let host = host.clone();
        async move {
            let mut lines = BufReader::new(read).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                let req = match serde_json::from_str::<Request>(&l) {
                    Ok(req) => req,
                    Err(e) => {
                        eprintln!("host: unreadable request: {e}");
                        continue;
                    }
                };
                // Each call on its own task: a Stop waiting on its supervisor
                // must not hold up the window's next call.
                let host = host.clone();
                let out = out.clone();
                tokio::spawn(async move {
                    let outcome = calls::answer(&host, req.call, peer).await.into();
                    let _ = out.send(Arc::from(line(&Frame::Reply {
                        id: req.id,
                        outcome,
                    })));
                });
            }
        }
    };

    let mut stopping = host.stopping.subscribe();
    tokio::select! {
        _ = forward => {},
        _ = answer => {},
        _ = stopping.wait_for(|s| *s) => {},
    }
    writer.abort();
}

/// `--host`: run this machine's Host until it is told to stop.
pub fn run() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("could not start the async runtime");
    let code = runtime.block_on(main());
    std::process::exit(code);
}

async fn main() -> i32 {
    match start().await {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("host: {e}");
            1
        }
    }
}

async fn start() -> Result<(), String> {
    paths::ensure_dirs().map_err(|e| e.to_string())?;
    let Some(_lock) = lock().map_err(|e| format!("could not take the Host lock: {e}"))? else {
        eprintln!("host: another Host is already running for this state directory");
        return Ok(());
    };

    let socket = paths::host_socket().map_err(|e| e.to_string())?;
    let listener = local::Listener::bind(&socket)
        .map_err(|e| format!("could not listen on {}: {e}", socket.display()))?;

    let orphans = runtime::adopt_orphans_on_launch()
        .unwrap_or_default()
        .into_iter()
        .map(|a| a.id)
        .collect();
    let (rt, rx) = AgentRuntime::new();
    let host = Host::new(rt, rx, orphans);
    crate::mail::start();

    let on_signal = host.clone();
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let (Ok(mut term), Ok(mut int)) = (
                signal(SignalKind::terminate()),
                signal(SignalKind::interrupt()),
            ) else {
                return;
            };
            tokio::select! {
                _ = term.recv() => {},
                _ = int.recv() => {},
            }
        }
        #[cfg(windows)]
        if tokio::signal::ctrl_c().await.is_err() {
            return;
        }
        on_signal.stop();
    });

    eprintln!("host: listening on {}", socket.display());
    serve(host, listener, remote_listener()).await;
    let _ = std::fs::remove_file(&socket);
    Ok(())
}

/// Where windows on the user's other machines reach this Host: its tailnet
/// address. Waits for Tailscale when it isn't up yet — at login, or while the
/// user has it switched off — since its address can't be listened on until it
/// is, and a Host that gave up would stay unreachable until it restarted.
///
/// A debug build can instead be given `ORCHESTRATE_DEBUG_LISTEN=addr:port`,
/// which lets in *anyone* who can reach that address. It's for trying a second
/// Host on one machine; a release build ignores it.
async fn remote_listener() -> Option<(TcpListener, Vet)> {
    if cfg!(debug_assertions) {
        if let Ok(addr) = std::env::var("ORCHESTRATE_DEBUG_LISTEN") {
            let tcp = TcpListener::bind(&addr).await.ok()?;
            eprintln!("host: DEBUG: letting anyone in on {addr}, unchecked");
            let open: Vet = Arc::new(|_| Box::pin(async { Ok(()) }));
            return Some((tcp, open));
        }
    }
    // Said once each time it changes, not on every try.
    let mut said = String::new();
    loop {
        let why = match tailnet::address().await {
            None => "Tailscale isn't running here; reachable from this machine only until it is"
                .to_owned(),
            Some(ip) => {
                let addr = std::net::SocketAddr::new(ip, tailnet::port());
                match TcpListener::bind(addr).await {
                    Ok(tcp) => {
                        eprintln!("host: listening for the user's other machines on {addr}");
                        return Some((tcp, tailnet::tailscale_vet()));
                    }
                    Err(e) => format!("could not listen on {addr}, will keep trying: {e}"),
                }
            }
        };
        if why != said {
            eprintln!("host: {why}");
            said = why;
        }
        tokio::time::sleep(TAILNET_RETRY).await;
    }
}

/// Take the state directory's Host lock, or `None` if another Host holds it.
/// The lock lasts as long as the returned file stays open.
fn lock() -> std::io::Result<Option<std::fs::File>> {
    let path = paths::host_lock().map_err(std::io::Error::other)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(e),
    }
}
