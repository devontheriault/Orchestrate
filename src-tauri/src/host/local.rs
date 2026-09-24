//! How a window reaches the Host on its own machine: a Unix socket in the
//! state directory, or on Windows, which has no Unix sockets tokio can use, a
//! named pipe named after it. Either way only this user can open it — anyone
//! who can runs Agents as them, and hears every Agent's output.

#[cfg(unix)]
pub use unix::{connect, Listener, Stream};
#[cfg(windows)]
pub use windows::{connect, Listener, Stream};

#[cfg(unix)]
mod unix {
    use std::io;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use tokio::net::{UnixListener, UnixStream};

    pub type Stream = UnixStream;

    pub struct Listener(UnixListener);

    impl Listener {
        pub fn bind(socket: &Path) -> io::Result<Self> {
            // Left behind by a Host that died without cleaning up. Safe to
            // remove: holding the Host lock means no live Host is listening.
            let _ = std::fs::remove_file(socket);
            let listener = UnixListener::bind(socket)?;
            std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600))?;
            Ok(Self(listener))
        }

        pub async fn accept(&mut self) -> io::Result<Stream> {
            self.0.accept().await.map(|(stream, _)| stream)
        }
    }

    pub async fn connect(socket: &Path) -> io::Result<Stream> {
        UnixStream::connect(socket).await
    }
}

#[cfg(windows)]
mod windows {
    use std::ffi::{c_void, OsString};
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
    use tokio::net::windows::named_pipe::{
        ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
    };
    use windows_sys::Win32::Foundation::{LocalFree, ERROR_PIPE_BUSY};
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;

    /// A pipe's default ACL lets every user on the machine read it. This one
    /// lets in its owner — the user who started the Host — and nobody else.
    const OWNER_ONLY: &str = "D:P(A;;GA;;;OW)";

    /// Either end of the pipe; the Host holds the server's, a window the
    /// client's.
    pub enum Stream {
        Server(NamedPipeServer),
        Client(NamedPipeClient),
    }

    pub struct Listener {
        name: OsString,
        /// The instance waiting for the next window.
        next: NamedPipeServer,
    }

    impl Listener {
        pub fn bind(name: &Path) -> io::Result<Self> {
            let name = name.as_os_str().to_owned();
            // The first instance must be ours: a pipe of this name already
            // there belongs to some other process, waiting to hear our calls.
            let next = create(&name, true)?;
            Ok(Self { name, next })
        }

        pub async fn accept(&mut self) -> io::Result<Stream> {
            self.next.connect().await?;
            let next = create(&self.name, false)?;
            Ok(Stream::Server(std::mem::replace(&mut self.next, next)))
        }
    }

    fn create(name: &OsString, first: bool) -> io::Result<NamedPipeServer> {
        let sddl: Vec<u16> = std::ffi::OsStr::new(OWNER_ONLY)
            .encode_wide()
            .chain(Some(0))
            .collect();
        let mut descriptor: *mut c_void = std::ptr::null_mut();
        // SAFETY: `sddl` is NUL-terminated, and the descriptor it allocates is
        // freed below, once the pipe has copied it.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        // SAFETY: `attributes` points at a valid descriptor for this call.
        let server = unsafe {
            ServerOptions::new()
                .first_pipe_instance(first)
                .create_with_security_attributes_raw(
                    name,
                    (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
                )
        };
        // SAFETY: allocated by the conversion above, and no longer used.
        unsafe { LocalFree(descriptor) };
        server
    }

    pub async fn connect(name: &Path) -> io::Result<Stream> {
        // Busy for the moment between a window connecting and the Host
        // opening the next instance.
        for _ in 0..20 {
            match ClientOptions::new().open(name) {
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                result => return result.map(Stream::Client),
            }
        }
        ClientOptions::new().open(name).map(Stream::Client)
    }

    impl AsyncRead for Stream {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> std::task::Poll<io::Result<()>> {
            match self.get_mut() {
                Stream::Server(s) => std::pin::Pin::new(s).poll_read(cx, buf),
                Stream::Client(s) => std::pin::Pin::new(s).poll_read(cx, buf),
            }
        }
    }

    impl AsyncWrite for Stream {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<io::Result<usize>> {
            match self.get_mut() {
                Stream::Server(s) => std::pin::Pin::new(s).poll_write(cx, buf),
                Stream::Client(s) => std::pin::Pin::new(s).poll_write(cx, buf),
            }
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<io::Result<()>> {
            match self.get_mut() {
                Stream::Server(s) => std::pin::Pin::new(s).poll_flush(cx),
                Stream::Client(s) => std::pin::Pin::new(s).poll_flush(cx),
            }
        }

        fn poll_shutdown(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<io::Result<()>> {
            match self.get_mut() {
                Stream::Server(s) => std::pin::Pin::new(s).poll_shutdown(cx),
                Stream::Client(s) => std::pin::Pin::new(s).poll_shutdown(cx),
            }
        }
    }
}
