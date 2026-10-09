//! Direct2D drawing surface for one overlay frame.
//!
//! The surface is a 32-bit top-down GDI DIB section that `window.rs` hands to
//! `UpdateLayeredWindow`. Drawing goes through an `ID2D1DCRenderTarget` bound to
//! the DIB's device context, which writes straight into the DIB as
//! *premultiplied* BGRA. That is exactly what `UpdateLayeredWindow` with
//! `AC_SRC_ALPHA` expects, so antialiased edges and translucent fills reach the
//! screen with real alpha.
//!
//! Why a DC render target and not a D2D device or swap chain: a layered window is
//! presented with `UpdateLayeredWindow`, which cannot take a DXGI swap chain, and
//! a D2D device would couple the overlay to another crate's D3D device
//! (`auv-driver-windows`' WGC context). The DC render target needs neither.
//!
//! GDI primitives must not be mixed into the same DC while a frame is open
//! (`BeginDraw` .. `EndDraw`); this module only ever draws through Direct2D and
//! DirectWrite.

use std::sync::OnceLock;

use auv_driver_overlay_common::style::{Color, Insets, Shadow};
use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::{
  D2D_POINT_2F, D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
  D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_BITMAP_PROPERTIES, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
  D2D1_EXTEND_MODE_CLAMP, D2D1_FACTORY_TYPE_MULTI_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_GAMMA_2_2,
  D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_RENDER_TARGET_USAGE_NONE,
  D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory, ID2D1DCRenderTarget, ID2D1Factory, ID2D1SolidColorBrush,
  ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
  DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_METRICS,
  DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory, IDWriteFactory, IDWriteTextLayout,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
  AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC,
  DeleteObject, HBITMAP, HDC, HGDIOBJ, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{SW_SHOWNOACTIVATE, ShowWindow, ULW_ALPHA, UpdateLayeredWindow};
use windows::core::w;

use crate::AuvResult;
use crate::svg::Sprite;

/// Font used for every label. A monospaced semibold face matches the macOS renderer
/// (`NSFont.monospacedSystemFont(ofSize:weight: .semibold)` in `Overlay.swift`).
///
/// NOTICE: Consolas ships with every supported Windows SKU, so measured pill sizes do
/// not depend on optional fonts. It has no semibold face; DirectWrite resolves the
/// request to its bold face. Glyphs Consolas lacks (CJK, emoji) come from DirectWrite's
/// system font fallback.
const LABEL_FONT_FAMILY: windows::core::PCWSTR = w!("Consolas");

static D2D_FACTORY: OnceLock<ID2D1Factory> = OnceLock::new();
static DWRITE_FACTORY: OnceLock<IDWriteFactory> = OnceLock::new();

/// Multithreaded because `present()` carries no thread-affinity guarantee: the factory
/// and the resources it creates may be shared across threads.
fn d2d_factory() -> AuvResult<&'static ID2D1Factory> {
  if let Some(factory) = D2D_FACTORY.get() {
    return Ok(factory);
  }
  let factory: ID2D1Factory = unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_MULTI_THREADED, None) }
    .map_err(|error| format!("failed to create Direct2D factory: {error}"))?;
  Ok(D2D_FACTORY.get_or_init(|| factory))
}

fn dwrite_factory() -> AuvResult<&'static IDWriteFactory> {
  if let Some(factory) = DWRITE_FACTORY.get() {
    return Ok(factory);
  }
  let factory: IDWriteFactory =
    unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }.map_err(|error| format!("failed to create DirectWrite factory: {error}"))?;
  Ok(DWRITE_FACTORY.get_or_init(|| factory))
}

/// Saturating, non-failing conversions: untrusted layer geometry must not reach Direct2D
/// as NaN or infinity (the GDI path got the same behavior from `as i32` casts).
pub(crate) fn px(value: f64) -> f32 {
  if value.is_finite() {
    value.clamp(-1.0e6, 1.0e6) as f32
  } else {
    0.0
  }
}

