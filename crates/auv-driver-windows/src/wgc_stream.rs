//! A continuously drained Windows.Graphics.Capture stream for one capture item.
//!
//! A frame pool is a FIFO: it hands out its oldest queued frame, and once its buffers are full
//! the compositor drops newer frames. The former on-demand pool returned pre-click and intermediate
//! frames before converging on the third read. See `2026-10-10-windows-background-session.md`.
//! Both background sessions and on-demand window/display images now use this continuously drained
//! stream, so an idle caller does not fill the pool with old frames.
//!
//! A [`FrameStream`] drains its pool the moment each frame arrives, keeps only the newest
//! frame as a GPU texture, and counts it. A read copies that newest frame to the CPU, or answers
//! [`FrameUpdate::Unchanged`] when nothing has arrived since the previous read and copies nothing.
//! This is the latest received frame; it does not prove that a pending application action has
//! already painted or that a frame was produced after a request began. A background session keeps
//! one stream per window and need not renegotiate when switching readers. Minimized windows deliver
//! no frames; the caller checks for that before reading and never restores them.
//!
//! A stream is bound to the window's capture region as it was when the stream opened. When that
//! region changes size, the stream stops with [`StreamError::Resized`] and its owner opens a new
//! one, because recreating the frame pool in place keeps the old origin (see `Shared::store`).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use auv_driver_common::error::DriverResult;
use image::RgbaImage;
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D11::{
  D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_TEXTURE2D_DESC, D3D11_USAGE, D3D11_USAGE_DEFAULT,
  D3D11_USAGE_STAGING, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess;
use windows::core::{HRESULT, Interface};

use super::native::{D3dContext, get_or_init_d3d_context, item_for_window};
use super::{FrameUpdate, StreamError};
use crate::error::{backend, invalid_input};

/// Buffers in the frame pool. The drain keeps the pool nearly empty, so this is headroom for a
/// frame that arrives while the previous one is still being copied.
const BUFFERS: i32 = 3;

struct Shared {
  d3d: Arc<D3dContext>,
  /// Frames stored so far; each replaced the newest. Bumped under `inner`.
  frames: AtomicU64,
  closed: AtomicBool,
  /// The capture region changed size; no frame after that is stored.
  resized: AtomicBool,
  failure: Mutex<Option<String>>,
  inner: Mutex<Inner>,
  first_frame: Condvar,
}

struct Inner {
  /// Content size the pool is currently sized for.
  size: SizeInt32,
  newest: Option<ID3D11Texture2D>,
  newest_size: (u32, u32),
  staging: Option<ID3D11Texture2D>,
  /// Frame number the previous read returned.
  delivered: u64,
}

// SAFETY: the D3D11 device and the textures made from it are free-threaded. The immediate context
// is not, and every use of it goes through `D3dContext::context`'s mutex. The rest is atomics and
// mutexes.
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

pub struct FrameStream {
  shared: Arc<Shared>,
  frame_pool: Direct3D11CaptureFramePool,
  session: GraphicsCaptureSession,
}

// SAFETY: WinRT capture objects are agile and thread-safe; the stream's mutable state is behind
// `Shared`'s locks.
unsafe impl Send for FrameStream {}
unsafe impl Sync for FrameStream {}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Shared {
  fn fail(&self, message: String) {
    let mut failure = lock(&self.failure);
    if failure.is_none() {
      *failure = Some(message);
    }
    drop(failure);
    self.first_frame.notify_all();
  }

  /// Runs on a capture thread-pool thread each time the compositor delivers a frame.
  fn on_frame_arrived(&self, pool: &Direct3D11CaptureFramePool) {
    // Take everything queued and keep the newest: an older frame is already out of date, and
    // dropping it returns its buffer to the pool.
    let mut newest: Option<Direct3D11CaptureFrame> = None;
    loop {
      match pool.TryGetNextFrame() {
        Ok(frame) => newest = Some(frame),
        // An empty pool comes back as S_OK with a null frame, which windows-rs reports as this.
        Err(error) if error.code() == HRESULT(0) => break,
        Err(error) => {
          self.fail(format!("TryGetNextFrame failed: {error}"));
          return;
        }
      }
    }
    if let Some(frame) = newest
      && let Err(message) = self.store(&frame)
    {
      self.fail(message);
    }
  }

  fn texture(&self, width: u32, height: u32, usage: D3D11_USAGE, cpu_access: u32) -> Result<ID3D11Texture2D, String> {
    let description = D3D11_TEXTURE2D_DESC {
      Width: width,
      Height: height,
      MipLevels: 1,
      ArraySize: 1,
      Format: DXGI_FORMAT_B8G8R8A8_UNORM,
      SampleDesc: DXGI_SAMPLE_DESC {
        Count: 1,
        Quality: 0,
      },
      Usage: usage,
      BindFlags: 0,
      CPUAccessFlags: cpu_access,
      MiscFlags: 0,
    };
    let mut texture = None;
    // SAFETY: a valid description and a writable out parameter; the device is free-threaded.
    unsafe { self.d3d.device.CreateTexture2D(&description, None, Some(&mut texture)) }
      .map_err(|error| format!("CreateTexture2D failed: {error}"))?;
    texture.ok_or_else(|| "CreateTexture2D returned no texture".to_string())
  }

  fn store(&self, frame: &Direct3D11CaptureFrame) -> Result<(), String> {
    let content = frame.ContentSize().map_err(|error| format!("ContentSize failed: {error}"))?;
    let surface = frame.Surface().map_err(|error| format!("frame Surface failed: {error}"))?;
    let access: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(|error| format!("surface is not a DXGI interface: {error}"))?;
    // SAFETY: the surface wraps a D3D11 texture, which is what this interface yields.
    let source: ID3D11Texture2D = unsafe { access.GetInterface() }.map_err(|error| format!("no texture behind the frame: {error}"))?;
    let mut description = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: a live texture and a writable description.
    unsafe { source.GetDesc(&mut description) };
    // HDR tonemapping guard: fail explicitly on non-B8G8R8A8 formats.
    if description.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
      return Err(format!("unsupported pixel format {:?}: window frames are expected as B8G8R8A8", description.Format));
    }

    let inner = lock(&self.inner);
    // NOTICE: a changed content size ends the stream instead of recreating the pool. Measured on
    // Windows 11 (`background-session-eval geometry`): a window whose DWM frame bounds first
    // covered its whole rectangle and, after a move, only its visible frame kept arriving from the
    // old origin after `Recreate` (the visible left edge at x = 7, the right edge cut off), while a
    // capture session opened at that moment lined up. Frames that do not line up with the window's
    // frame send every point read off them to the wrong place, so the owner opens a new stream.
    // Removal condition: `Recreate` keeping frames aligned with the window's current frame bounds
    // on the supported Windows builds.
    if content.Width > 0 && content.Height > 0 && (content.Width != inner.size.Width || content.Height != inner.size.Height) {
      drop(inner);
      self.resized.store(true, Ordering::Release);
      self.first_frame.notify_all();
      return Ok(());
    }
    let mut inner = inner;
    let size = (description.Width, description.Height);
    if inner.newest.is_none() || inner.newest_size != size {
      inner.newest = Some(self.texture(size.0, size.1, D3D11_USAGE_DEFAULT, 0)?);
      inner.newest_size = size;
      inner.staging = None;
    }
    let newest = inner.newest.as_ref().expect("newest texture was just ensured");
    let context = lock(&self.d3d.context);
    // SAFETY: both textures are live, the same size and format; the context mutex is held.
    unsafe { context.CopyResource(newest, &source) };
    drop(context);
    self.frames.fetch_add(1, Ordering::Release);
    drop(inner);
    self.first_frame.notify_all();
    Ok(())
  }
}

