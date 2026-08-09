//! Native Win32 layered-window overlay renderer.
//!
//! Every [`present`] call redraws all requested layers into one ARGB32
//! bitmap sized to the virtual screen and blits it onto a single topmost,
//! click-through, alpha-blended window via `UpdateLayeredWindow`. Layers are
//! one-shot visual evidence (see `Overlay::with_layer`'s deferral note in
//! `auv-driver-overlay-common`), so there is no incremental per-layer update
//! path to maintain; each call fully replaces the previous frame.

#[cfg(target_os = "windows")]
pub(crate) use native::{hide_all, present};

#[cfg(not(target_os = "windows"))]
pub(crate) fn present(_layers: &[auv_driver_overlay_common::Layer]) -> crate::AuvResult<()> {
  Err("windows overlay native window is unsupported on this target".to_string())
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn hide_all() -> crate::AuvResult<()> {
  Err("windows overlay native window is unsupported on this target".to_string())
}

/// BGRA pixel value used to seed the offscreen canvas before drawing. Any
/// pixel that still matches this exact color after all layers are drawn is
/// treated as transparent; every other pixel becomes fully opaque.
///
/// NOTICE: raw GDI drawing ignores the destination alpha channel, so this
/// sentinel-key technique is what turns solid-color GDI output into a
/// layered, alpha-blended window without a GDI+/Direct2D dependency. The
/// tradeoff is no antialiasing and a theoretical (practically negligible)
/// collision if a drawn pixel exactly matches the sentinel value.
const SENTINEL_BGRA: [u8; 4] = [3, 2, 1, 0];

/// Converts a sentinel-keyed BGRA buffer in place into a premultiplied-alpha
/// buffer suitable for `UpdateLayeredWindow`'s `ULW_ALPHA` mode: sentinel
/// pixels become fully transparent black, all other pixels become fully
/// opaque (premultiplication is a no-op at alpha 255).
fn key_sentinel_to_alpha(pixels: &mut [u8]) {
  for pixel in pixels.chunks_exact_mut(4) {
    if pixel == SENTINEL_BGRA {
      pixel.copy_from_slice(&[0, 0, 0, 0]);
    } else {
      pixel[3] = 255;
    }
  }
}

#[cfg(target_os = "windows")]
mod native {
  use std::sync::{Mutex, OnceLock};

  use auv_driver_common::geometry::Point;
  use auv_driver_overlay_common::Layer;
  use auv_driver_overlay_common::layers::{BuiltInCursor, Cursor, CursorImage, Outline, Status};
  use auv_driver_overlay_common::style::{Color, Insets};
  use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
  use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC, CreateDIBSection, CreatePen,
    CreateSolidBrush, DIB_RGB_COLORS, DT_CALCRECT, DT_CENTER, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, Ellipse,
    GetStockObject, HBITMAP, HDC, HGDIOBJ, NULL_BRUSH, PS_SOLID, RoundRect, SYSTEM_FONT, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
  };
  use windows::Win32::System::LibraryLoader::GetModuleHandleW;
  use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, GetSystemMetrics, RegisterClassExW, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_HIDE, SW_SHOWNOACTIVATE, ShowWindow, ULW_ALPHA, UpdateLayeredWindow, WNDCLASSEXW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
  };

  use super::{SENTINEL_BGRA, key_sentinel_to_alpha};
  use crate::AuvResult;

  const WINDOW_CLASS_NAME: &str = "AuvOverlayWindowWindows";

  static WINDOW: OnceLock<Mutex<Option<isize>>> = OnceLock::new();

  fn window_slot() -> &'static Mutex<Option<isize>> {
    WINDOW.get_or_init(|| Mutex::new(None))
  }

  fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
  }

  extern "system" fn window_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
  }

  fn virtual_screen_rect() -> RECT {
    unsafe {
      let left = GetSystemMetrics(SM_XVIRTUALSCREEN);
      let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
      RECT {
        left,
        top,
        right: left + GetSystemMetrics(SM_CXVIRTUALSCREEN),
        bottom: top + GetSystemMetrics(SM_CYVIRTUALSCREEN),
      }
    }
  }

  fn ensure_window() -> AuvResult<HWND> {
    let mut slot = window_slot().lock().map_err(|_| "overlay window state lock poisoned".to_string())?;
    if let Some(raw) = *slot {
      return Ok(HWND(raw as *mut _));
    }

    let class_name = wide_null(WINDOW_CLASS_NAME);
    let instance = unsafe { GetModuleHandleW(None) }.map_err(|error| format!("failed to resolve module handle: {error}"))?;
    let instance = windows::Win32::Foundation::HINSTANCE::from(instance);

    let class = WNDCLASSEXW {
      cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
      style: CS_HREDRAW | CS_VREDRAW,
      lpfnWndProc: Some(window_proc),
      hInstance: instance,
      lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
      ..Default::default()
    };
    // A prior `present()` call in this process may have already registered
    // the class; RegisterClassExW failing with ERROR_CLASS_ALREADY_EXISTS is
    // expected in that case and is not fatal.
    unsafe {
      let _ = RegisterClassExW(&class);
    }

    let rect = virtual_screen_rect();
    let title = wide_null("AUV Overlay");
    let hwnd = unsafe {
      CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        windows::core::PCWSTR(class_name.as_ptr()),
        windows::core::PCWSTR(title.as_ptr()),
        WS_POPUP,
        rect.left,
        rect.top,
        rect.right - rect.left,
        rect.bottom - rect.top,
        None,
        None,
        instance,
        None,
      )
    }
    .map_err(|error| format!("failed to create overlay window: {error}"))?;

    *slot = Some(hwnd.0 as isize);
    Ok(hwnd)
  }

  fn colorref(color: Color) -> COLORREF {
    let r = (color.red.clamp(0.0, 1.0) * 255.0).round() as u32;
    let g = (color.green.clamp(0.0, 1.0) * 255.0).round() as u32;
    let b = (color.blue.clamp(0.0, 1.0) * 255.0).round() as u32;
    COLORREF(r | (g << 8) | (b << 16))
  }

  /// Offscreen ARGB32 canvas backing one `present()` frame. Drawing uses
  /// plain GDI primitives against solid colors; [`Canvas::finish`] then
  /// alpha-keys the buffer (see [`key_sentinel_to_alpha`]) before blitting it
  /// onto the layered window.
  struct Canvas {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut u8,
    width: i32,
    height: i32,
    origin: POINT,
  }

  impl Canvas {
    fn new(rect: RECT) -> AuvResult<Self> {
      let width = rect.right - rect.left;
      let height = rect.bottom - rect.top;
      let dc = unsafe { CreateCompatibleDC(None) };
      if dc.is_invalid() {
        return Err("failed to create overlay memory device context".to_string());
      }

      let mut bmi = BITMAPINFO::default();
      bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
      bmi.bmiHeader.biWidth = width;
      bmi.bmiHeader.biHeight = -height;
      bmi.bmiHeader.biPlanes = 1;
      bmi.bmiHeader.biBitCount = 32;
      bmi.bmiHeader.biCompression = 0;

      let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
      let bitmap = unsafe { CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) }
        .map_err(|error| format!("failed to create overlay bitmap: {error}"))?;
      if bits.is_null() {
        unsafe {
          let _ = DeleteDC(dc);
        }
        return Err("overlay bitmap allocation returned a null pixel buffer".to_string());
      }

      let previous = unsafe { SelectObject(dc, bitmap) };
      unsafe { SetBkMode(dc, TRANSPARENT) };

      let pixel_count = (width as usize) * (height as usize);
      let pixels = unsafe { std::slice::from_raw_parts_mut(bits.cast::<u8>(), pixel_count * 4) };
      for pixel in pixels.chunks_exact_mut(4) {
        pixel.copy_from_slice(&SENTINEL_BGRA);
      }

      Ok(Self {
        dc,
        bitmap,
        previous,
        bits: bits.cast::<u8>(),
        width,
        height,
        origin: POINT {
          x: rect.left,
          y: rect.top,
        },
      })
    }

    fn to_local(&self, point: Point) -> POINT {
      POINT {
        x: (point.x - f64::from(self.origin.x)).round() as i32,
        y: (point.y - f64::from(self.origin.y)).round() as i32,
      }
    }

    fn fill_circle(&self, center: POINT, radius: i32, color: COLORREF) {
      let radius = radius.max(1);
      let brush = unsafe { CreateSolidBrush(color) };
      let pen = unsafe { CreatePen(PS_SOLID, 1, color) };
      let previous_brush = unsafe { SelectObject(self.dc, brush) };
      let previous_pen = unsafe { SelectObject(self.dc, pen) };
      unsafe {
        let _ = Ellipse(self.dc, center.x - radius, center.y - radius, center.x + radius, center.y + radius);
        SelectObject(self.dc, previous_brush);
        SelectObject(self.dc, previous_pen);
        let _ = DeleteObject(brush);
        let _ = DeleteObject(pen);
      }
    }

    fn stroke_circle(&self, center: POINT, radius: i32, color: COLORREF, width: i32) {
      let radius = radius.max(1);
      let pen = unsafe { CreatePen(PS_SOLID, width.max(1), color) };
      let previous_pen = unsafe { SelectObject(self.dc, pen) };
      let null_brush = unsafe { GetStockObject(NULL_BRUSH) };
      let previous_brush = unsafe { SelectObject(self.dc, null_brush) };
      unsafe {
        let _ = Ellipse(self.dc, center.x - radius, center.y - radius, center.x + radius, center.y + radius);
        SelectObject(self.dc, previous_pen);
        SelectObject(self.dc, previous_brush);
        let _ = DeleteObject(pen);
      }
    }

    fn stroke_rounded_rect(&self, rect: RECT, color: COLORREF, width: i32, corner_radius: i32) {
      let pen = unsafe { CreatePen(PS_SOLID, width.max(1), color) };
      let previous_pen = unsafe { SelectObject(self.dc, pen) };
      let null_brush = unsafe { GetStockObject(NULL_BRUSH) };
      let previous_brush = unsafe { SelectObject(self.dc, null_brush) };
      unsafe {
        let _ = RoundRect(self.dc, rect.left, rect.top, rect.right, rect.bottom, corner_radius.max(0) * 2, corner_radius.max(0) * 2);
        SelectObject(self.dc, previous_pen);
        SelectObject(self.dc, previous_brush);
        let _ = DeleteObject(pen);
      }
    }

    fn fill_rounded_rect(&self, rect: RECT, color: COLORREF, corner_radius: i32) {
      let brush = unsafe { CreateSolidBrush(color) };
      let pen = unsafe { CreatePen(PS_SOLID, 1, color) };
      let previous_brush = unsafe { SelectObject(self.dc, brush) };
      let previous_pen = unsafe { SelectObject(self.dc, pen) };
      unsafe {
        let _ = RoundRect(self.dc, rect.left, rect.top, rect.right, rect.bottom, corner_radius.max(0) * 2, corner_radius.max(0) * 2);
        SelectObject(self.dc, previous_brush);
        SelectObject(self.dc, previous_pen);
        let _ = DeleteObject(brush);
        let _ = DeleteObject(pen);
      }
    }

    /// Draws a solid pill behind `text`, anchored with its vertical center at
    /// `anchor` and growing to the right, sized from measured text extents
    /// plus `padding`.
    fn draw_label_pill(&self, anchor: POINT, text: &str, foreground: COLORREF, background: COLORREF, padding: Insets, corner_radius: f64) {
      let wide: Vec<u16> = text.encode_utf16().collect();
      let font = unsafe { GetStockObject(SYSTEM_FONT) };
      let previous_font = unsafe { SelectObject(self.dc, font) };

      let mut measured = RECT::default();
      unsafe {
        DrawTextW(self.dc, &mut wide.clone(), &mut measured, DT_CALCRECT | DT_SINGLELINE);
      }

      let pill_width = (measured.right - measured.left) + (padding.left + padding.right).round() as i32;
      let pill_height = (measured.bottom - measured.top) + (padding.top + padding.bottom).round() as i32;
      let pill = RECT {
        left: anchor.x,
        top: anchor.y - pill_height / 2,
        right: anchor.x + pill_width,
        bottom: anchor.y - pill_height / 2 + pill_height,
      };

      self.fill_rounded_rect(pill, background, corner_radius.round() as i32);

      unsafe {
        SetTextColor(self.dc, foreground);
        let mut text_rect = pill;
        let _ = DrawTextW(self.dc, &mut wide.clone(), &mut text_rect, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        SelectObject(self.dc, previous_font);
      }
    }

    fn finish(&self, hwnd: HWND) -> AuvResult<()> {
      let pixel_count = (self.width as usize) * (self.height as usize);
      let pixels = unsafe { std::slice::from_raw_parts_mut(self.bits, pixel_count * 4) };
      key_sentinel_to_alpha(pixels);

      let size = SIZE {
        cx: self.width,
        cy: self.height,
      };
      let src_point = POINT { x: 0, y: 0 };
      let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
      };

      unsafe {
        UpdateLayeredWindow(hwnd, None, Some(&self.origin), Some(&size), self.dc, Some(&src_point), COLORREF(0), Some(&blend), ULW_ALPHA)
      }
      .map_err(|error| format!("failed to update overlay layered window: {error}"))?;

      unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
      }
      Ok(())
    }
  }

  impl Drop for Canvas {
    fn drop(&mut self) {
      unsafe {
        SelectObject(self.dc, self.previous);
        let _ = DeleteObject(self.bitmap);
        let _ = DeleteDC(self.dc);
      }
    }
  }

  fn draw_cursor(canvas: &Canvas, cursor: &Cursor) -> AuvResult<()> {
    let CursorImage::BuiltIn { variant } = cursor.image() else {
      // TODO(driver-overlay-windows-svg): SVG cursor rasterization is
      // deferred; this crate has no vector-graphics dependency yet. Revisit
      // if a consumer needs custom cursor art rather than the built-in set.
      return Err("windows overlay does not support SVG cursor images in this slice".to_string());
    };

    let style = cursor.style();
    let center = canvas.to_local(cursor.point().point());
    let radius = (style.sprite_size / 2.0).max(1.0).round() as i32;
    let accent = colorref(style.label_background);

    canvas.fill_circle(center, radius, accent);
    if matches!(variant, BuiltInCursor::AuvClick) {
      let ring_radius = (style.sprite_size * 0.8).round() as i32;
      canvas.stroke_circle(center, ring_radius, accent, 2);
    }

    if cursor.label_visible()
      && let Some(label) = cursor.label()
    {
      let anchor = POINT {
        x: center.x + radius + style.label_gap.round() as i32,
        y: center.y,
      };
      canvas.draw_label_pill(
        anchor,
        label,
        colorref(style.label_foreground),
        colorref(style.label_background),
        style.label_padding,
        style.label_corner_radius,
      );
    }

    Ok(())
  }

  fn draw_outline(canvas: &Canvas, outline: &Outline) -> AuvResult<()> {
    let style = outline.style();
    let rect = outline.rect();
    let top_left = canvas.to_local(rect.origin);
    let bottom_right = canvas.to_local(Point::new(rect.origin.x + rect.size.width, rect.origin.y + rect.size.height));
    let padded = RECT {
      left: top_left.x + style.padding.left.round() as i32,
      top: top_left.y + style.padding.top.round() as i32,
      right: bottom_right.x - style.padding.right.round() as i32,
      bottom: bottom_right.y - style.padding.bottom.round() as i32,
    };

    canvas.stroke_rounded_rect(padded, colorref(style.stroke.color), style.stroke.width.round() as i32, style.corner_radius.round() as i32);

    if outline.label_visible()
      && let Some(label) = outline.label()
    {
      let anchor = POINT {
        x: padded.left,
        y: padded.top - 12,
      };
      canvas.draw_label_pill(anchor, label, colorref(style.stroke.color), COLORREF(0x00FF_FFFF), Insets::default(), 6.0);
    }

    Ok(())
  }

  fn draw_status(canvas: &Canvas, status: &Status) -> AuvResult<()> {
    let style = status.style();
    let anchor = canvas.to_local(status.point().point());
    canvas.draw_label_pill(
      anchor,
      status.text(),
      colorref(style.foreground),
      colorref(style.background),
      style.padding,
      style.corner_radius,
    );
    Ok(())
  }

  pub(crate) fn present(layers: &[Layer]) -> AuvResult<()> {
    let hwnd = ensure_window()?;
    let rect = virtual_screen_rect();
    let canvas = Canvas::new(rect)?;

    for layer in layers {
      match layer {
        Layer::Cursor(cursor) => draw_cursor(&canvas, cursor)?,
        Layer::Outline(outline) => draw_outline(&canvas, outline)?,
        Layer::Status(status) => draw_status(&canvas, status)?,
      }
    }

    canvas.finish(hwnd)
  }

  pub(crate) fn hide_all() -> AuvResult<()> {
    let slot = window_slot().lock().map_err(|_| "overlay window state lock poisoned".to_string())?;
    if let Some(raw) = *slot {
      unsafe {
        let _ = ShowWindow(HWND(raw as *mut _), SW_HIDE);
      }
    }
    Ok(())
  }
}

#[cfg(test)]
#[path = "window_test.rs"]
mod tests;