fn channel(value: f64) -> f32 {
  if value.is_finite() {
    value.clamp(0.0, 1.0) as f32
  } else {
    0.0
  }
}

fn d2d_color(color: Color) -> D2D1_COLOR_F {
  D2D1_COLOR_F {
    r: channel(color.red),
    g: channel(color.green),
    b: channel(color.blue),
    a: channel(color.alpha),
  }
}

pub(crate) fn point(x: f32, y: f32) -> D2D_POINT_2F {
  D2D_POINT_2F { x, y }
}

pub(crate) fn rect(left: f32, top: f32, right: f32, bottom: f32) -> D2D_RECT_F {
  D2D_RECT_F {
    left,
    top,
    right,
    bottom,
  }
}

/// Radii larger than half of either side collapse to a full pill instead of overshooting.
fn clamp_radius(bounds: D2D_RECT_F, radius: f32) -> f32 {
  let half = ((bounds.right - bounds.left).min(bounds.bottom - bounds.top) / 2.0).max(0.0);
  radius.clamp(0.0, half)
}

/// A rounded text pill: the shared look of cursor labels, outline labels and status text.
pub(crate) struct LabelPill<'a> {
  pub text: &'a str,
  pub foreground: Color,
  pub background: Color,
  pub padding: Insets,
  pub corner_radius: f64,
  pub font_size: f32,
}

/// GDI backing store for one frame. Dropped after the render target that draws into it.
struct Dib {
  dc: HDC,
  bitmap: HBITMAP,
  previous: HGDIOBJ,
  /// Pixel memory owned by `bitmap`. Production hands the DC to `UpdateLayeredWindow`;
  /// only tests read the pixels back.
  #[cfg(test)]
  bits: *mut u8,
  width: i32,
  height: i32,
}

impl Dib {
  fn new(width: i32, height: i32) -> AuvResult<Self> {
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
    let bitmap = match unsafe { CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) } {
      Ok(bitmap) => bitmap,
      Err(error) => {
        unsafe {
          let _ = DeleteDC(dc);
        }
        return Err(format!("failed to create overlay bitmap: {error}"));
      }
    };
    if bits.is_null() {
      unsafe {
        let _ = DeleteObject(bitmap);
        let _ = DeleteDC(dc);
      }
      return Err("overlay bitmap allocation returned a null pixel buffer".to_string());
    }

    let previous = unsafe { SelectObject(dc, bitmap) };
    Ok(Self {
      dc,
      bitmap,
      previous,
      #[cfg(test)]
      bits: bits.cast::<u8>(),
      width,
      height,
    })
  }

  #[cfg(test)]
  fn pixels(&self) -> &[u8] {
    unsafe { std::slice::from_raw_parts(self.bits, (self.width as usize) * (self.height as usize) * 4) }
  }
}

impl Drop for Dib {
  fn drop(&mut self) {
    unsafe {
      SelectObject(self.dc, self.previous);
      let _ = DeleteObject(self.bitmap);
      let _ = DeleteDC(self.dc);
    }
  }
}

/// Offscreen premultiplied-BGRA canvas backing one `present()` frame.
///
/// [`Canvas::new`] opens a Direct2D frame and clears it to transparent; drawing
/// methods append to it; [`Canvas::finish`] closes the frame and blits it onto the
/// layered window. Field order matters: the render target must drop before the DIB it
/// is bound to.
pub(crate) struct Canvas {
  target: ID2D1DCRenderTarget,
  dib: Dib,
  origin: POINT,
}