impl FrameStream {
  /// Starts observing `hwnd`. The window must have a size: a minimized window has none.
  pub fn open(hwnd: HWND) -> DriverResult<Self> {
    let d3d = get_or_init_d3d_context()?;
    let item = item_for_window(hwnd)?;
    Self::open_item(d3d, &item)
  }

  /// Shares pool draining and GPU/CPU staging with on-demand window and display capture.
  /// The context is resolved before taking the on-demand cache lock, preserving reset lock order.
  pub(super) fn open_item(d3d: Arc<D3dContext>, item: &GraphicsCaptureItem) -> DriverResult<Self> {
    let size = item.Size().map_err(|error| backend(format!("failed to read the capture item size: {error}")))?;
    if size.Width <= 0 || size.Height <= 0 {
      return Err(invalid_input(format!("target has zero or invalid dimensions ({}x{}); target may be minimized", size.Width, size.Height)));
    }
    let frame_pool =
      Direct3D11CaptureFramePool::CreateFreeThreaded(&d3d.winrt_device, DirectXPixelFormat::B8G8R8A8UIntNormalized, BUFFERS, size)
        .map_err(|error| backend(format!("failed to create the frame pool: {error}")))?;
    let shared = Arc::new(Shared {
      d3d,
      frames: AtomicU64::new(0),
      closed: AtomicBool::new(false),
      resized: AtomicBool::new(false),
      failure: Mutex::new(None),
      inner: Mutex::new(Inner {
        size,
        newest: None,
        newest_size: (0, 0),
        staging: None,
        delivered: 0,
      }),
      first_frame: Condvar::new(),
    });

    let arrived = shared.clone();
    // NOTICE: the registration token is not kept. The handler stays registered until the pool is
    // closed, which `Drop` does.
    frame_pool
      .FrameArrived(&TypedEventHandler::new(move |pool: &Option<Direct3D11CaptureFramePool>, _| {
        if let Some(pool) = pool {
          arrived.on_frame_arrived(pool);
        }
        Ok(())
      }))
      .map_err(|error| backend(format!("failed to register the FrameArrived handler: {error}")))?;
    let closing = shared.clone();
    item
      .Closed(&TypedEventHandler::new(move |_, _| {
        closing.closed.store(true, Ordering::Release);
        closing.first_frame.notify_all();
        Ok(())
      }))
      .map_err(|error| backend(format!("failed to register the Closed handler: {error}")))?;

    let session =
      frame_pool.CreateCaptureSession(item).map_err(|error| backend(format!("failed to create the capture session: {error}")))?;
    let _ = session.SetIsBorderRequired(false);
    let _ = session.SetIsCursorCaptureEnabled(false);
    session.StartCapture().map_err(|error| backend(format!("failed to start the capture session: {error}")))?;
    Ok(Self {
      shared,
      frame_pool,
      session,
    })
  }

