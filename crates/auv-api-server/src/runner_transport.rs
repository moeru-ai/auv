//! Private local transport for daemon-owned AUV Runner processes.
//!
//! This crate deliberately does not define a private Runner control protocol.
//! A Runner serves its own gRPC services plus standard Health and Reflection;
//! the daemon owns routing, admission, process lifetime, and active-call
//! accounting.

use std::future::Future;
#[cfg(unix)]
use std::os::fd::{FromRawFd as _, RawFd};
use std::pin::Pin;
use std::task::{Context, Poll};

#[cfg(any(unix, windows))]
use tokio_stream::StreamExt as _;

/// Environment variable naming the inherited Runner IPC file descriptor.
pub const RUNNER_IPC_FD_ENV: &str = "AUV_RUNNER_IPC_FD";
#[cfg(windows)]
/// Environment variable naming the daemon-created Runner named pipe.
pub const RUNNER_IPC_PIPE_ENV: &str = "AUV_RUNNER_IPC_PIPE";
#[cfg(unix)]
/// Fixed file descriptor used for inherited Runner IPC on Unix.
pub const RUNNER_IPC_FD: RawFd = 3;

#[cfg(unix)]
/// Connected Runner IPC stream that signals when its parent side disconnects.
pub struct InheritedStream {
  inner: tokio::net::UnixStream,
  disconnected: Option<tokio::sync::oneshot::Sender<()>>,
}

#[cfg(unix)]
impl tokio::io::AsyncRead for InheritedStream {
  fn poll_read(mut self: Pin<&mut Self>, context: &mut Context<'_>, buffer: &mut tokio::io::ReadBuf<'_>) -> Poll<std::io::Result<()>> {
    Pin::new(&mut self.inner).poll_read(context, buffer)
  }
}

#[cfg(unix)]
impl tokio::io::AsyncWrite for InheritedStream {
  fn poll_write(mut self: Pin<&mut Self>, context: &mut Context<'_>, buffer: &[u8]) -> Poll<Result<usize, std::io::Error>> {
    Pin::new(&mut self.inner).poll_write(context, buffer)
  }

  fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
    Pin::new(&mut self.inner).poll_flush(context)
  }

  fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
    Pin::new(&mut self.inner).poll_shutdown(context)
  }
}

#[cfg(unix)]
impl Drop for InheritedStream {
  fn drop(&mut self) {
    if let Some(disconnected) = self.disconnected.take() {
      let _ = disconnected.send(());
    }
  }
}

#[cfg(unix)]
impl tonic::transport::server::Connected for InheritedStream {
  type ConnectInfo = ();

  fn connect_info(&self) -> Self::ConnectInfo {}
}

#[cfg(windows)]
/// Connected Runner IPC stream that signals when its parent side disconnects.
pub struct InheritedStream {
  inner: tokio::net::windows::named_pipe::NamedPipeClient,
  disconnected: Option<tokio::sync::oneshot::Sender<()>>,
}

#[cfg(windows)]
impl tokio::io::AsyncRead for InheritedStream {
  fn poll_read(mut self: Pin<&mut Self>, context: &mut Context<'_>, buffer: &mut tokio::io::ReadBuf<'_>) -> Poll<std::io::Result<()>> {
    Pin::new(&mut self.inner).poll_read(context, buffer)
  }
}

#[cfg(windows)]
impl tokio::io::AsyncWrite for InheritedStream {
  fn poll_write(mut self: Pin<&mut Self>, context: &mut Context<'_>, buffer: &[u8]) -> Poll<Result<usize, std::io::Error>> {
    Pin::new(&mut self.inner).poll_write(context, buffer)
  }

  fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
    Pin::new(&mut self.inner).poll_flush(context)
  }

  fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
    Pin::new(&mut self.inner).poll_shutdown(context)
  }
}

