//! Windows.Graphics.Capture (WGC) modern capture backend.
//!
//! Provides GPU-accelerated window and display capture using Direct3D 11
//! and `Windows.Graphics.Capture` WinRT APIs. Delivers CPU-mapped RGBA frames
//! with sub-10ms steady-state latency, coexisting with legacy GDI / PrintWindow
//! backends under the `"wgc.windows"` backend tag.

#[cfg(target_os = "windows")]
use std::time::{Duration, Instant};

use auv_driver_common::capture::{Capture, DisplayCapture};
use auv_driver_common::error::DriverResult;
use auv_driver_common::window::Window;
use serde::{Deserialize, Serialize};

#[cfg(target_os = "windows")]
use crate::error::backend;
#[cfg(target_os = "windows")]
use crate::window::window_handle;

pub const WGC_BACKEND: &str = "wgc.windows";

/// Result of a lightweight WGC window health check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowHealth {
  pub width: u32,
  pub height: u32,
  pub non_black_ratio: f64,
  pub is_fresh: bool,
  pub alive: bool,
}

#[cfg(target_os = "windows")]
mod native {
  use std::sync::mpsc::sync_channel;
  use std::sync::{Arc, Mutex, RwLock};
  use std::time::Duration;

  use super::*;
  use windows::Foundation::TypedEventHandler;
  use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem};
  use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
  use windows::Graphics::DirectX::DirectXPixelFormat;
  use windows::Win32::Foundation::{HWND, POINT};
  use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0};
  use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
  };
  use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
  use windows::Win32::Graphics::Dxgi::IDXGIDevice;
  use windows::Win32::Graphics::Gdi::{HMONITOR, MONITOR_DEFAULTTONEAREST, MonitorFromPoint};
  use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
  use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
  use windows::core::{Interface, factory};

  pub struct D3dContext {
    pub device: ID3D11Device,
    pub context: Mutex<ID3D11DeviceContext>,
    pub winrt_device: IDirect3DDevice,
  }

  // SAFETY: ID3D11Device and IDirect3DDevice are thread-safe COM interfaces in D3D11.
  // ID3D11DeviceContext is guarded by a Mutex.
  unsafe impl Send for D3dContext {}
  unsafe impl Sync for D3dContext {}

  static D3D_CONTEXT: RwLock<Option<Arc<D3dContext>>> = RwLock::new(None);

  pub const DXGI_ERROR_DEVICE_REMOVED_CODE: i32 = 0x887A0005_u32 as i32;
  pub const DXGI_ERROR_DEVICE_RESET_CODE: i32 = 0x887A0007_u32 as i32;

  #[inline]
  pub fn is_device_lost_hresult(hr: windows::core::HRESULT) -> bool {
    hr.0 == DXGI_ERROR_DEVICE_REMOVED_CODE || hr.0 == DXGI_ERROR_DEVICE_RESET_CODE
  }

  #[inline]
  pub fn is_device_lost_reason(reason: windows::core::Result<()>) -> bool {
    match reason {
      Ok(()) => false,
      Err(e) => is_device_lost_hresult(e.code()),
    }
  }

  pub fn reset_d3d_context() {
    if let Ok(mut lock) = D3D_CONTEXT.write() {
      *lock = None;
    }
    if let Ok(mut session) = ACTIVE_SESSION.lock() {
      *session = None;
    }
  }

  #[cfg(test)]
  pub fn is_d3d_context_initialized() -> bool {
    D3D_CONTEXT.read().map(|g| g.is_some()).unwrap_or(false)
  }

  pub fn check_device_lost_error(err: &windows::core::Error) {
    if is_device_lost_hresult(err.code()) {
      reset_d3d_context();
    }
  }

  pub(crate) fn get_or_init_d3d_context() -> DriverResult<Arc<D3dContext>> {
    if let Ok(guard) = D3D_CONTEXT.read()
      && let Some(ctx) = guard.as_ref()
    {
      return Ok(Arc::clone(ctx));
    }

    let mut guard = D3D_CONTEXT.write().map_err(|_| backend("d3d context rwlock poisoned"))?;
    if let Some(ref ctx) = *guard {
      return Ok(Arc::clone(ctx));
    }

    crate::desktop::ensure_input_desktop();

    let ctx = create_d3d_context()?;
    let arc = Arc::new(ctx);
    *guard = Some(Arc::clone(&arc));
    Ok(arc)
  }

  fn create_d3d_context() -> DriverResult<D3dContext> {
    unsafe {
      let mut d3d11_device: Option<ID3D11Device> = None;
      let mut d3d11_context: Option<ID3D11DeviceContext> = None;
      let mut feature_level = D3D_FEATURE_LEVEL_11_0;

      D3D11CreateDevice(
        None,
        D3D_DRIVER_TYPE_HARDWARE,
        None,
        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
        Some(&[D3D_FEATURE_LEVEL_11_0]),
        7,
        Some(&mut d3d11_device),
        Some(&mut feature_level),
        Some(&mut d3d11_context),
      )
      .map_err(|e| {
        check_device_lost_error(&e);
        backend(format!("D3D11CreateDevice failed: {e}"))
      })?;

      let d3d_device = d3d11_device.ok_or_else(|| backend("D3D11 device was None"))?;
      let d3d_context = d3d11_context.ok_or_else(|| backend("D3D11 context was None"))?;

      let dxgi_device: IDXGIDevice = d3d_device.cast().map_err(|e| backend(format!("failed to cast ID3D11Device to IDXGIDevice: {e}")))?;

      let inspectable = CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device)
        .map_err(|e| backend(format!("CreateDirect3D11DeviceFromDXGIDevice failed: {e}")))?;

      let winrt_device: IDirect3DDevice =
        inspectable.cast().map_err(|e| backend(format!("failed to cast inspectable to IDirect3DDevice: {e}")))?;

      Ok(D3dContext {
        device: d3d_device,
        context: Mutex::new(d3d_context),
        winrt_device,
      })
    }
  }

  struct CachedSession {
    target_id: isize,
    frame_pool: Direct3D11CaptureFramePool,
    session: windows::Graphics::Capture::GraphicsCaptureSession,
    size: windows::Graphics::SizeInt32,
    receiver: std::sync::mpsc::Receiver<()>,
    staging_texture: Option<ID3D11Texture2D>,
    last_frame: Option<image::RgbaImage>,
    last_health: Option<WindowHealth>,
  }

  // SAFETY: WinRT capture session and frame pool are thread-safe COM objects;
  // all mutations are guarded behind ACTIVE_SESSION Mutex.
  unsafe impl Send for CachedSession {}
  unsafe impl Sync for CachedSession {}

  impl Drop for CachedSession {
    fn drop(&mut self) {
      let _ = self.session.Close();
      let _ = self.frame_pool.Close();
    }
  }

  static ACTIVE_SESSION: Mutex<Option<CachedSession>> = Mutex::new(None);

  fn try_get_next_frame(
    frame_pool: &Direct3D11CaptureFramePool,
  ) -> DriverResult<Option<windows::Graphics::Capture::Direct3D11CaptureFrame>> {
    match frame_pool.TryGetNextFrame() {
      Ok(frame) => Ok(Some(frame)),
      Err(e) if e.code() == windows::core::HRESULT(0) => {
        // WinRT returns S_OK (0x0) with null interface pointer when pool is empty.
        // windows-rs converts this into Error with HRESULT(0).
        Ok(None)
      }
      Err(e) => {
        // Propagate real WinRT COM errors (device removed, closed, access denied, etc.)
        check_device_lost_error(&e);
        Err(backend(format!("Direct3D11CaptureFramePool::TryGetNextFrame failed: {e}")))
      }
    }
  }

  fn get_or_create_staging(d3d: &D3dContext, s: &mut CachedSession, desc: &D3D11_TEXTURE2D_DESC) -> DriverResult<ID3D11Texture2D> {
    if let Some(ref staging) = s.staging_texture {
      let mut staging_desc = D3D11_TEXTURE2D_DESC::default();
      unsafe { staging.GetDesc(&mut staging_desc) };
      if staging_desc.Width == desc.Width && staging_desc.Height == desc.Height && staging_desc.Format == desc.Format {
        return Ok(staging.clone());
      }
    }

    let staging_desc = D3D11_TEXTURE2D_DESC {
      Width: desc.Width,
      Height: desc.Height,
      MipLevels: 1,
      ArraySize: 1,
      Format: desc.Format,
      SampleDesc: DXGI_SAMPLE_DESC {
        Count: 1,
        Quality: 0,
      },
      Usage: D3D11_USAGE_STAGING,
      BindFlags: 0,
      CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
      MiscFlags: 0,
    };

    let mut staging_texture = None;
    unsafe {
      d3d.device.CreateTexture2D(&staging_desc, None, Some(&mut staging_texture)).map_err(|e| {
        check_device_lost_error(&e);
        backend(format!("failed to create D3D11 staging texture: {e}"))
      })?;
    }
    let staging = staging_texture.ok_or_else(|| backend("staging texture was None"))?;
    s.staging_texture = Some(staging.clone());
    Ok(staging)
  }

  /// Captures a single frame from a `GraphicsCaptureItem` and maps it to an RGBA image.
  ///
  /// Reuses active Direct3D11CaptureFramePool and GraphicsCaptureSession across consecutive
  /// calls on the same target, avoiding the ~70ms DWM session negotiation on every frame.
  pub fn capture_item_rgba(target_id: isize, item: &GraphicsCaptureItem, timeout: Duration) -> DriverResult<(image::RgbaImage, bool)> {
    let d3d = get_or_init_d3d_context()?;
    let res = capture_item_rgba_impl(&d3d, target_id, item, timeout);
    if res.is_err() {
      let reason = unsafe { d3d.device.GetDeviceRemovedReason() };
      if is_device_lost_reason(reason) {
        reset_d3d_context();
      }
    }
    res
  }

  fn capture_item_rgba_impl(
    d3d: &D3dContext,
    target_id: isize,
    item: &GraphicsCaptureItem,
    timeout: Duration,
  ) -> DriverResult<(image::RgbaImage, bool)> {
    let size = item.Size().map_err(|e| {
      check_device_lost_error(&e);
      backend(format!("failed to read GraphicsCaptureItem size: {e}"))
    })?;

    if size.Width <= 0 || size.Height <= 0 {
      return Err(crate::error::invalid_input(format!(
        "target has zero or invalid dimensions ({}x{}); target may be minimized",
        size.Width, size.Height
      )));
    }

    let mut session_guard = ACTIVE_SESSION.lock().map_err(|_| backend("active session mutex poisoned"))?;

    let is_match = match &*session_guard {
      Some(s) => s.target_id == target_id && s.size.Width == size.Width && s.size.Height == size.Height,
      None => false,
    };

    if !is_match {
      session_guard.take();

      let frame_pool =
        Direct3D11CaptureFramePool::CreateFreeThreaded(&d3d.winrt_device, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size).map_err(
          |e| {
            check_device_lost_error(&e);
            backend(format!("failed to create Direct3D11CaptureFramePool: {e}"))
          },
        )?;

      let (sender, receiver) = sync_channel::<()>(4);
      // NOTICE: Discarding the WinRT EventRegistrationToken does not unregister the handler;
      // the callback remains registered until frame_pool is closed via Close().
      let _token = frame_pool
        .FrameArrived(&TypedEventHandler::new(move |pool: &Option<Direct3D11CaptureFramePool>, _| {
          if pool.is_some() {
            let _ = sender.try_send(());
          }
          Ok(())
        }))
        .map_err(|e| {
          check_device_lost_error(&e);
          backend(format!("failed to register FrameArrived handler: {e}"))
        })?;

      let session = frame_pool.CreateCaptureSession(item).map_err(|e| {
        check_device_lost_error(&e);
        backend(format!("failed to create GraphicsCaptureSession: {e}"))
      })?;

      let _ = session.SetIsBorderRequired(false);
      let _ = session.SetIsCursorCaptureEnabled(false);

      session.StartCapture().map_err(|e| {
        check_device_lost_error(&e);
        backend(format!("failed to start GraphicsCaptureSession: {e}"))
      })?;

      *session_guard = Some(CachedSession {
        target_id,
        frame_pool,
        session,
        size,
        receiver,
        staging_texture: None,
        last_frame: None,
        last_health: None,
      });
    }

    let s = session_guard.as_mut().unwrap();

    // Try to get next frame. If none ready immediately, wait on receiver.
    let mut frame_opt = try_get_next_frame(&s.frame_pool)?;
    if frame_opt.is_none() {
      let wait_timeout = if s.last_frame.is_none() {
        timeout
      } else {
        Duration::from_millis(15)
      };
      let _ = s.receiver.recv_timeout(wait_timeout);
      frame_opt = try_get_next_frame(&s.frame_pool)?;
    }

    match frame_opt {
      Some(frame) => {
        let surface = frame.Surface().map_err(|e| {
          check_device_lost_error(&e);
          backend(format!("failed to obtain frame surface: {e}"))
        })?;

        let access: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(|e| {
          check_device_lost_error(&e);
          backend(format!("failed to cast surface to IDirect3DDxgiInterfaceAccess: {e}"))
        })?;

        let texture: ID3D11Texture2D = unsafe {
          access.GetInterface().map_err(|e| {
            check_device_lost_error(&e);
            backend(format!("failed to obtain ID3D11Texture2D from surface: {e}"))
          })?
        };

        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };

        // HDR tonemapping guard: fail explicitly on non-B8G8R8A8 formats
        if desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
          return Err(backend(format!(
            "unsupported pixel format {:?}: WGC v1 only supports B8G8R8A8_UNORM; HDR / 10-bit tonemapping is not implemented",
            desc.Format
          )));
        }

        let staging = get_or_create_staging(d3d, s, &desc)?;

        let ctx = d3d.context.lock().map_err(|_| backend("d3d context mutex poisoned"))?;
        unsafe {
          ctx.CopyResource(&staging, &texture);

          let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
          ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).map_err(|e| {
            check_device_lost_error(&e);
            backend(format!("failed to map staging texture: {e}"))
          })?;

          let width = desc.Width as usize;
          let height = desc.Height as usize;
          let row_pitch = mapped.RowPitch as usize;
          let mut rgba = vec![0u8; width * height * 4];

          let src_ptr = mapped.pData as *const u8;
          for y in 0..height {
            let src_row = std::slice::from_raw_parts(src_ptr.add(y * row_pitch), width * 4);
            let dst_row = &mut rgba[y * width * 4..(y + 1) * width * 4];
            for x in 0..width {
              dst_row[x * 4] = src_row[x * 4 + 2];
              dst_row[x * 4 + 1] = src_row[x * 4 + 1];
              dst_row[x * 4 + 2] = src_row[x * 4];
              dst_row[x * 4 + 3] = src_row[x * 4 + 3];
            }
          }

          ctx.Unmap(&staging, 0);

          let image = image::RgbaImage::from_raw(desc.Width, desc.Height, rgba)
            .ok_or_else(|| backend("failed to decode captured RGBA image buffer"))?;
          s.last_frame = Some(image.clone());
          Ok((image, true))
        }
      }
      None => {
        if let Some(ref img) = s.last_frame {
          Ok((img.clone(), false))
        } else {
          Err(backend(format!("WGC frame arrival timed out after {:?}", timeout)))
        }
      }
    }
  }

  /// Lightweight health check: checks window dimensions, pixel activity, and freshness
  /// without allocating and decoding full RGBA image.
  pub fn capture_item_health(target_id: isize, item: &GraphicsCaptureItem, timeout: Duration) -> DriverResult<WindowHealth> {
    let d3d = get_or_init_d3d_context()?;
    let res = capture_item_health_impl(&d3d, target_id, item, timeout);
    if res.is_err() {
      let reason = unsafe { d3d.device.GetDeviceRemovedReason() };
      if is_device_lost_reason(reason) {
        reset_d3d_context();
      }
    }
    res
  }

  fn capture_item_health_impl(
    d3d: &D3dContext,
    target_id: isize,
    item: &GraphicsCaptureItem,
    timeout: Duration,
  ) -> DriverResult<WindowHealth> {
    let size = item.Size().map_err(|e| {
      check_device_lost_error(&e);
      backend(format!("failed to read GraphicsCaptureItem size: {e}"))
    })?;

    if size.Width <= 0 || size.Height <= 0 {
      return Err(crate::error::invalid_input(format!(
        "target has zero or invalid dimensions ({}x{}); target may be minimized",
        size.Width, size.Height
      )));
    }

    let mut session_guard = ACTIVE_SESSION.lock().map_err(|_| backend("active session mutex poisoned"))?;

    let is_match = match &*session_guard {
      Some(s) => s.target_id == target_id && s.size.Width == size.Width && s.size.Height == size.Height,
      None => false,
    };

    if !is_match {
      session_guard.take();

      let frame_pool =
        Direct3D11CaptureFramePool::CreateFreeThreaded(&d3d.winrt_device, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size).map_err(
          |e| {
            check_device_lost_error(&e);
            backend(format!("failed to create Direct3D11CaptureFramePool: {e}"))
          },
        )?;

      let (sender, receiver) = sync_channel::<()>(4);
      let _token = frame_pool
        .FrameArrived(&TypedEventHandler::new(move |pool: &Option<Direct3D11CaptureFramePool>, _| {
          if pool.is_some() {
            let _ = sender.try_send(());
          }
          Ok(())
        }))
        .map_err(|e| {
          check_device_lost_error(&e);
          backend(format!("failed to register FrameArrived handler: {e}"))
        })?;

      let session = frame_pool.CreateCaptureSession(item).map_err(|e| {
        check_device_lost_error(&e);
        backend(format!("failed to create GraphicsCaptureSession: {e}"))
      })?;

      let _ = session.SetIsBorderRequired(false);
      let _ = session.SetIsCursorCaptureEnabled(false);

      session.StartCapture().map_err(|e| {
        check_device_lost_error(&e);
        backend(format!("failed to start GraphicsCaptureSession: {e}"))
      })?;

      *session_guard = Some(CachedSession {
        target_id,
        frame_pool,
        session,
        size,
        receiver,
        staging_texture: None,
        last_frame: None,
        last_health: None,
      });
    }

    let s = session_guard.as_mut().unwrap();

    let mut frame_opt = try_get_next_frame(&s.frame_pool)?;
    if frame_opt.is_none() {
      let wait_timeout = if s.last_health.is_none() && s.last_frame.is_none() {
        timeout
      } else {
        Duration::from_millis(15)
      };
      let _ = s.receiver.recv_timeout(wait_timeout);
      frame_opt = try_get_next_frame(&s.frame_pool)?;
    }

    match frame_opt {
      Some(frame) => {
        let surface = frame.Surface().map_err(|e| {
          check_device_lost_error(&e);
          backend(format!("failed to obtain frame surface: {e}"))
        })?;

        let access: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(|e| {
          check_device_lost_error(&e);
          backend(format!("failed to cast surface to IDirect3DDxgiInterfaceAccess: {e}"))
        })?;

        let texture: ID3D11Texture2D = unsafe {
          access.GetInterface().map_err(|e| {
            check_device_lost_error(&e);
            backend(format!("failed to obtain ID3D11Texture2D from surface: {e}"))
          })?
        };

        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };

        if desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
          return Err(backend(format!("unsupported pixel format {:?}: WGC v1 only supports B8G8R8A8_UNORM", desc.Format)));
        }

        let staging = get_or_create_staging(d3d, s, &desc)?;

        let ctx = d3d.context.lock().map_err(|_| backend("d3d context mutex poisoned"))?;
        unsafe {
          ctx.CopyResource(&staging, &texture);

          let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
          ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).map_err(|e| {
            check_device_lost_error(&e);
            backend(format!("failed to map staging texture: {e}"))
          })?;

          let width = desc.Width as usize;
          let height = desc.Height as usize;
          let row_pitch = mapped.RowPitch as usize;
          let src_ptr = mapped.pData as *const u8;

          // Subsample 1/16 pixels (every 4th row, every 4th pixel)
          let mut non_black_sampled = 0usize;
          let mut total_sampled = 0usize;
          let step_y = 4usize;
          let step_x = 4usize;

          let mut y = 0usize;
          while y < height {
            let row_ptr = src_ptr.add(y * row_pitch);
            let mut x = 0usize;
            while x < width {
              let px = row_ptr.add(x * 4);
              let b = *px;
              let g = *px.add(1);
              let r = *px.add(2);
              if r > 10 || g > 10 || b > 10 {
                non_black_sampled += 1;
              }
              total_sampled += 1;
              x += step_x;
            }
            y += step_y;
          }

          ctx.Unmap(&staging, 0);

          let ratio = if total_sampled > 0 {
            non_black_sampled as f64 / total_sampled as f64 * 100.0
          } else {
            0.0
          };

          let health = WindowHealth {
            width: desc.Width,
            height: desc.Height,
            non_black_ratio: ratio,
            is_fresh: true,
            alive: ratio >= 50.0,
          };
          s.last_health = Some(health.clone());
          Ok(health)
        }
      }
      None => {
        if let Some(ref h) = s.last_health {
          let mut stale = h.clone();
          stale.is_fresh = false;
          Ok(stale)
        } else if let Some(ref img) = s.last_frame {
          let total = (img.width() * img.height()) as usize;
          let raw = img.as_raw();
          let mut non_black = 0usize;
          for chunk in raw.chunks_exact(4) {
            if chunk[0] > 10 || chunk[1] > 10 || chunk[2] > 10 {
              non_black += 1;
            }
          }
          let ratio = if total > 0 {
            non_black as f64 / total as f64 * 100.0
          } else {
            0.0
          };
          Ok(WindowHealth {
            width: img.width(),
            height: img.height(),
            non_black_ratio: ratio,
            is_fresh: false,
            alive: ratio >= 50.0,
          })
        } else {
          Err(backend(format!("WGC frame arrival timed out after {:?}", timeout)))
        }
      }
    }
  }

  /// Creates a `GraphicsCaptureItem` for a native window handle.
  pub fn item_for_window(hwnd: HWND) -> DriverResult<GraphicsCaptureItem> {
    unsafe {
      let interop: IGraphicsCaptureItemInterop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
        .map_err(|e| backend(format!("failed to obtain IGraphicsCaptureItemInterop factory: {e}")))?;
      interop.CreateForWindow(hwnd).map_err(|e| backend(format!("IGraphicsCaptureItemInterop::CreateForWindow failed: {e}")))
    }
  }

  /// Creates a `GraphicsCaptureItem` for a native monitor handle.
  pub fn item_for_monitor(hmonitor: HMONITOR) -> DriverResult<GraphicsCaptureItem> {
    unsafe {
      let interop: IGraphicsCaptureItemInterop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
        .map_err(|e| backend(format!("failed to obtain IGraphicsCaptureItemInterop factory: {e}")))?;
      interop.CreateForMonitor(hmonitor).map_err(|e| backend(format!("IGraphicsCaptureItemInterop::CreateForMonitor failed: {e}")))
    }
  }

  /// Resolves the primary or target HMONITOR from screen coordinates.
  pub fn monitor_for_point(x: i32, y: i32) -> HMONITOR {
    unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) }
  }
}

