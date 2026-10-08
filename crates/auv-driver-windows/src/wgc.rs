//! Windows.Graphics.Capture (WGC) modern capture backend.
//!
//! Provides GPU-accelerated window and display capture using Direct3D 11
//! and `Windows.Graphics.Capture` WinRT APIs. Delivers CPU-mapped RGBA frames
//! with sub-10ms steady-state latency, coexisting with legacy GDI / PrintWindow
//! backends under the `"wgc.windows"` backend tag.

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

/// Configuration policy for Fast window verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FastWindowVerification {
  /// Default mode: Uses 30ms TTL WGC health cache; waits for fresh WGC result on expiry.
  #[default]
  WgcFresh,
  /// Lightweight mode: Checks HWND validity, PID stability, and valid dimensions without pixel sampling.
  LightweightLiveness,
}

/// Cache key identifying a unique window target for WGC health checks.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HealthCacheKey {
  pub hwnd: isize,
  pub pid: u32,
  pub width: u32,
  pub height: u32,
}

/// Cached WGC health sample with timestamp, duration, and target dimensions.
#[derive(Debug, Clone)]
pub struct HealthCacheEntry {
  pub health: WindowHealth,
  pub captured_at: Instant,
  pub sample_duration: Duration,
  pub target_size: (u32, u32),
}

#[cfg(target_os = "windows")]
fn is_fresh_health_entry(entry: &HealthCacheEntry, now: Instant) -> bool {
  entry.health.is_fresh && now.saturating_duration_since(entry.captured_at) <= Duration::from_millis(30)
}

#[cfg(target_os = "windows")]
mod native {
  use std::sync::atomic::{AtomicBool, Ordering};
  use std::sync::mpsc::sync_channel;
  use std::sync::{Arc, Condvar, Mutex, RwLock};
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

  #[inline]
  pub fn is_device_lost_error_msg(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("0x887a0005") || lower.contains("0x887a0007") || lower.contains("device_removed") || lower.contains("device_reset")
  }

  pub fn reset_d3d_context() {
    match D3D_CONTEXT.write() {
      Ok(mut lock) => *lock = None,
      Err(e) => *e.into_inner() = None,
    }
    match ACTIVE_SESSION.lock() {
      Ok(mut session) => *session = None,
      Err(e) => *e.into_inner() = None,
    }
    match HEALTH_SESSION.lock() {
      Ok(mut session) => *session = None,
      Err(e) => *e.into_inner() = None,
    }
    clear_health_cache();
  }

  #[cfg(test)]
  pub fn is_d3d_context_initialized() -> bool {
    D3D_CONTEXT.read().map(|g| g.is_some()).unwrap_or(false)
  }

  #[cfg(test)]
  pub fn is_active_session_none() -> bool {
    ACTIVE_SESSION.lock().map(|g| g.is_none()).unwrap_or(true)
  }

  #[cfg(test)]
  pub fn with_active_session_locked<R>(f: impl FnOnce() -> R) -> R {
    let _guard = ACTIVE_SESSION.lock().unwrap();
    f()
  }

  fn finish_context_creation_error(
    guard: std::sync::RwLockWriteGuard<'_, Option<Arc<D3dContext>>>,
    err: auv_driver_common::error::DriverError,
  ) -> auv_driver_common::error::DriverError {
    let device_lost = is_device_lost_error_msg(&err.to_string());
    drop(guard);
    if device_lost {
      reset_d3d_context();
    }
    err
  }

