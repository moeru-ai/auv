//! SVG cursor art rasterization.
//!
//! resvg (which parses with usvg) rasterizes the cursor SVG on the CPU into a
//! premultiplied bitmap at the sprite's pixel size; `canvas.rs` then draws that bitmap
//! with Direct2D. Native Direct2D SVG (`ID2D1SvgDocument`) would need a D2D device
//! context, which this crate deliberately avoids (see `canvas.rs`). resvg is only the
//! cursor-art rasterizer here; every other overlay shape is drawn by Direct2D.
//!
//! NOTICE: resvg is built without its `text`, font and raster-image features, so
//! `<text>` elements and embedded PNG/JPEG/GIF/WebP images do not render. Cursor art is
//! vector paths. Revisit if a consumer needs text or bitmaps inside cursor SVGs.

use resvg::{tiny_skia, usvg};

use crate::AuvResult;

/// Largest accepted SVG source, matching the macOS adapter's runtime limit.
const MAX_SOURCE_BYTES: usize = 256 * 1024;

/// Largest sprite edge in pixels. Bounds the bitmap allocation for an untrusted
/// `sprite_size` (1024 x 1024 x 4 bytes = 4 MiB).
const MAX_SPRITE_PX: u32 = 1024;

/// Premultiplied BGRA cursor art, top row first, `size` pixels square.
pub(crate) struct Sprite {
  pub size: u32,
  pub bgra: Vec<u8>,
}

/// Rasterizes `source` into a `size` x `size` sprite. The art is scaled uniformly to fit
/// and centered, so non-square artwork keeps its aspect ratio.
pub(crate) fn rasterize(source: &str, size: u32) -> AuvResult<Sprite> {
  if source.trim().is_empty() || source.len() > MAX_SOURCE_BYTES {
    return Err("cursor SVG must be non-empty and at most 256 KiB".to_string());
  }
  if size == 0 || size > MAX_SPRITE_PX {
    return Err(format!("cursor sprite size must be between 1 and {MAX_SPRITE_PX} pixels, got {size}"));
  }

  let mut options = usvg::Options::default();
  // Overlay SVG can come from remote callers. usvg's default resolver reads `<image href>`
  // paths from the local disk; refuse every external reference. Inline `data:` URLs still
  // resolve through the default data resolver.
  options.image_href_resolver.resolve_string = Box::new(|_, _| None);
  let tree = usvg::Tree::from_str(source, &options).map_err(|error| format!("invalid cursor SVG: {error}"))?;

  let mut pixmap = tiny_skia::Pixmap::new(size, size).ok_or_else(|| "failed to allocate cursor sprite bitmap".to_string())?;
  let art = tree.size();
  let edge = size as f32;
  let scale = (edge / art.width()).min(edge / art.height());
  let transform =
    tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, (edge - art.width() * scale) / 2.0, (edge - art.height() * scale) / 2.0);
  resvg::render(&tree, transform, &mut pixmap.as_mut());

  // tiny-skia stores premultiplied RGBA; Direct2D's B8G8R8A8 wants the same values with
  // red and blue swapped.
  let mut bgra = pixmap.take();
  for pixel in bgra.as_chunks_mut::<4>().0 {
    pixel.swap(0, 2);
  }
  Ok(Sprite { size, bgra })
}

#[cfg(test)]
#[path = "svg_test.rs"]
mod tests;