/// Captures a target window using Windows.Graphics.Capture.
///
/// Produces a [`Capture`] with backend `"wgc.windows"`. Supports occluded windows
/// (where GDI/PrintWindow returns occluding content or black pixels).
#[cfg(target_os = "windows")]
pub fn capture_window_wgc(window: &Window) -> DriverResult<Capture> {
  let start_time = Instant::now();
  let hwnd = window_handle(window)?;
  let item = native::item_for_window(hwnd)?;
  let (image, is_fresh) = native::capture_item_rgba(hwnd.0 as isize, &item, Duration::from_millis(1000))?;
  let (width, height) = (image.width(), image.height());

  let scale_factor = if window.frame.size.width > 0.0 {
    f64::from(width) / window.frame.size.width
  } else {
    1.0
  };

  let elapsed_ms = start_time.elapsed().as_secs_f64() * 1000.0;
  let target_name = window.app_name.as_deref().or(window.title.as_deref());
  let details = match target_name {
    Some(name) => format!("fresh={is_fresh};target={name}"),
    None => format!("fresh={is_fresh}"),
  };
  crate::latency::record_latency_event("capture_window", elapsed_ms, Some((width, height)), Some(WGC_BACKEND), Some(&details));

  Ok(Capture {
    origin: Some(auv_driver_common::Position::in_window(&window.reference, auv_driver_common::WindowPoint::new(0.0, 0.0))),
    image,
    bounds: window.frame,
    scale_factor,
    backend: WGC_BACKEND.to_string(),
    fallback_reason: None,
  })
}