impl Canvas {
  pub(crate) fn new(bounds: RECT) -> AuvResult<Self> {
    let width = bounds.right - bounds.left;
    let height = bounds.bottom - bounds.top;
    let dib = Dib::new(width, height)?;

    // NOTICE: `D2D1_RENDER_TARGET_TYPE_SOFTWARE` keeps rasterization on the CPU so the
    // pixels do not depend on the GPU or driver (a DC target would otherwise be free to
    // use a hardware path and read the result back). DPI is pinned to 96 so one DIP is
    // one device pixel, matching the pixel coordinates the layers carry; the default
    // would follow the system DPI and silently scale every shape.
    let properties = D2D1_RENDER_TARGET_PROPERTIES {
      r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
      pixelFormat: D2D1_PIXEL_FORMAT {
        format: DXGI_FORMAT_B8G8R8A8_UNORM,
        alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
      },
      dpiX: 96.0,
      dpiY: 96.0,
      usage: D2D1_RENDER_TARGET_USAGE_NONE,
      minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
    };
    let target = unsafe { d2d_factory()?.CreateDCRenderTarget(&properties) }
      .map_err(|error| format!("failed to create Direct2D render target: {error}"))?;
    let surface = RECT {
      left: 0,
      top: 0,
      right: width,
      bottom: height,
    };
    unsafe {
      target.BindDC(dib.dc, &surface).map_err(|error| format!("failed to bind Direct2D render target to the overlay bitmap: {error}"))?;
      target.BeginDraw();
      // Layered windows have no opaque background to blend ClearType against, so text
      // must use grayscale antialiasing to stay alpha-correct.
      target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
      target.Clear(Some(&D2D1_COLOR_F {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
      }));
    }

    Ok(Self {
      target,
      dib,
      origin: POINT {
        x: bounds.left,
        y: bounds.top,
      },
    })
  }

  /// Maps a screen-space point into canvas pixels. The coordinates are rounded to whole
  /// device pixels, as before the Direct2D move, so layouts do not shift; antialiasing
  /// then only softens curved edges.
  pub(crate) fn to_local(&self, screen: auv_driver_common::geometry::Point) -> D2D_POINT_2F {
    point(px(screen.x - f64::from(self.origin.x)).round(), px(screen.y - f64::from(self.origin.y)).round())
  }

  fn brush(&self, color: Color) -> AuvResult<ID2D1SolidColorBrush> {
    unsafe { self.target.CreateSolidColorBrush(&d2d_color(color), None) }.map_err(|error| format!("failed to create overlay brush: {error}"))
  }

  pub(crate) fn fill_circle(&self, center: D2D_POINT_2F, radius: f32, color: Color) -> AuvResult<()> {
    let brush = self.brush(color)?;
    let ellipse = D2D1_ELLIPSE {
      point: center,
      radiusX: radius.max(1.0),
      radiusY: radius.max(1.0),
    };
    unsafe { self.target.FillEllipse(&ellipse, &brush) };
    Ok(())
  }

  pub(crate) fn stroke_circle(&self, center: D2D_POINT_2F, radius: f32, color: Color, width: f32) -> AuvResult<()> {
    let brush = self.brush(color)?;
    let ellipse = D2D1_ELLIPSE {
      point: center,
      radiusX: radius.max(1.0),
      radiusY: radius.max(1.0),
    };
    unsafe { self.target.DrawEllipse(&ellipse, &brush, width.max(1.0), None::<&ID2D1StrokeStyle>) };
    Ok(())
  }

  pub(crate) fn fill_rounded_rect(&self, bounds: D2D_RECT_F, radius: f32, color: Color) -> AuvResult<()> {
    let brush = self.brush(color)?;
    let radius = clamp_radius(bounds, radius);
    let rounded = D2D1_ROUNDED_RECT {
      rect: bounds,
      radiusX: radius,
      radiusY: radius,
    };
    unsafe { self.target.FillRoundedRectangle(&rounded, &brush) };
    Ok(())
  }

  /// Strokes a rounded rectangle centered on `bounds`' edges.
  pub(crate) fn stroke_rounded_rect(&self, bounds: D2D_RECT_F, radius: f32, color: Color, width: f32) -> AuvResult<()> {
    let brush = self.brush(color)?;
    let radius = clamp_radius(bounds, radius);
    let rounded = D2D1_ROUNDED_RECT {
      rect: bounds,
      radiusX: radius,
      radiusY: radius,
    };
    unsafe { self.target.DrawRoundedRectangle(&rounded, &brush, width.max(1.0), None::<&ID2D1StrokeStyle>) };
    Ok(())
  }