  /// Frames the compositor has delivered so far. Constant while the window does not change.
  pub fn frames_delivered(&self) -> u64 {
    self.shared.frames.load(Ordering::Acquire)
  }

  fn check(&self) -> Result<(), StreamError> {
    if let Some(message) = lock(&self.shared.failure).clone() {
      return Err(StreamError::Failed(message));
    }
    if self.shared.closed.load(Ordering::Acquire) {
      return Err(StreamError::Closed);
    }
    if self.shared.resized.load(Ordering::Acquire) {
      return Err(StreamError::Resized);
    }
    Ok(())
  }

  /// Reads the newest frame if it is newer than the one the previous read returned. The very
  /// first read waits up to `first_frame_wait` for the compositor's first frame; later reads
  /// never wait.
  pub fn read(&self, first_frame_wait: Duration) -> Result<FrameUpdate, StreamError> {
    let deadline = Instant::now() + first_frame_wait;
    let mut inner = lock(&self.shared.inner);
    loop {
      self.check()?;
      if self.shared.frames.load(Ordering::Acquire) > 0 {
        break;
      }
      let now = Instant::now();
      if now >= deadline {
        return Err(StreamError::NoFrame);
      }
      inner = self.shared.first_frame.wait_timeout(inner, deadline - now).unwrap_or_else(|poisoned| poisoned.into_inner()).0;
    }
    self.check()?;
    let number = self.shared.frames.load(Ordering::Acquire);
    if number == inner.delivered {
      return Ok(FrameUpdate::Unchanged {
        frame_number: number,
      });
    }

    let (width, height) = inner.newest_size;
    if inner.staging.is_none() {
      let staging = self.shared.texture(width, height, D3D11_USAGE_STAGING, D3D11_CPU_ACCESS_READ.0 as u32).map_err(StreamError::Failed)?;
      inner.staging = Some(staging);
    }
    let newest = inner.newest.clone().ok_or(StreamError::NoFrame)?;
    let staging = inner.staging.clone().expect("staging texture was just ensured");
    let context = lock(&self.shared.d3d.context);
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    // SAFETY: both textures are live and the same size; the context mutex is held, so no other
    // thread uses the immediate context while the staging texture is mapped.
    unsafe {
      context.CopyResource(&staging, &newest);
      let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
      context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).map_err(|error| StreamError::Failed(format!("Map failed: {error}")))?;
      if mapped.pData.is_null() {
        context.Unmap(&staging, 0);
        return Err(StreamError::Failed("a mapped staging texture had no data".into()));
      }
      let pitch = mapped.RowPitch as usize;
      let row_bytes = width as usize * 4;
      for (row, target) in rgba.chunks_exact_mut(row_bytes).enumerate() {
        let source = std::slice::from_raw_parts((mapped.pData as *const u8).add(row * pitch), row_bytes);
        for (pixel, out) in source.as_chunks::<4>().0.iter().zip(target.as_chunks_mut::<4>().0) {
          // BGRA to RGBA: swap the first and third bytes.
          let value = u32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]);
          let swapped = (value & 0xFF00_FF00) | ((value << 16) & 0x00FF_0000) | ((value >> 16) & 0x0000_00FF);
          out.copy_from_slice(&swapped.to_le_bytes());
        }
      }
      context.Unmap(&staging, 0);
    }
    drop(context);
    inner.delivered = number;
    drop(inner);
    let image =
      RgbaImage::from_raw(width, height, rgba).ok_or_else(|| StreamError::Failed("the frame did not fit its image buffer".into()))?;
    Ok(FrameUpdate::Fresh {
      image,
      frame_number: number,
    })
  }
}

impl Drop for FrameStream {
  fn drop(&mut self) {
    let _ = self.session.Close();
    let _ = self.frame_pool.Close();
  }
}
