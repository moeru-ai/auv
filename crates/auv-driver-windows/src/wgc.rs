//! Windows.Graphics.Capture (WGC) modern capture backend.
//!
//! Provides GPU-accelerated window and display capture using Direct3D 11
//! and `Windows.Graphics.Capture` WinRT APIs. Delivers CPU-mapped RGBA frames
//! with sub-10ms steady-state latency, coexisting with legacy GDI / PrintWindow
//! backends under the `"wgc.windows"` backend tag.

use std::sync::mpsc::sync_channel;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use auv_driver_common::capture::{Capture, DisplayCapture};
use auv_driver_common::error::DriverResult;
use auv_driver_common::window::Window;

use crate::error::{backend, invalid_input};
use crate::window::window_handle;

pub const WGC_BACKEND: &str = "wgc.windows";

#[cfg(target_os = "windows")]
mod native {
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
  use windows::Win32::System::StationsAndDesktops::{DESKTOP_ACCESS_FLAGS, DESKTOP_CONTROL_FLAGS, OpenInputDesktop, SetThreadDesktop};
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

  static D3D_CONTEXT: OnceLock<D3dContext> = OnceLock::new();

  fn get_or_init_d3d_context() -> DriverResult<&'static D3dContext> {
    if let Some(ctx) = D3D_CONTEXT.get() {
      return Ok(ctx);
    }

    ensure_input_desktop();

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

      let ctx = D3dContext {
        device: d3d_device,
        context: Mutex::new(d3d_context),
        winrt_device,
      };

      let _ = D3D_CONTEXT.set(ctx);
    }

    D3D_CONTEXT.get().ok_or_else(|| backend("failed to retrieve initialized D3D context"))
  }

  pub fn ensure_input_desktop() {
    unsafe {
      if let Ok(desktop) = OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_ACCESS_FLAGS(0x000F_01FF)) {
        let _ = SetThreadDesktop(desktop);
      }
    }
  }

  struct CachedSession {
    target_id: isize,
    _item: GraphicsCaptureItem,
    frame_pool: Direct3D11CaptureFramePool,
    session: windows::Graphics::Capture::GraphicsCaptureSession,
    size: windows::Graphics::SizeInt32,
    receiver: std::sync::mpsc::Receiver<()>,
    last_frame: Option<image::RgbaImage>,
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

  /// Captures a single frame from a `GraphicsCaptureItem` and maps it to an RGBA image.
  ///
  /// Reuses active Direct3D11CaptureFramePool and GraphicsCaptureSession across consecutive
  /// calls on the same target, avoiding the ~70ms DWM session negotiation on every frame.
  pub fn capture_item_rgba(target_id: isize, item: &GraphicsCaptureItem, timeout: Duration) -> DriverResult<image::RgbaImage> {
    let d3d = get_or_init_d3d_context()?;
    let size = item.Size().map_err(|e| backend(format!("failed to read GraphicsCaptureItem size: {e}")))?;

    if size.Width <= 0 || size.Height <= 0 {
      return Err(invalid_input(format!("target has zero or invalid dimensions ({}x{}); target may be minimized", size.Width, size.Height)));
    }

    let mut session_guard = ACTIVE_SESSION.lock().map_err(|_| backend("active session mutex poisoned"))?;

    let is_match = match &*session_guard {
      Some(s) => s.target_id == target_id && s.size.Width == size.Width && s.size.Height == size.Height,
      None => false,
    };

    if !is_match {
      if let Some(prev) = session_guard.take() {
        drop(prev);
      }

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
        _item: item.clone(),
        frame_pool,
        session,
        size,
        receiver,
        last_frame: None,
      });
    }

    let s = session_guard.as_mut().unwrap();

    // Try to get next frame. If none ready immediately, wait on receiver.
    let mut frame_opt = s.frame_pool.TryGetNextFrame().ok();
    if frame_opt.is_none() {
      let wait_timeout = if s.last_frame.is_none() {
        timeout
      } else {
        Duration::from_millis(15)
      };
      let _ = s.receiver.recv_timeout(wait_timeout);
      frame_opt = s.frame_pool.TryGetNextFrame().ok();
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
          Ok(image)
        }
      }
      None => {
        if let Some(ref img) = s.last_frame {
          Ok(img.clone())
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
  let image = native::capture_item_rgba(hwnd.0 as isize, &item, Duration::from_millis(1000))?;
  let (width, height) = (image.width(), image.height());

  let scale_factor = if window.frame.size.width > 0.0 {
    f64::from(width) / window.frame.size.width
  } else {
    1.0
  };

  let elapsed_ms = start_time.elapsed().as_secs_f64() * 1000.0;
  let target_name = window.app_name.as_deref().or(window.title.as_deref());
  crate::latency::record_latency_event("capture_window", elapsed_ms, Some((width, height)), Some(WGC_BACKEND), target_name);

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
  Err(DriverError::unsupported("window.capture_wgc"))
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
  let image = native::capture_item_rgba(hmonitor.0 as isize, &item, Duration::from_millis(1000))?;
  let (width, height) = (image.width(), image.height());

  let scale_factor = if target.display.frame.size.width > 0.0 {
    f64::from(width) / target.display.frame.size.width
  } else {
    1.0
  };

  let elapsed_ms = start_time.elapsed().as_secs_f64() * 1000.0;
  crate::latency::record_latency_event("capture_display", elapsed_ms, Some((width, height)), Some(WGC_BACKEND), selector);

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
  Err(DriverError::unsupported("display.capture_wgc"))
}