#[cfg(windows)]
impl Drop for InheritedStream {
  fn drop(&mut self) {
    if let Some(disconnected) = self.disconnected.take() {
      let _ = disconnected.send(());
    }
  }
}

#[cfg(windows)]
impl tonic::transport::server::Connected for InheritedStream {
  type ConnectInfo = ();

  fn connect_info(&self) -> Self::ConnectInfo {}
}

/// One adopted daemon connection and a shutdown signal that resolves when the
/// parent side disconnects.
#[cfg(any(unix, windows))]
pub struct InheritedTransport {
  stream: InheritedStream,
  parent_disconnected: tokio::sync::oneshot::Receiver<()>,
}

#[cfg(any(unix, windows))]
impl InheritedTransport {
  /// Splits the transport into tonic's incoming stream and a parent-disconnect
  /// shutdown signal.
  pub fn into_parts(
    self,
  ) -> (impl tokio_stream::Stream<Item = Result<InheritedStream, std::io::Error>> + Send + 'static, impl Future<Output = ()> + Send + 'static)
  {
    let incoming = tokio_stream::iter([Ok::<_, std::io::Error>(self.stream)]).chain(tokio_stream::pending());
    let shutdown = async move {
      let _ = self.parent_disconnected.await;
    };
    (incoming, shutdown)
  }
}

/// Adopts the connected local stream supplied by the parent daemon.
#[cfg(unix)]
pub fn inherited_transport() -> Result<InheritedTransport, String> {
  let fd = std::env::var(RUNNER_IPC_FD_ENV)
    .map_err(|_| format!("{RUNNER_IPC_FD_ENV} is required"))?
    .parse::<RawFd>()
    .map_err(|error| format!("invalid {RUNNER_IPC_FD_ENV}: {error}"))?;
  if fd != RUNNER_IPC_FD {
    return Err(format!("{RUNNER_IPC_FD_ENV} must name inherited descriptor {RUNNER_IPC_FD}"));
  }
  // SAFETY: dup returns a new descriptor owned by this call. The original
  // inherited descriptor remains owned by the process bootstrap contract.
  let owned_fd = unsafe { libc::dup(fd) };
  if owned_fd == -1 {
    return Err(format!("failed to duplicate inherited Runner descriptor: {}", std::io::Error::last_os_error()));
  }
  // SAFETY: owned_fd is the fresh descriptor returned by dup and has not been
  // transferred elsewhere.
  let stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(owned_fd) };
  stream.set_nonblocking(true).map_err(|error| format!("failed to configure inherited Runner stream: {error}"))?;
  let stream = tokio::net::UnixStream::from_std(stream).map_err(|error| format!("failed to adopt inherited Runner stream: {error}"))?;
  let (disconnected, parent_disconnected) = tokio::sync::oneshot::channel();
  Ok(InheritedTransport {
    stream: InheritedStream {
      inner: stream,
      disconnected: Some(disconnected),
    },
    parent_disconnected,
  })
}

/// Opens the private named pipe created by the parent daemon.
#[cfg(windows)]
pub fn inherited_transport() -> Result<InheritedTransport, String> {
  let pipe = std::env::var_os(RUNNER_IPC_PIPE_ENV).ok_or_else(|| format!("{RUNNER_IPC_PIPE_ENV} is required"))?;
  let stream = tokio::net::windows::named_pipe::ClientOptions::new()
    .open(&pipe)
    .map_err(|error| format!("failed to open inherited Runner named pipe {}: {error}", std::path::Path::new(&pipe).display()))?;
  let (disconnected, parent_disconnected) = tokio::sync::oneshot::channel();
  Ok(InheritedTransport {
    stream: InheritedStream {
      inner: stream,
      disconnected: Some(disconnected),
    },
    parent_disconnected,
  })
}

#[cfg(not(any(unix, windows)))]
pub fn inherited_transport() -> Result<(), String> {
  Err("the inherited Runner transport is not supported on this platform".to_string())
}