  /// Draws a filled pill behind `pill.text`, anchored with its left edge at `anchor.x`
  /// and its vertical center at `anchor.y`. The pill is sized from DirectWrite's measured
  /// text extents plus padding, so it never clips the glyphs.
  pub(crate) fn draw_label_pill(&self, anchor: D2D_POINT_2F, pill: &LabelPill) -> AuvResult<()> {
    let (layout, text_width, text_height) = measure_text(pill.text, pill.font_size)?;
    let padding = pill.padding;
    let width = text_width + px(padding.left) + px(padding.right);
    let height = text_height + px(padding.top) + px(padding.bottom);
    let left = anchor.x.round();
    let top = (anchor.y - height / 2.0).round();
    let bounds = rect(left, top, left + width, top + height);

    self.fill_rounded_rect(bounds, px(pill.corner_radius), pill.background)?;

    let brush = self.brush(pill.foreground)?;
    let origin = point((left + px(padding.left)).round(), (top + px(padding.top)).round());
    unsafe { self.target.DrawTextLayout(origin, &layout, &brush, D2D1_DRAW_TEXT_OPTIONS_NONE) };
    Ok(())
  }

  /// Draws a soft radial glow, the Direct2D stand-in for a blurred silhouette shadow.
  ///
  /// `silhouette_radius` is the radius of the disc whose blurred edge the glow follows.
  /// The falloff samples a Gaussian-blurred disc (sigma = blur radius / 2, the same
  /// relation Core Graphics uses) into gradient stops, so the halo reads like the macOS
  /// `Shadow::auv()` glow without needing a D2D effect (which would need a device context).
  pub(crate) fn draw_glow(&self, center: D2D_POINT_2F, silhouette_radius: f32, shadow: &Shadow) -> AuvResult<()> {
    let color = shadow.color;
    let peak = channel(color.alpha);
    if peak <= 0.0 {
      return Ok(());
    }
    let center = point(center.x + px(shadow.offset_x), center.y + px(shadow.offset_y));
    let blur = px(shadow.blur_radius).max(0.0);
    let silhouette_radius = silhouette_radius.max(1.0);

    if blur < 0.5 {
      // No blur: the shadow is just the offset silhouette.
      return self.fill_circle(center, silhouette_radius, color);
    }

    let sigma = blur / 2.0;
    let reach = silhouette_radius + 3.0 * sigma;
    const STOPS: usize = 16;
    let stops = (0..STOPS)
      .map(|index| {
        let position = index as f32 / (STOPS - 1) as f32;
        let distance = position * reach;
        // Logistic approximation of the normal CDF (max error ~0.01), plenty for a glow.
        let coverage = 1.0 / (1.0 + (-1.702 * (silhouette_radius - distance) / sigma).exp());
        D2D1_GRADIENT_STOP {
          position,
          color: D2D1_COLOR_F {
            a: peak * coverage,
            ..d2d_color(color)
          },
        }
      })
      .collect::<Vec<_>>();

    let collection = unsafe { self.target.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP) }
      .map_err(|error| format!("failed to create glow gradient: {error}"))?;
    let properties = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
      center,
      gradientOriginOffset: point(0.0, 0.0),
      radiusX: reach,
      radiusY: reach,
    };
    let brush = unsafe { self.target.CreateRadialGradientBrush(&properties, None, &collection) }
      .map_err(|error| format!("failed to create glow brush: {error}"))?;
    let extent = D2D1_ELLIPSE {
      point: center,
      radiusX: reach,
      radiusY: reach,
    };
    unsafe { self.target.FillEllipse(&extent, &brush) };
    Ok(())
  }

  /// Draws premultiplied BGRA art 1:1 with its top-left corner at `top_left`.
  pub(crate) fn draw_sprite(&self, top_left: D2D_POINT_2F, sprite: &Sprite) -> AuvResult<()> {
    let properties = D2D1_BITMAP_PROPERTIES {
      pixelFormat: D2D1_PIXEL_FORMAT {
        format: DXGI_FORMAT_B8G8R8A8_UNORM,
        alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
      },
      dpiX: 96.0,
      dpiY: 96.0,
    };
    let size = D2D_SIZE_U {
      width: sprite.size,
      height: sprite.size,
    };
    let bitmap = unsafe { self.target.CreateBitmap(size, Some(sprite.bgra.as_ptr().cast()), sprite.size * 4, &properties) }
      .map_err(|error| format!("failed to upload cursor sprite: {error}"))?;
    let edge = sprite.size as f32;
    let destination = rect(top_left.x, top_left.y, top_left.x + edge, top_left.y + edge);
    // The art is rasterized at its final pixel size and placed on whole pixels, so
    // nearest-neighbor copies it exactly instead of resampling it.
    unsafe { self.target.DrawBitmap(&bitmap, Some(&destination), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, None) };
    Ok(())
  }

  fn end_draw(&self) -> AuvResult<()> {
    unsafe { self.target.EndDraw(None, None) }.map_err(|error| format!("failed to finish Direct2D overlay frame: {error}"))
  }

  /// Closes the Direct2D frame and blits it onto the layered window.
  pub(crate) fn finish(&self, hwnd: HWND) -> AuvResult<()> {
    self.end_draw()?;

    let size = SIZE {
      cx: self.dib.width,
      cy: self.dib.height,
    };
    let src_point = POINT { x: 0, y: 0 };
    let blend = BLENDFUNCTION {
      BlendOp: AC_SRC_OVER as u8,
      BlendFlags: 0,
      SourceConstantAlpha: 255,
      AlphaFormat: AC_SRC_ALPHA as u8,
    };

    unsafe {
      UpdateLayeredWindow(hwnd, None, Some(&self.origin), Some(&size), self.dib.dc, Some(&src_point), COLORREF(0), Some(&blend), ULW_ALPHA)
    }
    .map_err(|error| format!("failed to update overlay layered window: {error}"))?;

    unsafe {
      let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    Ok(())
  }

  /// Closes the frame and returns the premultiplied BGRA pixels, top row first.
  #[cfg(test)]
  pub(crate) fn into_pixels(self) -> AuvResult<Vec<u8>> {
    self.end_draw()?;
    Ok(self.dib.pixels().to_vec())
  }

  #[cfg(test)]
  pub(crate) fn size(&self) -> (usize, usize) {
    (self.dib.width as usize, self.dib.height as usize)
  }
}