#[cfg(not(target_os = "windows"))]
pub fn capture_window_wgc(_window: &Window) -> DriverResult<Capture> {
  Err(auv_driver_common::error::DriverError::unsupported("window.capture_wgc"))
}

/// Lightweight health check for a window using Windows.Graphics.Capture.
/// Checks dimensions and non-black pixel ratio using mapped memory subsampling
/// without full RGBA image decoding and memory allocation.
#[cfg(target_os = "windows")]
pub fn capture_window_health(window: &Window) -> DriverResult<WindowHealth> {
  let start_time = Instant::now();
  let hwnd = window_handle(window)?;
  let item = native::item_for_window(hwnd)?;
  let health = native::capture_item_health(hwnd.0 as isize, &item, Duration::from_millis(1000))?;

  let elapsed_ms = start_time.elapsed().as_secs_f64() * 1000.0;
  let target_name = window.app_name.as_deref().or(window.title.as_deref());
  let details = match target_name {
    Some(name) => format!("fresh={};target={};non_black={:.1}%", health.is_fresh, name, health.non_black_ratio),
    None => format!("fresh={};non_black={:.1}%", health.is_fresh, health.non_black_ratio),
  };
  crate::latency::record_latency_event(
    "capture_window_health",
    elapsed_ms,
    Some((health.width, health.height)),
    Some(WGC_BACKEND),
    Some(&details),
  );

  Ok(health)
}

