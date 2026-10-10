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
  use std::sync::{Mutex, OnceLock};

  use auv_driver_common::geometry::Point;
  use auv_driver_overlay_common::Layer;
  use auv_driver_overlay_common::layers::{BuiltInCursor, Cursor, CursorImage, Outline, Status};
  use auv_driver_overlay_common::style::{Color, CursorStyle, Insets};
  use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
  use windows::Win32::System::LibraryLoader::GetModuleHandleW;
  use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, GetSystemMetrics, RegisterClassExW, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_HIDE, ShowWindow, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
  };

  use crate::AuvResult;
  use windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F;

  use crate::canvas::{Canvas, LabelPill, point, px, rect};

  /// Label sizes match the macOS renderer (`Overlay.swift`): 11 pt cursor and outline
  /// labels, 12 pt status text, as device pixels at 96 DPI.
  const LABEL_FONT_SIZE: f32 = 11.0;
  const STATUS_FONT_SIZE: f32 = 12.0;

  /// Radius of the disc the glow follows behind SVG art, as a fraction of the sprite
  /// edge. Cursor art fills most of its box without reaching the corners.
  const SVG_GLOW_SILHOUETTE: f32 = 0.35;

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

  /// Draws one cursor: its sprite (with an optional glow behind it) and its label pill.
  fn draw_cursor(canvas: &Canvas, cursor: &Cursor) -> AuvResult<()> {
    let style = cursor.style();
    if let Some(shadow) = style.shadow {
      shadow.validate()?;
    }
    let target = canvas.to_local(cursor.point().point());
    let (sprite_right, sprite_middle) = match cursor.image() {
      CursorImage::BuiltIn { variant } => draw_disc_sprite(canvas, target, *variant, &style)?,
      CursorImage::Svg { source } => draw_svg_sprite(canvas, target, source, &style)?,
    };

    if cursor.label_visible()
      && let Some(label) = cursor.label()
    {
      let anchor = point(sprite_right + px(style.label_gap).round(), sprite_middle);
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

  /// Draws the Windows built-in sprite, a disc centered on the target point, and returns
  /// the sprite's right edge and vertical middle for label placement.
  fn draw_disc_sprite(canvas: &Canvas, center: D2D_POINT_2F, variant: BuiltInCursor, style: &CursorStyle) -> AuvResult<(f32, f32)> {
    let radius = px(style.sprite_size / 2.0).max(1.0).round();
    let accent = style.label_background;

    // TODO(driver-overlay-windows-builtin-art): built-in cursors keep the
    // Windows disc sprite. macOS swaps Auv/AuvClick for their canonical SVG art
    // and a default `Shadow::auv()` glow (`cursor_for_rendering` in
    // auv-driver-overlay-macos); here only an explicit shadow glows. Adopt the
    // macOS defaults when the owner asks for built-in art parity.
    if let Some(shadow) = &style.shadow {
      canvas.draw_glow(center, radius, shadow)?;
    }
    canvas.fill_circle(center, radius, accent)?;
    if matches!(variant, BuiltInCursor::AuvClick) {
      let ring_radius = px(style.sprite_size * 0.8).round();
      canvas.stroke_circle(center, ring_radius, accent, 2.0)?;
    }
    Ok((center.x + radius, center.y))
  }

  /// Draws custom SVG art and returns the sprite's right edge and vertical middle.
  ///
  /// NOTICE: placement mirrors the macOS adapter (`placeCursor` in `Overlay.swift`):
  /// the sprite box's top-left sits 4px right of and below the target point, so an
  /// arrow drawn from the box corner hovers just off the target instead of covering it.
  fn draw_svg_sprite(canvas: &Canvas, target: D2D_POINT_2F, source: &str, style: &CursorStyle) -> AuvResult<(f32, f32)> {
    const SPRITE_OFFSET: f32 = 4.0;
    let size = px(style.sprite_size).round().max(1.0) as u32;
    let sprite = crate::svg::rasterize(source, size)?;
    let edge = sprite.size as f32;
    let left = target.x + SPRITE_OFFSET;
    let top = target.y + SPRITE_OFFSET;
    let middle = point(left + edge / 2.0, top + edge / 2.0);

    if let Some(shadow) = &style.shadow {
      // TODO(driver-overlay-windows-silhouette-shadow): the glow is radial around the
      // sprite box, not a blur of the art's own silhouette as on macOS. A silhouette blur
      // needs a blur of the rasterized alpha (no D2D effects without a device context);
      // revisit if the round halo reads wrong for non-round cursor art.
      canvas.draw_glow(middle, edge * SVG_GLOW_SILHOUETTE, shadow)?;
    }
    canvas.draw_sprite(point(left, top), &sprite)?;
    Ok((left + edge, middle.y))
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