/// Lays `text` out on one line and returns the layout with its whole-pixel extents.
fn measure_text(text: &str, font_size: f32) -> AuvResult<(IDWriteTextLayout, f32, f32)> {
  let factory = dwrite_factory()?;
  let format = unsafe {
    factory.CreateTextFormat(
      LABEL_FONT_FAMILY,
      None,
      DWRITE_FONT_WEIGHT_SEMI_BOLD,
      DWRITE_FONT_STYLE_NORMAL,
      DWRITE_FONT_STRETCH_NORMAL,
      font_size,
      w!("en-us"),
    )
  }
  .map_err(|error| format!("failed to create label text format: {error}"))?;
  unsafe { format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP) }
    .map_err(|error| format!("failed to configure label text format: {error}"))?;

  let wide: Vec<u16> = text.encode_utf16().collect();
  let layout =
    unsafe { factory.CreateTextLayout(&wide, &format, 1.0e5, 1.0e5) }.map_err(|error| format!("failed to lay out label text: {error}"))?;
  let mut metrics = DWRITE_TEXT_METRICS::default();
  unsafe { layout.GetMetrics(&mut metrics) }.map_err(|error| format!("failed to measure label text: {error}"))?;
  Ok((layout, metrics.widthIncludingTrailingWhitespace.ceil(), metrics.height.ceil()))
}

#[cfg(test)]
#[path = "canvas_test.rs"]
mod tests;