#[cfg(not(target_os = "windows"))]
pub fn capture_window_health(_window: &Window) -> DriverResult<WindowHealth> {
  Err(auv_driver_common::error::DriverError::unsupported("window.capture_window_health"))
}

/// Captures a target display using Windows.Graphics.Capture.
///
/// Produces a [`DisplayCapture`] with backend `"wgc.windows"`.
#[cfg(target_os = "windows")]
pub fn capture_display_wgc(selector: Option<&str>) -> DriverResult<DisplayCapture> {
  let start_time = Instant::now();
  let monitors = xcap::Monitor::all().map_err(|error| backend(format!("failed to enumerate displays: {error}")))?;
  let targets = crate::capture::display_targets_from_monitors(&monitors)?;
  let target = crate::capture::resolve_display_target(&targets, selector)?;

  let origin_x = target.display.frame.origin.x as i32;
  let origin_y = target.display.frame.origin.y as i32;
  let hmonitor = native::monitor_for_point(origin_x, origin_y);

  let item = native::item_for_monitor(hmonitor)?;
  let (image, is_fresh) = native::capture_item_rgba(hmonitor.0 as isize, &item, Duration::from_millis(1000))?;
  let (width, height) = (image.width(), image.height());

  let scale_factor = if target.display.frame.size.width > 0.0 {
    f64::from(width) / target.display.frame.size.width
  } else {
    1.0
  };

  let elapsed_ms = start_time.elapsed().as_secs_f64() * 1000.0;
  let details = match selector {
    Some(sel) => format!("fresh={is_fresh};selector={sel}"),
    None => format!("fresh={is_fresh}"),
  };
  crate::latency::record_latency_event("capture_display", elapsed_ms, Some((width, height)), Some(WGC_BACKEND), Some(&details));

  let capture = Capture {
    origin: Some(auv_driver_common::Position::in_screen(auv_driver_common::ScreenPoint::from(target.display.frame.origin))),
    image,
    bounds: target.display.frame,
    scale_factor,
    backend: WGC_BACKEND.to_string(),
    fallback_reason: None,
  };

  Ok(DisplayCapture {
    display: target.display,
    capture,
  })
}

