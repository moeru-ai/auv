//! Native Win32 layered-window overlay renderer.
//!
//! Every [`present`] call redraws all requested layers into one premultiplied
//! BGRA bitmap sized to the virtual screen (see `canvas.rs`, which draws with
//! Direct2D) and blits it onto a single topmost, click-through, alpha-blended
//! window via `UpdateLayeredWindow`. Layers are one-shot visual evidence (see
//! `Overlay::with_layer`'s deferral note in `auv-driver-overlay-common`), so
//! there is no incremental per-layer update path to maintain; each call fully
//! replaces the previous frame.

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

#[cfg(target_os = "windows")]
mod native {
  use std::borrow::Cow;
  use std::sync::{Mutex, OnceLock};

  use auv_driver_common::geometry::Point;
  use auv_driver_overlay_common::Layer;
  use auv_driver_overlay_common::layers::{Cursor, CursorImage, Outline, Status};
  use auv_driver_overlay_common::style::{Color, Insets, Shadow};
  use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
  use windows::Win32::System::LibraryLoader::GetModuleHandleW;
  use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, GetSystemMetrics, IsWindow, RegisterClassExW, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_HIDE, ShowWindow, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
  };

  use crate::AuvResult;
  use crate::canvas::{Canvas, LabelPill, point, px, rect};
  use crate::svg::{BUILT_IN_TIP, SpriteLayout};

  /// Label sizes match the macOS renderer (`Overlay.swift`): 11 pt cursor and outline
  /// labels, 12 pt status text, as device pixels at 96 DPI.
  const LABEL_FONT_SIZE: f32 = 11.0;
  const STATUS_FONT_SIZE: f32 = 12.0;

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
      let hwnd = HWND(raw as *mut _);
      // A window dies with the thread that created it, so a presenter thread that
      // exited (an animator that was stopped) leaves a stale handle behind.
      if unsafe { IsWindow(hwnd) }.as_bool() {
        return Ok(hwnd);
      }
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

  /// Soft drop shadow under the built-in pointer when the style sets none: a dark copy of
  /// its silhouette, slightly lowered and blurred, so its white rim reads on light
  /// backgrounds and its body lifts off dark ones.
  const BUILT_IN_SHADOW: Shadow = Shadow {
    color: Color::rgba(0.0, 0.0, 0.0, 0.35),
    blur_radius: 4.0,
    offset_x: 0.0,
    offset_y: 1.5,
  };

  /// NOTICE: custom SVG placement mirrors the macOS adapter (`placeCursor` in
  /// `Overlay.swift`): the sprite box's top-left sits 4px right of and below the target
  /// point, so an arrow drawn from the box corner hovers just off the target instead of
  /// covering it. As a hotspot, the target is 4px up and left of the box.
  const CUSTOM_SVG_HOTSPOT: (f32, f32) = (-4.0, -4.0);

  /// Draws one cursor: its art, posed about its hotspot with its shadow underneath, then
  /// its label pill.
  ///
  /// Built-in cursors draw the Windows pointer, whose rounded tip is the hotspot, with
  /// `BUILT_IN_SHADOW` unless the style sets a shadow (a transparent one turns it off).
  /// Custom SVG cursors cast only the shadow their style sets.
  fn draw_cursor(canvas: &Canvas, cursor: &Cursor) -> AuvResult<()> {
    let style = cursor.style();
    if let Some(shadow) = style.shadow {
      shadow.validate()?;
    }
    let target = canvas.to_local(cursor.point().point());
    let size = px(style.sprite_size).round().max(1.0) as u32;
    let edge = size as f32;
    let (source, hotspot, shadow) = match cursor.image() {
      CursorImage::BuiltIn { variant } => (
        Cow::Owned(crate::svg::built_in_source(*variant, style.accent)?),
        (edge * BUILT_IN_TIP, edge * BUILT_IN_TIP),
        style.shadow.or(Some(BUILT_IN_SHADOW)),
      ),
      CursorImage::Svg { source } => (Cow::Borrowed(source.as_str()), CUSTOM_SVG_HOTSPOT, style.shadow),
    };
    let layout = SpriteLayout {
      size,
      hotspot,
      pose: cursor.pose(),
    };
    let sprite = crate::svg::rasterize(&source, &layout, shadow.as_ref())?;
    canvas.draw_sprite(point(target.x - sprite.hotspot.0 as f32, target.y - sprite.hotspot.1 as f32), &sprite)?;

    if cursor.label_visible()
      && let Some(label) = cursor.label()
    {
      // The label sits beside the unposed sprite box, so a tilting cursor does not drag
      // its label around.
      let (box_left, box_top) = (target.x - hotspot.0, target.y - hotspot.1);
      let anchor = point(box_left + edge + px(style.label_gap).round(), box_top + edge / 2.0);
      canvas.draw_label_pill(
        anchor,
        &LabelPill {
          text: label,
          foreground: style.label_foreground,
          background: style.label_background,
          padding: style.label_padding,
          corner_radius: style.label_corner_radius,
          font_size: LABEL_FONT_SIZE,
        },
      )?;
    }

    Ok(())
  }

  fn draw_outline(canvas: &Canvas, outline: &Outline) -> AuvResult<()> {
    let style = outline.style();
    let bounds = outline.rect();
    let top_left = canvas.to_local(bounds.origin);
    let bottom_right = canvas.to_local(Point::new(bounds.origin.x + bounds.size.width, bounds.origin.y + bounds.size.height));
    let padded = rect(
      top_left.x + px(style.padding.left).round(),
      top_left.y + px(style.padding.top).round(),
      bottom_right.x - px(style.padding.right).round(),
      bottom_right.y - px(style.padding.bottom).round(),
    );

    let width = px(style.stroke.width).round().max(1.0);
    if padded.right > padded.left && padded.bottom > padded.top {
      // A stroke is centered on its path. Shifting odd widths by half a pixel puts both
      // edges of every straight side on pixel boundaries, so the box stays as crisp as
      // the old GDI pen (which covered the same pixels) and only the corners antialias.
      let shift = (width / 2.0).fract();
      let centerline = rect(padded.left + shift, padded.top + shift, padded.right + shift, padded.bottom + shift);
      canvas.stroke_rounded_rect(centerline, px(style.corner_radius), style.stroke.color, width)?;
    }

    if outline.label_visible()
      && let Some(label) = outline.label()
    {
      // TODO(driver-overlay-windows-outline-label): macOS fills this label pill with
      // the stroke color inside the box (`NativeOverlayOutlineView`); Windows keeps its
      // original white pill above the box until an owner asks for layout parity.
      let anchor = point(padded.left, padded.top - 12.0);
      canvas.draw_label_pill(
        anchor,
        &LabelPill {
          text: label,
          foreground: style.stroke.color,
          background: Color::WHITE,
          padding: Insets::default(),
          corner_radius: 6.0,
          font_size: LABEL_FONT_SIZE,
        },
      )?;
    }

    Ok(())
  }

  fn draw_status(canvas: &Canvas, status: &Status) -> AuvResult<()> {
    let style = status.style();
    let anchor = canvas.to_local(status.point().point());
    canvas.draw_label_pill(
      anchor,
      &LabelPill {
        text: status.text(),
        foreground: style.foreground,
        background: style.background,
        padding: style.padding,
        corner_radius: style.corner_radius,
        font_size: STATUS_FONT_SIZE,
      },
    )
  }

  /// Draws `layers` in order onto an open canvas frame.
  pub(super) fn draw_layers(canvas: &Canvas, layers: &[Layer]) -> AuvResult<()> {
    for layer in layers {
      match layer {
        Layer::Cursor(cursor) => draw_cursor(canvas, cursor)?,
        Layer::Outline(outline) => draw_outline(canvas, outline)?,
        Layer::Status(status) => draw_status(canvas, status)?,
      }
    }
    Ok(())
  }

  pub(crate) fn present(layers: &[Layer]) -> AuvResult<()> {
    let hwnd = ensure_window()?;
    let canvas = Canvas::new(virtual_screen_rect())?;
    draw_layers(&canvas, layers)?;
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