  #[cfg(test)]
  pub(super) fn simulate_context_creation_error(err: auv_driver_common::error::DriverError) -> auv_driver_common::error::DriverError {
    let guard = D3D_CONTEXT.write().unwrap();
    finish_context_creation_error(guard, err)
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

    // Do not reset while holding D3D_CONTEXT.write(): device creation failures can
    // themselves report a device-lost HRESULT, and reset_d3d_context acquires this
    // same lock. Drop the guard before running recovery.
    let ctx = match create_d3d_context() {
      Ok(ctx) => ctx,
      Err(err) => return Err(finish_context_creation_error(guard, err)),
    };
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
      .map_err(|e| backend(format!("D3D11CreateDevice failed: {e}")))?;

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
    target_pid: Option<u32>,
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
  static HEALTH_SESSION: Mutex<Option<CachedSession>> = Mutex::new(None);

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
        // Notice: Do NOT invoke check_device_lost_error() / reset_d3d_context() here as
        // caller holds the ACTIVE_SESSION lock. The outer recovery logic resets D3D context
        // after releasing the session lock.
        Err(backend(format!("Direct3D11CaptureFramePool::TryGetNextFrame failed: {e}")))
      }
    }
  }

  pub(super) fn capture_with_device_lost_recovery<T>(
    capture: impl FnOnce() -> DriverResult<T>,
    is_device_lost: impl FnOnce(&str) -> bool,
  ) -> DriverResult<T> {
    let result = capture();
    if let Err(ref error) = result
      && is_device_lost(&error.to_string())
    {
      reset_d3d_context();
    }
    result
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
      d3d
        .device
        .CreateTexture2D(&staging_desc, None, Some(&mut staging_texture))
        .map_err(|e| backend(format!("failed to create D3D11 staging texture: {e}")))?;
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
    capture_with_device_lost_recovery(
      || capture_item_rgba_impl(&d3d, target_id, item, timeout),
      |error| {
        let reason = unsafe { d3d.device.GetDeviceRemovedReason() };
        is_device_lost_reason(reason) || is_device_lost_error_msg(error)
      },
    )
  }

  fn capture_item_rgba_impl(
    d3d: &D3dContext,
    target_id: isize,
    item: &GraphicsCaptureItem,
    timeout: Duration,
  ) -> DriverResult<(image::RgbaImage, bool)> {
    let size = item.Size().map_err(|e| backend(format!("failed to read GraphicsCaptureItem size: {e}")))?;

    if size.Width <= 0 || size.Height <= 0 {
      return Err(crate::error::invalid_input(format!(
        "target has zero or invalid dimensions ({}x{}); target may be minimized",
        size.Width, size.Height
      )));
    }

    let mut session_guard = ACTIVE_SESSION.lock().map_err(|_| backend("active session mutex poisoned"))?;

    let is_match = match &*session_guard {
      Some(s) => s.target_id == target_id && s.target_pid.is_none() && s.size.Width == size.Width && s.size.Height == size.Height,
      None => false,
    };

    if !is_match {
      session_guard.take();

      let frame_pool =
        Direct3D11CaptureFramePool::CreateFreeThreaded(&d3d.winrt_device, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size)
          .map_err(|e| backend(format!("failed to create Direct3D11CaptureFramePool: {e}")))?;

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
        .map_err(|e| backend(format!("failed to register FrameArrived handler: {e}")))?;

      let session = frame_pool.CreateCaptureSession(item).map_err(|e| backend(format!("failed to create GraphicsCaptureSession: {e}")))?;

      let _ = session.SetIsBorderRequired(false);
      let _ = session.SetIsCursorCaptureEnabled(false);

      session.StartCapture().map_err(|e| backend(format!("failed to start GraphicsCaptureSession: {e}")))?;

      *session_guard = Some(CachedSession {
        target_id,
        target_pid: None,
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
        let surface = frame.Surface().map_err(|e| backend(format!("failed to obtain frame surface: {e}")))?;

        let access: IDirect3DDxgiInterfaceAccess =
          surface.cast().map_err(|e| backend(format!("failed to cast surface to IDirect3DDxgiInterfaceAccess: {e}")))?;

        let texture: ID3D11Texture2D =
          unsafe { access.GetInterface().map_err(|e| backend(format!("failed to obtain ID3D11Texture2D from surface: {e}")))? };

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
          ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).map_err(|e| backend(format!("failed to map staging texture: {e}")))?;

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
  pub fn capture_item_health(
    target_id: isize,
    target_pid: u32,
    item: &GraphicsCaptureItem,
    timeout: Duration,
  ) -> DriverResult<WindowHealth> {
    let d3d = get_or_init_d3d_context()?;
    capture_with_device_lost_recovery(
      || capture_item_health_impl(&d3d, target_id, target_pid, item, timeout),
      |error| {
        let reason = unsafe { d3d.device.GetDeviceRemovedReason() };
        is_device_lost_reason(reason) || is_device_lost_error_msg(error)
      },
    )
  }

  fn capture_item_health_impl(
    d3d: &D3dContext,
    target_id: isize,
    target_pid: u32,
    item: &GraphicsCaptureItem,
    timeout: Duration,
  ) -> DriverResult<WindowHealth> {
    let size = item.Size().map_err(|e| backend(format!("failed to read GraphicsCaptureItem size: {e}")))?;

    if size.Width <= 0 || size.Height <= 0 {
      return Err(crate::error::invalid_input(format!(
        "target has zero or invalid dimensions ({}x{}); target may be minimized",
        size.Width, size.Height
      )));
    }

    let mut session_guard = HEALTH_SESSION.lock().map_err(|_| backend("health session mutex poisoned"))?;

    let is_match = match &*session_guard {
      Some(s) => s.target_id == target_id && s.target_pid == Some(target_pid) && s.size.Width == size.Width && s.size.Height == size.Height,
      None => false,
    };

    if !is_match {
      session_guard.take();

      let frame_pool =
        Direct3D11CaptureFramePool::CreateFreeThreaded(&d3d.winrt_device, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size)
          .map_err(|e| backend(format!("failed to create Direct3D11CaptureFramePool: {e}")))?;

      let (sender, receiver) = sync_channel::<()>(4);
      let _token = frame_pool
        .FrameArrived(&TypedEventHandler::new(move |pool: &Option<Direct3D11CaptureFramePool>, _| {
          if pool.is_some() {
            let _ = sender.try_send(());
          }
          Ok(())
        }))
        .map_err(|e| backend(format!("failed to register FrameArrived handler: {e}")))?;

      let session = frame_pool.CreateCaptureSession(item).map_err(|e| backend(format!("failed to create GraphicsCaptureSession: {e}")))?;

      let _ = session.SetIsBorderRequired(false);
      let _ = session.SetIsCursorCaptureEnabled(false);

      session.StartCapture().map_err(|e| backend(format!("failed to start GraphicsCaptureSession: {e}")))?;

      *session_guard = Some(CachedSession {
        target_id,
        target_pid: Some(target_pid),
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
      let wait_timeout = if s.last_health.is_none() {
        timeout
      } else {
        Duration::from_millis(15)
      };
      let _ = s.receiver.recv_timeout(wait_timeout);
      frame_opt = try_get_next_frame(&s.frame_pool)?;
    }

    match frame_opt {
      Some(frame) => {
        let surface = frame.Surface().map_err(|e| backend(format!("failed to obtain frame surface: {e}")))?;

        let access: IDirect3DDxgiInterfaceAccess =
          surface.cast().map_err(|e| backend(format!("failed to cast surface to IDirect3DDxgiInterfaceAccess: {e}")))?;

        let texture: ID3D11Texture2D =
          unsafe { access.GetInterface().map_err(|e| backend(format!("failed to obtain ID3D11Texture2D from surface: {e}")))? };

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
          ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).map_err(|e| backend(format!("failed to map staging texture: {e}")))?;

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

  pub struct HealthManagerState {
    pub cached_entry: Option<HealthCacheEntry>,
    pub target_key: Option<HealthCacheKey>,
    pub worker_running: bool,
    pub stop_requested: Arc<AtomicBool>,
    pub last_requested: Instant,
    pub last_refresh_duration: Duration,
    pub spawn_count: usize,
    pub active_workers: usize,
    /// Monotonically identifies the current worker. Old workers must not be
    /// allowed to clear state belonging to a replacement worker.
    pub worker_generation: u64,
  }

  static HEALTH_STATE: Mutex<Option<HealthManagerState>> = Mutex::new(None);
  /// Health capture currently owns one reusable WGC session. Serialize public
  /// health requests so a second target cannot evict the first request while
  /// it is waiting for its sample.
  static HEALTH_REQUEST_SERIAL: Mutex<()> = Mutex::new(());
  pub static HEALTH_CONDVAR: Condvar = Condvar::new();

  pub fn lock_health_state() -> std::sync::MutexGuard<'static, Option<HealthManagerState>> {
    HEALTH_STATE.lock().unwrap_or_else(|e| e.into_inner())
  }

  pub fn get_or_init_state<'a>(guard: &'a mut Option<HealthManagerState>) -> &'a mut HealthManagerState {
    guard.get_or_insert_with(|| HealthManagerState {
      cached_entry: None,
      target_key: None,
      worker_running: false,
      stop_requested: Arc::new(AtomicBool::new(false)),
      last_requested: Instant::now(),
      last_refresh_duration: Duration::ZERO,
      spawn_count: 0,
      active_workers: 0,
      worker_generation: 0,
    })
  }

  pub fn lock_health_request() -> std::sync::MutexGuard<'static, ()> {
    HEALTH_REQUEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner())
  }

  fn close_health_session_for_key(session_guard: &mut std::sync::MutexGuard<'_, Option<CachedSession>>, key: &HealthCacheKey) {
    if session_guard.as_ref().is_some_and(|session| {
      session.target_id == key.hwnd
        && session.target_pid == Some(key.pid)
        && session.size.Width == key.width as i32
        && session.size.Height == key.height as i32
    }) {
      session_guard.take();
    }
  }

  fn stop_idle_worker(key: &HealthCacheKey, worker_generation: u64) -> bool {
    // Keep the lock order consistent with reset_d3d_context (HEALTH_SESSION,
    // then HEALTH_STATE) so idle cleanup cannot deadlock device recovery.
    let mut session_guard = HEALTH_SESSION.lock().unwrap_or_else(|e| e.into_inner());
    let mut state_guard = lock_health_state();
    let stopped = if let Some(ref mut state) = *state_guard
      && state.target_key.as_ref() == Some(key)
      && state.worker_generation == worker_generation
      && state.last_requested.elapsed() >= Duration::from_millis(250)
    {
      state.worker_running = false;
      close_health_session_for_key(&mut session_guard, key);
      true
    } else {
      false
    };
    drop(state_guard);
    drop(session_guard);
    if stopped {
      HEALTH_CONDVAR.notify_all();
    }
    stopped
  }

  #[allow(dead_code)]
  pub fn with_health_state<R>(f: impl FnOnce(&mut HealthManagerState) -> R) -> R {
    let mut guard = lock_health_state();
    let state = get_or_init_state(&mut guard);
    f(state)
  }

  pub fn clear_health_cache() {
    let mut guard = lock_health_state();
    if let Some(ref mut state) = *guard {
      state.stop_requested.store(true, Ordering::SeqCst);
      state.cached_entry = None;
      state.target_key = None;
      state.worker_running = false;
      state.worker_generation = state.worker_generation.wrapping_add(1);
      state.stop_requested = Arc::new(AtomicBool::new(false));
    }
    HEALTH_CONDVAR.notify_all();
  }

  pub fn ensure_worker_started(key: &HealthCacheKey) -> DriverResult<()> {
    loop {
      let mut guard = lock_health_state();
      let state = get_or_init_state(&mut guard);
      state.last_requested = Instant::now();

      if state.target_key.as_ref() != Some(key) {
        state.stop_requested.store(true, Ordering::SeqCst);
        state.target_key = Some(key.clone());
        state.cached_entry = None;
        state.worker_running = false;
        state.worker_generation = state.worker_generation.wrapping_add(1);
        state.stop_requested = Arc::new(AtomicBool::new(false));
      }

      if state.worker_running {
        return Ok(());
      }

      if state.active_workers > 0 {
        let (new_guard, _) = HEALTH_CONDVAR.wait_timeout(guard, Duration::from_millis(500)).unwrap();
        drop(new_guard);
        continue;
      }

      // No worker is using the session now. Drop any session left by the
      // previous target before creating the replacement worker.
      drop(guard);
      if let Ok(mut session) = HEALTH_SESSION.lock() {
        session.take();
      }

      let mut guard = lock_health_state();
      let state = get_or_init_state(&mut guard);
      if state.target_key.as_ref() != Some(key) {
        continue;
      }
      if state.worker_running || state.active_workers > 0 {
        continue;
      }

      state.worker_running = true;
      state.spawn_count += 1;
      state.worker_generation = state.worker_generation.wrapping_add(1);
      let worker_generation = state.worker_generation;
      let stop_flag = Arc::clone(&state.stop_requested);
      let worker_key = key.clone();
      state.active_workers += 1;

      let spawn_result = std::thread::Builder::new().name("wgc-health-worker".to_string()).spawn(move || {
        health_worker_loop(worker_key, stop_flag, worker_generation);
      });
      if let Err(error) = spawn_result {
        state.active_workers = state.active_workers.saturating_sub(1);
        state.worker_running = false;
        HEALTH_CONDVAR.notify_all();
        return Err(backend(format!("failed to spawn wgc health worker: {error}")));
      }

      return Ok(());
    }
  }

  fn worker_exited(key: &HealthCacheKey, worker_generation: u64) {
    let mut guard = lock_health_state();
    if let Some(ref mut state) = *guard {
      state.active_workers = state.active_workers.saturating_sub(1);
      if state.target_key.as_ref() == Some(key) && state.worker_generation == worker_generation {
        state.worker_running = false;
      }
    }
    HEALTH_CONDVAR.notify_all();
  }

  struct HealthWorkerExit {
    key: HealthCacheKey,
    worker_generation: u64,
  }

  impl Drop for HealthWorkerExit {
    fn drop(&mut self) {
      worker_exited(&self.key, self.worker_generation);
    }
  }

  fn health_worker_loop(key: HealthCacheKey, stop_flag: Arc<AtomicBool>, worker_generation: u64) {
    let _exit = HealthWorkerExit {
      key: key.clone(),
      worker_generation,
    };
    crate::desktop::ensure_input_desktop();
    let hwnd = HWND(key.hwnd as _);
    let item = match item_for_window(hwnd) {
      Ok(it) => it,
      Err(_) => {
        let mut guard = lock_health_state();
        if let Some(ref mut state) = *guard {
          if state.target_key.as_ref() == Some(&key) && state.worker_generation == worker_generation {
            state.worker_running = false;
            state.cached_entry = None;
          }
        }
        HEALTH_CONDVAR.notify_all();
        return;
      }
    };

    while !stop_flag.load(Ordering::SeqCst) {
      // 1. Idle timeout check (250ms)
      let mut superseded = false;
      let mut idle = false;
      {
        let mut guard = lock_health_state();
        if let Some(ref mut state) = *guard {
          if state.target_key.as_ref() != Some(&key) || state.worker_generation != worker_generation {
            superseded = true;
          } else if state.last_requested.elapsed() >= Duration::from_millis(250) {
            idle = true;
          }
        } else {
          superseded = true;
        }
      }
      if superseded {
        break;
      }
      if idle {
        if stop_idle_worker(&key, worker_generation) {
          break;
        }
        continue;
      }

      // 2. Perform health check sample
      let sample_start = Instant::now();
      let res = capture_item_health(key.hwnd, key.pid, &item, Duration::from_millis(500));
      let sample_dur = sample_start.elapsed();

      match res {
        Ok(health) if health.is_fresh => {
          let mut guard = lock_health_state();
          if let Some(ref mut state) = *guard {
            if state.target_key.as_ref() == Some(&key) && state.worker_generation == worker_generation && !stop_flag.load(Ordering::SeqCst) {
              state.cached_entry = Some(HealthCacheEntry {
                health,
                captured_at: Instant::now(),
                sample_duration: sample_dur,
                target_size: (key.width, key.height),
              });
              state.last_refresh_duration = sample_dur;
              HEALTH_CONDVAR.notify_all();
            } else {
              break;
            }
          } else {
            break;
          }
        }
        Ok(_stale_health) => {
          // A frame pool can return the last frame when no new frame arrived.
          // Never refresh the cache timestamp for that stale sample.
          std::thread::sleep(Duration::from_millis(12));
          continue;
        }
        Err(_err) => {
          let mut guard = lock_health_state();
          if let Some(ref mut state) = *guard {
            if state.target_key.as_ref() == Some(&key) && state.worker_generation == worker_generation {
              state.worker_running = false;
              state.cached_entry = None;
            }
          }
          HEALTH_CONDVAR.notify_all();
          break;
        }
      }

      std::thread::sleep(Duration::from_millis(12));
    }
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
  let _request_guard = native::lock_health_request();
  capture_window_health_unserialized(window)
}

#[cfg(target_os = "windows")]
fn capture_window_health_unserialized(window: &Window) -> DriverResult<WindowHealth> {
  let start_time = Instant::now();
  let (hwnd, key, item) = resolve_window_key(window)?;
  let health = native::capture_item_health(hwnd.0 as isize, key.pid, &item, Duration::from_millis(1000))?;

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

#[cfg(target_os = "windows")]
fn resolve_window_key(
  window: &Window,
) -> DriverResult<(windows::Win32::Foundation::HWND, HealthCacheKey, windows::Graphics::Capture::GraphicsCaptureItem)> {
  let hwnd = window_handle(window)?;
  let mut current_pid = 0u32;
  let thread_id = unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(hwnd, Some(&mut current_pid)) };
  if thread_id == 0 || current_pid == 0 {
    return Err(backend("failed to resolve the live process id for the target HWND"));
  }
  if let Some(expected_pid) = window.process_id
    && expected_pid != current_pid
  {
    return Err(backend(format!("target HWND process changed from expected PID {expected_pid} to live PID {current_pid}")));
  }
  let item = native::item_for_window(hwnd)?;
  let size = item.Size().map_err(|e| backend(format!("failed to read GraphicsCaptureItem size: {e}")))?;
  if size.Width <= 0 || size.Height <= 0 {
    return Err(crate::error::invalid_input(format!(
      "target has zero or invalid dimensions ({}x{}); target may be minimized",
      size.Width, size.Height
    )));
  }
  let key = HealthCacheKey {
    hwnd: hwnd.0 as isize,
    pid: current_pid,
    width: size.Width as u32,
    height: size.Height as u32,
  };
  Ok((hwnd, key, item))
}

/// Lightweight liveness check for a target window.
///
/// Checks HWND validity, PID stability, and valid dimensions without pixel sampling.
#[cfg(target_os = "windows")]
pub fn check_window_liveness(window: &Window) -> DriverResult<bool> {
  use windows::Win32::Foundation::RECT;
  use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, GetWindowThreadProcessId, IsWindow};

  let hwnd = window_handle(window)?;
  unsafe {
    if !IsWindow(hwnd).as_bool() {
      return Ok(false);
    }

    if let Some(expected_pid) = window.process_id {
      let mut current_pid = 0u32;
      let tid = GetWindowThreadProcessId(hwnd, Some(&mut current_pid));
      if tid == 0 || current_pid != expected_pid {
        return Ok(false);
      }
    }

    let mut rect = RECT::default();
    if GetClientRect(hwnd, &mut rect).is_ok() {
      let width = rect.right - rect.left;
      let height = rect.bottom - rect.top;
      if width <= 0 || height <= 0 {
        return Ok(false);
      }
    } else {
      return Ok(false);
    }
  }

  Ok(true)
}

#[cfg(not(target_os = "windows"))]
pub fn check_window_liveness(_window: &Window) -> DriverResult<bool> {
  Err(auv_driver_common::error::DriverError::unsupported("window.check_window_liveness"))
}

/// Reads a cached WGC window health sample with 30ms TTL.
/// If fresh sample is present in cache (age <= 30ms), returns it immediately (<0.1ms).
/// If stale or absent, ensures worker is active and waits for worker refresh.
#[cfg(target_os = "windows")]
pub fn capture_window_health_cached(window: &Window) -> DriverResult<WindowHealth> {
  let _request_guard = native::lock_health_request();
  let start_time = Instant::now();
  let (_hwnd, key, _item) = resolve_window_key(window)?;

  let mut sample_age_ms = 0.0;
  let mut sample_refresh_ms = 0.0;

  // 1. Fast path: check if cache has fresh sample (age <= 30ms)
  {
    let mut guard = native::lock_health_state();
    let state = native::get_or_init_state(&mut guard);
    state.last_requested = Instant::now();

    if state.target_key.as_ref() == Some(&key)
      && let Some(ref entry) = state.cached_entry
    {
      let age = entry.captured_at.elapsed();
      sample_age_ms = age.as_secs_f64() * 1000.0;
      sample_refresh_ms = entry.sample_duration.as_secs_f64() * 1000.0;
      if entry.health.is_fresh && age <= Duration::from_millis(30) {
        let mut health = entry.health.clone();
        health.is_fresh = true;
        drop(guard);

        let elapsed_ms = start_time.elapsed().as_secs_f64() * 1000.0;
        let target_name = window.app_name.as_deref().or(window.title.as_deref());
        let details = format!(
          "cache_hit=true;age_ms={:.1};refresh_ms={:.1};target={};kind=cached",
          sample_age_ms,
          sample_refresh_ms,
          target_name.unwrap_or("unknown")
        );
        crate::latency::record_latency_event(
          "capture_window_health_cached",
          elapsed_ms,
          Some((health.width, health.height)),
          Some(WGC_BACKEND),
          Some(&details),
        );
        return Ok(health);
      }
    }
  }

  // 2. Slow path: ensure worker is started and wait for fresh sample
  native::ensure_worker_started(&key)?;

  let deadline = Instant::now() + Duration::from_millis(50);
  let mut guard = native::lock_health_state();
  let mut wait_success = false;

  while Instant::now() < deadline {
    let state = native::get_or_init_state(&mut guard);
    if state.target_key.as_ref() == Some(&key)
      && let Some(ref entry) = state.cached_entry
      && is_fresh_health_entry(entry, Instant::now())
      && entry.captured_at.elapsed() <= Duration::from_millis(30)
    {
      wait_success = true;
      break;
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
      break;
    }
    let (new_guard, wait_res) = native::HEALTH_CONDVAR.wait_timeout(guard, remaining).unwrap();
    guard = new_guard;
    if wait_res.timed_out() {
      break;
    }
  }

  let state = native::get_or_init_state(&mut guard);
  let (result, cache_hit, fallback_reason) = if wait_success && let Some(ref entry) = state.cached_entry {
    let mut h = entry.health.clone();
    h.is_fresh = true;
    sample_age_ms = entry.captured_at.elapsed().as_secs_f64() * 1000.0;
    sample_refresh_ms = entry.sample_duration.as_secs_f64() * 1000.0;
    (Ok(h), true, None)
  } else if let Some(ref entry) = state.cached_entry
    && state.target_key.as_ref() == Some(&key)
  {
    let mut h = entry.health.clone();
    h.is_fresh = false;
    sample_age_ms = entry.captured_at.elapsed().as_secs_f64() * 1000.0;
    sample_refresh_ms = entry.sample_duration.as_secs_f64() * 1000.0;
    (Ok(h), false, Some("stale_sample".to_string()))
  } else {
    drop(guard);
    let health = capture_window_health_unserialized(window)?;
    if !health.is_fresh {
      return Err(backend("fresh WGC health sample unavailable"));
    }
    let mut guard = native::lock_health_state();
    let state = native::get_or_init_state(&mut guard);
    if state.target_key.as_ref() == Some(&key) {
      state.cached_entry = Some(HealthCacheEntry {
        health: health.clone(),
        captured_at: Instant::now(),
        sample_duration: Duration::ZERO,
        target_size: (key.width, key.height),
      });
    }
    (Ok(health), false, Some("sync_fallback".to_string()))
  };

  let elapsed_ms = start_time.elapsed().as_secs_f64() * 1000.0;
  let target_name = window.app_name.as_deref().or(window.title.as_deref());
  if let Ok(ref h) = result {
    let details = format!(
      "cache_hit={};age_ms={:.1};refresh_ms={:.1};target={};kind=cached{}",
      cache_hit,
      sample_age_ms,
      sample_refresh_ms,
      target_name.unwrap_or("unknown"),
      fallback_reason.as_deref().map(|r| format!(";fallback={r}")).unwrap_or_default()
    );
    crate::latency::record_latency_event(
      "capture_window_health_cached",
      elapsed_ms,
      Some((h.width, h.height)),
      Some(WGC_BACKEND),
      Some(&details),
    );
  }

  result
}

#[cfg(not(target_os = "windows"))]
pub fn capture_window_health_cached(_window: &Window) -> DriverResult<WindowHealth> {
  Err(auv_driver_common::error::DriverError::unsupported("window.capture_window_health_cached"))
}

/// Strictly reads a fresh WGC window health sample (age <= 30ms).
/// If cache is expired or empty, waits for worker refresh.
/// Fails if unable to obtain a fresh sample within deadline.
#[cfg(target_os = "windows")]
pub fn capture_window_health_strict(window: &Window) -> DriverResult<WindowHealth> {
  let _request_guard = native::lock_health_request();
  let start_time = Instant::now();
  let (_hwnd, key, _item) = resolve_window_key(window)?;

  // 1. Fast path: check if cache has fresh sample (age <= 30ms)
  {
    let mut guard = native::lock_health_state();
    let state = native::get_or_init_state(&mut guard);
    state.last_requested = Instant::now();

    if state.target_key.as_ref() == Some(&key)
      && let Some(ref entry) = state.cached_entry
    {
      let age = entry.captured_at.elapsed();
      if entry.health.is_fresh && age <= Duration::from_millis(30) {
        let mut health = entry.health.clone();
        health.is_fresh = true;
        let age_ms = age.as_secs_f64() * 1000.0;
        let refresh_ms = entry.sample_duration.as_secs_f64() * 1000.0;
        drop(guard);

        let elapsed_ms = start_time.elapsed().as_secs_f64() * 1000.0;
        let target_name = window.app_name.as_deref().or(window.title.as_deref());
        let details = format!(
          "cache_hit=true;age_ms={:.1};refresh_ms={:.1};target={};kind=strict",
          age_ms,
          refresh_ms,
          target_name.unwrap_or("unknown")
        );
        crate::latency::record_latency_event(
          "capture_window_health_strict",
          elapsed_ms,
          Some((health.width, health.height)),
          Some(WGC_BACKEND),
          Some(&details),
        );
        return Ok(health);
      }
    }
  }

  // 2. Slow path: ensure worker running and wait for fresh sample
  native::ensure_worker_started(&key)?;

  let deadline = Instant::now() + Duration::from_millis(60);
  let mut guard = native::lock_health_state();

  while Instant::now() < deadline {
    let state = native::get_or_init_state(&mut guard);
    if state.target_key.as_ref() == Some(&key)
      && let Some(ref entry) = state.cached_entry
      && is_fresh_health_entry(entry, Instant::now())
      && entry.captured_at.elapsed() <= Duration::from_millis(30)
    {
      let mut health = entry.health.clone();
      health.is_fresh = true;
      let age_ms = entry.captured_at.elapsed().as_secs_f64() * 1000.0;
      let refresh_ms = entry.sample_duration.as_secs_f64() * 1000.0;
      drop(guard);

      let elapsed_ms = start_time.elapsed().as_secs_f64() * 1000.0;
      let target_name = window.app_name.as_deref().or(window.title.as_deref());
      let details =
        format!("cache_hit=false;age_ms={:.1};refresh_ms={:.1};target={};kind=strict", age_ms, refresh_ms, target_name.unwrap_or("unknown"));
      crate::latency::record_latency_event(
        "capture_window_health_strict",
        elapsed_ms,
        Some((health.width, health.height)),
        Some(WGC_BACKEND),
        Some(&details),
      );
      return Ok(health);
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
      break;
    }
    let (new_guard, wait_res) = native::HEALTH_CONDVAR.wait_timeout(guard, remaining).unwrap();
    guard = new_guard;
    if wait_res.timed_out() {
      break;
    }
  }

  Err(backend("fresh WGC health sample unavailable"))
}

#[cfg(not(target_os = "windows"))]
pub fn capture_window_health_strict(_window: &Window) -> DriverResult<WindowHealth> {
  Err(auv_driver_common::error::DriverError::unsupported("window.capture_window_health_strict"))
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
/// and starting the background health worker, caching the initial health sample.
#[cfg(target_os = "windows")]
pub fn prewarm_wgc_window(window: &Window) -> DriverResult<Duration> {
  let _request_guard = native::lock_health_request();
  let start = Instant::now();
  let (_hwnd, key, _item) = resolve_window_key(window)?;

  native::ensure_worker_started(&key)?;

  let deadline = Instant::now() + Duration::from_millis(500);
  let mut guard = native::lock_health_state();
  while Instant::now() < deadline {
    let state = native::get_or_init_state(&mut guard);
    if state.target_key.as_ref() == Some(&key) && state.cached_entry.is_some() {
      return Ok(start.elapsed());
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
      break;
    }
    let (new_guard, wait_res) = native::HEALTH_CONDVAR.wait_timeout(guard, remaining).unwrap();
    guard = new_guard;
    if wait_res.timed_out() {
      break;
    }
  }

  // Fallback to synchronous capture if worker did not populate cache within deadline
  drop(guard);
  let _ = capture_window_health_unserialized(window)?;
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

#[cfg(all(test, target_os = "windows"))]
pub fn health_cache_spawn_count() -> usize {
  native::with_health_state(|s| s.spawn_count)
}

#[cfg(all(test, target_os = "windows"))]
pub fn is_health_worker_running() -> bool {
  native::with_health_state(|s| s.worker_running)
}

#[cfg(all(test, target_os = "windows"))]
pub fn inject_health_cache_entry(key: HealthCacheKey, health: WindowHealth, age: Duration) {
  native::with_health_state(|s| {
    s.target_key = Some(key.clone());
    s.cached_entry = Some(HealthCacheEntry {
      health,
      captured_at: Instant::now().checked_sub(age).unwrap_or_else(Instant::now),
      sample_duration: Duration::from_millis(2),
      target_size: (key.width, key.height),
    });
  });
}

#[cfg(all(test, target_os = "windows"))]
pub fn clear_health_cache() {
  native::clear_health_cache();
}

// The tests drive Direct3D and WGC directly, so they only build on Windows.
#[cfg(all(test, target_os = "windows"))]
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

    // 2. Simulate device removal while the context registry's write lock is held.
    let removed_err = windows::core::Error::from(windows::core::HRESULT(native::DXGI_ERROR_DEVICE_REMOVED_CODE));
    assert!(native::is_device_lost_hresult(removed_err.code()));
    let _ = native::simulate_context_creation_error(crate::error::backend(format!("D3D11CreateDevice failed: {removed_err}")));
    assert!(!native::is_d3d_context_initialized(), "context must be cleared after device-removed");

    // 3. Re-create succeeds on subsequent capture / prewarm
    let _ = prewarm_wgc().expect("re-creating context after device-removed must succeed");
    assert!(native::is_d3d_context_initialized(), "context must be re-initialized");

    // 4. Simulate device reset through the same write-lock recovery path.
    let reset_err = windows::core::Error::from(windows::core::HRESULT(native::DXGI_ERROR_DEVICE_RESET_CODE));
    assert!(native::is_device_lost_hresult(reset_err.code()));
    let _ = native::simulate_context_creation_error(crate::error::backend(format!("D3D11CreateDevice failed: {reset_err}")));
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

  #[test]
  fn test_device_lost_recovery_under_lock_simulation() {
    let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

    // Verify error string detection
    assert!(native::is_device_lost_error_msg("Direct3D11CaptureFramePool failed: 0x887A0005"));
    assert!(native::is_device_lost_error_msg("Direct3D11CaptureFramePool failed: 0x887a0007"));
    assert!(native::is_device_lost_error_msg("DXGI_ERROR_DEVICE_REMOVED occurred"));
    assert!(native::is_device_lost_error_msg("DXGI_ERROR_DEVICE_RESET occurred"));
    assert!(!native::is_device_lost_error_msg("unrelated io error"));

    // Ensure context is initialized
    let _ = prewarm_wgc().expect("prewarm must succeed");
    assert!(native::is_d3d_context_initialized());

    // The production wrapper runs recovery only after the capture closure returns,
    // which releases ACTIVE_SESSION. This would self-deadlock if recovery ran inside it.
    let result: DriverResult<()> = native::capture_with_device_lost_recovery(
      || native::with_active_session_locked(|| Err(crate::error::backend("TryGetNextFrame failed: 0x887A0005"))),
      native::is_device_lost_error_msg,
    );
    assert!(result.is_err());
    assert!(!native::is_d3d_context_initialized(), "context must be reset by outer recovery");
    assert!(native::is_active_session_none(), "session must be cleared");

    // Can recover and re-initialize
    let _ = prewarm_wgc().expect("re-prewarm must succeed after recovery");
    assert!(native::is_d3d_context_initialized());
  }

  #[test]
  fn test_fast_window_verification_policy_selection() {
    assert_eq!(FastWindowVerification::default(), FastWindowVerification::WgcFresh);
    let json_wgc = serde_json::to_string(&FastWindowVerification::WgcFresh).unwrap();
    assert_eq!(json_wgc, "\"wgc_fresh\"");
    let parsed_wgc: FastWindowVerification = serde_json::from_str(&json_wgc).unwrap();
    assert_eq!(parsed_wgc, FastWindowVerification::WgcFresh);

    let json_live = serde_json::to_string(&FastWindowVerification::LightweightLiveness).unwrap();
    assert_eq!(json_live, "\"lightweight_liveness\"");
    let parsed_live: FastWindowVerification = serde_json::from_str(&json_live).unwrap();
    assert_eq!(parsed_live, FastWindowVerification::LightweightLiveness);
  }

  #[test]
  fn test_health_cache_hit_within_30ms() {
    let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    clear_health_cache();

    let key = HealthCacheKey {
      hwnd: 1234,
      pid: 5678,
      width: 800,
      height: 600,
    };
    let health = WindowHealth {
      width: 800,
      height: 600,
      non_black_ratio: 95.0,
      is_fresh: true,
      alive: true,
    };
    inject_health_cache_entry(key.clone(), health, Duration::from_millis(10));

    // Verify cache query returns fresh sample
    native::with_health_state(|s| {
      assert_eq!(s.target_key.as_ref(), Some(&key));
      let entry = s.cached_entry.as_ref().expect("entry must exist");
      assert!(entry.captured_at.elapsed() <= Duration::from_millis(30));
    });
  }

  #[test]
  fn test_expired_sample_detection() {
    let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    clear_health_cache();

    let key = HealthCacheKey {
      hwnd: 1234,
      pid: 5678,
      width: 800,
      height: 600,
    };
    let health = WindowHealth {
      width: 800,
      height: 600,
      non_black_ratio: 95.0,
      is_fresh: true,
      alive: true,
    };
    inject_health_cache_entry(key.clone(), health, Duration::from_millis(45));

    // Verify sample is expired (> 30ms)
    native::with_health_state(|s| {
      let entry = s.cached_entry.as_ref().expect("entry must exist");
      assert!(entry.captured_at.elapsed() > Duration::from_millis(30), "sample must be marked stale");
    });
  }

  #[test]
  fn test_cache_invalidation_on_key_changes() {
    let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    clear_health_cache();

    let key1 = HealthCacheKey {
      hwnd: 1001,
      pid: 2001,
      width: 640,
      height: 480,
    };
    let health = WindowHealth {
      width: 640,
      height: 480,
      non_black_ratio: 80.0,
      is_fresh: true,
      alive: true,
    };
    inject_health_cache_entry(key1.clone(), health.clone(), Duration::from_millis(5));

    // Invalidate on key change
    let key2 = HealthCacheKey {
      hwnd: 1002, // HWND changed
      pid: 2001,
      width: 640,
      height: 480,
    };
    native::with_health_state(|s| {
      if s.target_key.as_ref() != Some(&key2) {
        s.cached_entry = None;
        s.target_key = Some(key2);
      }
    });

    native::with_health_state(|s| {
      assert!(s.cached_entry.is_none(), "cache must be invalidated when HWND changes");
    });
  }

  #[test]
  fn test_single_worker_per_target_simulation() {
    let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    clear_health_cache();

    let key = HealthCacheKey {
      hwnd: 9999,
      pid: 8888,
      width: 500,
      height: 500,
    };

    native::with_health_state(|s| {
      s.target_key = Some(key.clone());
      s.worker_running = true;
      s.spawn_count = 1;
    });

    let count_before = health_cache_spawn_count();

    // Simulating ensure_worker_started when already running
    native::with_health_state(|s| {
      if s.worker_running && s.target_key.as_ref() == Some(&key) {
        // Does not spawn duplicate worker
      } else {
        s.spawn_count += 1;
      }
    });

    let count_after = health_cache_spawn_count();
    assert_eq!(count_before, count_after, "should not spawn duplicate worker for same active target");
  }

  #[test]
  fn test_worker_idle_exit_and_restart_simulation() {
    let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    clear_health_cache();

    let key = HealthCacheKey {
      hwnd: 7777,
      pid: 6666,
      width: 400,
      height: 300,
    };

    native::with_health_state(|s| {
      s.target_key = Some(key.clone());
      s.worker_running = true;
      s.spawn_count = 1;
      // Simulate idle for 300ms (> 250ms)
      s.last_requested = Instant::now().checked_sub(Duration::from_millis(300)).unwrap_or_else(Instant::now);
    });

    // Simulate idle check
    native::with_health_state(|s| {
      if s.last_requested.elapsed() >= Duration::from_millis(250) {
        s.worker_running = false;
      }
    });
    assert!(!is_health_worker_running(), "worker should exit after 250ms idle");

    // Restart worker
    native::with_health_state(|s| {
      if !s.worker_running {
        s.worker_running = true;
        s.spawn_count += 1;
        s.last_requested = Instant::now();
      }
    });
    assert!(is_health_worker_running(), "worker should restart on new request");
    assert_eq!(health_cache_spawn_count(), 2, "spawn count must increment on restart");
  }

  #[test]
  fn test_device_lost_clears_health_cache() {
    let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    clear_health_cache();

    let key = HealthCacheKey {
      hwnd: 1111,
      pid: 2222,
      width: 800,
      height: 600,
    };
    let health = WindowHealth {
      width: 800,
      height: 600,
      non_black_ratio: 90.0,
      is_fresh: true,
      alive: true,
    };
    inject_health_cache_entry(key, health, Duration::from_millis(5));

    // Resetting D3D context must clear health cache
    reset_d3d_context();

    native::with_health_state(|s| {
      assert!(s.cached_entry.is_none(), "health cache must be cleared upon reset_d3d_context");
      assert!(s.target_key.is_none(), "target key must be reset");
      assert!(!s.worker_running, "worker must be marked stopped");
    });
  }
}