#[cfg(not(target_os = "windows"))]
pub fn capture_display_wgc(_selector: Option<&str>) -> DriverResult<DisplayCapture> {
  Err(auv_driver_common::error::DriverError::unsupported("display.capture_wgc"))
}

/// Eagerly initializes the WGC Direct3D 11 device and context, returning the elapsed initialization duration.
///
/// Prewarming moves the ~30-50ms D3D11 device creation cost off the first capture's critical path.
#[cfg(target_os = "windows")]
pub fn prewarm_wgc() -> DriverResult<Duration> {
  let start = Instant::now();
  let d3d = native::get_or_init_d3d_context()?;
  let dummy_size = windows::Graphics::SizeInt32 {
    Width: 16,
    Height: 16,
  };
  if let Ok(pool) = windows::Graphics::Capture::Direct3D11CaptureFramePool::CreateFreeThreaded(
    &d3d.winrt_device,
    windows::Graphics::DirectX::DirectXPixelFormat::B8G8R8A8UIntNormalized,
    1,
    dummy_size,
  ) {
    let _ = pool.Close();
  }
  Ok(start.elapsed())
}

#[cfg(not(target_os = "windows"))]
pub fn prewarm_wgc() -> DriverResult<Duration> {
  Err(auv_driver_common::error::DriverError::unsupported("wgc.prewarm"))
}

/// Eagerly prewarms WGC for a specific window target, establishing the capture session
/// and caching the initial frame so subsequent health checks take ~3-5ms.
#[cfg(target_os = "windows")]
pub fn prewarm_wgc_window(window: &Window) -> DriverResult<Duration> {
  let start = Instant::now();
  let _ = capture_window_health(window)?;
  Ok(start.elapsed())
}

#[cfg(not(target_os = "windows"))]
pub fn prewarm_wgc_window(_window: &Window) -> DriverResult<Duration> {
  Err(auv_driver_common::error::DriverError::unsupported("wgc.prewarm_window"))
}

/// Resets the cached D3D11 context and active WGC session.
///
/// Used for device-lost recovery and fault-injection testing.
#[cfg(target_os = "windows")]
pub fn reset_d3d_context() {
  native::reset_d3d_context();
}

#[cfg(not(target_os = "windows"))]
pub fn reset_d3d_context() {}

#[cfg(test)]
mod tests {
  use super::*;
  use windows::Graphics::Capture::Direct3D11CaptureFramePool;
  use windows::Graphics::DirectX::DirectXPixelFormat;
  use windows::Graphics::SizeInt32;

  static TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

  #[test]
  fn test_try_get_next_frame_empty() {
    let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let d3d = native::get_or_init_d3d_context().unwrap();
    let size = SizeInt32 {
      Width: 100,
      Height: 100,
    };
    let frame_pool =
      Direct3D11CaptureFramePool::CreateFreeThreaded(&d3d.winrt_device, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size).unwrap();

    let res = match frame_pool.TryGetNextFrame() {
      Ok(f) => Ok(Some(f)),
      Err(e) if e.code() == windows::core::HRESULT(0) => Ok(None),
      Err(e) => Err(e),
    };
    assert!(res.is_ok(), "Expected Ok(None) for empty pool");
    assert!(res.unwrap().is_none(), "Expected None for empty pool");
  }

  #[test]
  fn test_prewarm_wgc() {
    let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let _dur = prewarm_wgc().expect("prewarm_wgc should succeed");
    assert!(native::is_d3d_context_initialized());
    // Subsequent prewarm is a fast cache hit
    let dur2 = prewarm_wgc().expect("second prewarm_wgc should succeed");
    assert!(dur2 < Duration::from_millis(50));
  }

  #[test]
  fn test_device_lost_recovery_simulation() {
    let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    // ROOT CAUSE:
    //
    // If GPU driver resets or device is lost (DXGI_ERROR_DEVICE_REMOVED 0x887A0005 or
    // DXGI_ERROR_DEVICE_RESET 0x887A0007), OnceLock<D3dContext> prevented recovery,
    // causing all subsequent WGC captures to permanently fail until process restart.
    //
    // Before the fix, D3D_CONTEXT was OnceLock and unrecoverable.
    // The fix keeps a recoverable RwLock store, resetting both context and active session
    // on device-lost so the next capture recreates the Direct3D device without panic.

    // 1. Context must be initialized
    let _ = prewarm_wgc().expect("prewarm should succeed");
    assert!(native::is_d3d_context_initialized());

    // 2. Fault injection: DXGI_ERROR_DEVICE_REMOVED (0x887A0005)
    let removed_err = windows::core::Error::from(windows::core::HRESULT(native::DXGI_ERROR_DEVICE_REMOVED_CODE));
    assert!(native::is_device_lost_hresult(removed_err.code()));
    native::check_device_lost_error(&removed_err);
    assert!(!native::is_d3d_context_initialized(), "context must be cleared after device-removed");

    // 3. Re-create succeeds on subsequent capture / prewarm
    let _ = prewarm_wgc().expect("re-creating context after device-removed must succeed");
    assert!(native::is_d3d_context_initialized(), "context must be re-initialized");

    // 4. Fault injection: DXGI_ERROR_DEVICE_RESET (0x887A0007)
    let reset_err = windows::core::Error::from(windows::core::HRESULT(native::DXGI_ERROR_DEVICE_RESET_CODE));
    assert!(native::is_device_lost_hresult(reset_err.code()));
    native::check_device_lost_error(&reset_err);
    assert!(!native::is_d3d_context_initialized(), "context must be cleared after device-reset");

    // 5. Re-create succeeds again
    let _ = prewarm_wgc().expect("re-creating context after device-reset must succeed");
    assert!(native::is_d3d_context_initialized(), "context must be re-initialized");

    // 6. Direct reset_d3d_context() wipe & recreation
    reset_d3d_context();
    assert!(!native::is_d3d_context_initialized());
    let _ = prewarm_wgc().expect("prewarm after direct reset must succeed");
    assert!(native::is_d3d_context_initialized());
  }
}
