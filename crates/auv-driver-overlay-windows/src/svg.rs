//! Cursor art rasterization.
//!
//! resvg (which parses with usvg) rasterizes the cursor SVG on the CPU into a premultiplied
//! bitmap, already turned and sized to the cursor's pose and with its shadow composited
//! underneath; `canvas.rs` then draws that bitmap 1:1 with Direct2D. Native Direct2D SVG
//! (`ID2D1SvgDocument`) and the Direct2D shadow effect would both need a D2D device
//! context, which this crate deliberately avoids (see `canvas.rs`). resvg is only the
//! cursor-art rasterizer here; every other overlay shape is drawn by Direct2D.
//!
//! NOTICE: resvg is built without its `text`, font and raster-image features, so
//! `<text>` elements and embedded PNG/JPEG/GIF/WebP images do not render. Cursor art is
//! vector paths. Revisit if a consumer needs text or bitmaps inside cursor SVGs.

use auv_driver_overlay_common::layers::{BuiltInCursor, CursorPose};
use auv_driver_overlay_common::style::{Color, Shadow};
use resvg::{tiny_skia, usvg};

use crate::AuvResult;

/// Largest accepted SVG source, matching the macOS adapter's runtime limit.
const MAX_SOURCE_BYTES: usize = 256 * 1024;

/// Most `<use>` elements a cursor SVG may contain.
///
/// NOTICE: usvg expands every `<use>` into a copy of its target at parse time, and nested
/// references multiply: a 1 KB SVG whose 16 groups each use the previous group twice took
/// 5 s to parse and render (measured 2026-10-09, depth 12: 0.3 s, depth 16: 5.1 s), well
/// inside the 256 KiB size limit. With at most 16 `<use>` elements the expansion is bounded
/// to a few hundred copies whatever the nesting. Cursor art is a handful of paths and does
/// not need symbol reuse. Raise this only together with a work bound on the expansion.
const MAX_USE_ELEMENTS: usize = 16;

/// Largest sprite edge in pixels. Bounds the bitmap allocation for an untrusted
/// `sprite_size` (1024 x 1024 x 4 bytes = 4 MiB).
const MAX_SPRITE_PX: u32 = 1024;

/// Largest posed bitmap edge in pixels. A largest sprite turned 45 degrees with its
/// shadow still fits; an untrusted pose that scales the art up further is rejected.
const MAX_BITMAP_PX: u32 = 2048;

/// Where the built-in pointer's hotspot sits in its box, as a fraction of the box edge:
/// (1, 1) of the 24-unit art (see `assets/cursor-pointer.svg`).
pub(crate) const BUILT_IN_TIP: f32 = 1.0 / 24.0;

/// The Windows built-in cursor art for `variant`: a rounded pointer with a white rim, the
/// "Vector modern" design the owner chose on 2026-10-10 from rendered candidates.
///
/// Each variant fills the pointer with a light-to-deep gradient: AUV cyan for the AUV
/// cursor, a lighter cyan while pressed, and the `YOU_SLATE` family for the user cursor.
pub(crate) fn built_in_source(variant: BuiltInCursor) -> String {
  let (top, bottom) = match variant {
    BuiltInCursor::Auv => ("#2fd3df", "#0896a6"),
    BuiltInCursor::AuvClick => ("#8cecf2", "#25bccb"),
    BuiltInCursor::You => ("#51647f", "#2a3a52"),
  };
  include_str!("../assets/cursor-pointer.svg").replace("{TOP}", top).replace("{BOTTOM}", bottom)
}

/// How one cursor's art is laid out for a frame.
pub(crate) struct SpriteLayout {
  /// Edge of the square the art is scaled into, in pixels.
  pub size: u32,
  /// The point the cursor acts on, in pixels from the top-left of that square. It may lie
  /// outside the square, as for custom SVG art drawn beside the target.
  pub hotspot: (f32, f32),
  /// Turn and scale of the art about the hotspot.
  pub pose: CursorPose,
}

/// Rasterized cursor art: premultiplied BGRA, top row first.
pub(crate) struct Sprite {
  pub width: u32,
  pub height: u32,
  pub bgra: Vec<u8>,
  /// Where the hotspot falls in the bitmap, in whole pixels from its top-left. Drawing the
  /// bitmap at `target - hotspot` puts the hotspot on the target.
  pub hotspot: (i32, i32),
}

/// Rasterizes `source` laid out as `layout`, with `shadow` composited underneath.
///
/// The art is scaled uniformly to fit the layout's square and centered, so non-square
/// artwork keeps its aspect ratio; then it is turned and scaled about the hotspot. The
/// shadow is the posed art's own silhouette, offset and blurred, so it follows any shape.
pub(crate) fn rasterize(source: &str, layout: &SpriteLayout, shadow: Option<&Shadow>) -> AuvResult<Sprite> {
  if layout.size == 0 || layout.size > MAX_SPRITE_PX {
    return Err(format!("cursor sprite size must be between 1 and {MAX_SPRITE_PX} pixels, got {}", layout.size));
  }
  layout.pose.validate()?;
  let tree = parse(source)?;

  let art = tree.size();
  let edge = layout.size as f32;
  let fit = (edge / art.width()).min(edge / art.height());

  // Sprite square -> bitmap: subtract the hotspot, turn and scale (`x' = a x + b y`,
  // `y' = c x + d y`; a positive tilt turns clockwise on screen), then shift by the
  // bitmap's hotspot so everything lands inside.
  let (sin, cos) = (layout.pose.tilt_degrees.to_radians() as f32).sin_cos();
  let scale = layout.pose.scale as f32;
  let (a, b, c, d) = (scale * cos, -scale * sin, scale * sin, scale * cos);
  let posed = |x: f32, y: f32| {
    let (x, y) = (x - layout.hotspot.0, y - layout.hotspot.1);
    (a * x + b * y, c * x + d * y)
  };
  let corners = [
    posed(0.0, 0.0),
    posed(edge, 0.0),
    posed(0.0, edge),
    posed(edge, edge),
  ];
  let (min_x, max_x) = corners.iter().fold((f32::MAX, f32::MIN), |(low, high), corner| (low.min(corner.0), high.max(corner.0)));
  let (min_y, max_y) = corners.iter().fold((f32::MAX, f32::MIN), |(low, high), corner| (low.min(corner.1), high.max(corner.1)));
  // One pixel of room for antialiasing, plus the shadow's reach.
  let margin = 1.0 + shadow.map_or(0.0, shadow_reach);
  let hotspot = ((margin - min_x).ceil(), (margin - min_y).ceil());
  let (width, height) = ((max_x + hotspot.0 + margin).ceil(), (max_y + hotspot.1 + margin).ceil());
  if !(1.0..=MAX_BITMAP_PX as f32).contains(&width) || !(1.0..=MAX_BITMAP_PX as f32).contains(&height) {
    return Err(format!("posed cursor sprite must fit in {MAX_BITMAP_PX} x {MAX_BITMAP_PX} pixels"));
  }
  let (width, height) = (width as u32, height as u32);

  // Art units -> bitmap: the fit scale, then the pose. The art's origin lands where the
  // square's centered origin lands.
  let origin = posed((edge - art.width() * fit) / 2.0, (edge - art.height() * fit) / 2.0);
  let place = |shift_x: f32, shift_y: f32| {
    tiny_skia::Transform::from_row(a * fit, c * fit, b * fit, d * fit, origin.0 + hotspot.0 + shift_x, origin.1 + hotspot.1 + shift_y)
  };

  let mut rgba = render(&tree, place(0.0, 0.0), width, height)?;
  if let Some(shadow) = shadow
    && shadow.color.alpha > 0.0
  {
    let silhouette = render(&tree, place(shadow.offset_x as f32, shadow.offset_y as f32), width, height)?;
    let mut coverage = silhouette.as_chunks::<4>().0.iter().map(|pixel| f32::from(pixel[3]) / 255.0).collect::<Vec<_>>();
    blur(&mut coverage, width as usize, height as usize, shadow.blur_radius as f32 / 2.0);
    paint_under(&mut rgba, &coverage, shadow.color);
  }

  // tiny-skia stores premultiplied RGBA; Direct2D's B8G8R8A8 wants the same values with
  // red and blue swapped.
  for pixel in rgba.as_chunks_mut::<4>().0 {
    pixel.swap(0, 2);
  }
  Ok(Sprite {
    width,
    height,
    bgra: rgba,
    hotspot: (hotspot.0 as i32, hotspot.1 as i32),
  })
}

fn parse(source: &str) -> AuvResult<usvg::Tree> {
  if source.trim().is_empty() || source.len() > MAX_SOURCE_BYTES {
    return Err("cursor SVG must be non-empty and at most 256 KiB".to_string());
  }
  if count_use_elements(source) > MAX_USE_ELEMENTS {
    return Err(format!("cursor SVG may contain at most {MAX_USE_ELEMENTS} <use> elements"));
  }
  let mut options = usvg::Options::default();
  // Overlay SVG can come from remote callers. usvg's default resolver reads `<image href>`
  // paths from the local disk; refuse every external reference. Inline `data:` URLs still
  // resolve through the default data resolver.
  options.image_href_resolver.resolve_string = Box::new(|_, _| None);
  usvg::Tree::from_str(source, &options).map_err(|error| format!("invalid cursor SVG: {error}"))
}

/// Renders `tree` through `transform` into a fresh transparent premultiplied RGBA bitmap.
fn render(tree: &usvg::Tree, transform: tiny_skia::Transform, width: u32, height: u32) -> AuvResult<Vec<u8>> {
  let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or_else(|| "failed to allocate cursor sprite bitmap".to_string())?;
  resvg::render(tree, transform, &mut pixmap.as_mut());
  Ok(pixmap.take())
}

/// How far a shadow can reach beyond the art: its offset plus three blur sigmas
/// (sigma = blur radius / 2, the relation Core Graphics uses for the macOS shadow).
fn shadow_reach(shadow: &Shadow) -> f32 {
  (1.5 * shadow.blur_radius + shadow.offset_x.abs().max(shadow.offset_y.abs())) as f32
}

/// Blurs a coverage plane with a Gaussian of standard deviation `sigma`, as two separable
/// passes cut at three sigmas. Below a quarter pixel the blur is invisible and skipped.
fn blur(plane: &mut [f32], width: usize, height: usize, sigma: f32) {
  if sigma < 0.25 {
    return;
  }
  let radius = (3.0 * sigma).ceil() as usize;
  let mut kernel = (0..=2 * radius)
    .map(|index| {
      let distance = index as f32 - radius as f32;
      (-distance * distance / (2.0 * sigma * sigma)).exp()
    })
    .collect::<Vec<_>>();
  let total = kernel.iter().sum::<f32>();
  kernel.iter_mut().for_each(|weight| *weight /= total);

  let mut pass = vec![0.0; plane.len()];
  for y in 0..height {
    for x in 0..width {
      let first = x.saturating_sub(radius);
      let last = (x + radius).min(width - 1);
      pass[y * width + x] = (first..=last).map(|source| plane[y * width + source] * kernel[source + radius - x]).sum();
    }
  }
  for y in 0..height {
    for x in 0..width {
      let first = y.saturating_sub(radius);
      let last = (y + radius).min(height - 1);
      plane[y * width + x] = (first..=last).map(|source| pass[source * width + x] * kernel[source + radius - y]).sum();
    }
  }
}

/// Paints `color` at `coverage` underneath premultiplied RGBA art: wherever the art is
/// transparent the shadow shows, and an opaque art pixel hides it.
fn paint_under(rgba: &mut [u8], coverage: &[f32], color: Color) {
  let channels = [color.red, color.green, color.blue].map(|channel| channel as f32);
  for (pixel, &covered) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(coverage) {
    let shadow = (color.alpha as f32 * covered).clamp(0.0, 1.0) * 255.0 * (1.0 - f32::from(pixel[3]) / 255.0);
    if shadow <= 0.0 {
      continue;
    }
    for (value, channel) in pixel.iter_mut().zip(channels) {
      *value = (f32::from(*value) + shadow * channel).round().min(255.0) as u8;
    }
    pixel[3] = (f32::from(pixel[3]) + shadow).round().min(255.0) as u8;
  }
}

/// Counts `<use>` start tags, including namespace-prefixed ones (`<s:use xmlns:s=...>`).
///
/// This scans tag names rather than parsing, so it also counts `<use` inside comments and
/// CDATA. Over-counting only rejects, which is the safe direction for an input limit.
fn count_use_elements(source: &str) -> usize {
  source
    .split('<')
    .skip(1)
    .filter(|tag| {
      let name = tag.split(|c: char| c.is_whitespace() || c == '/' || c == '>').next().unwrap_or("");
      name.rsplit(':').next() == Some("use")
    })
    .count()
}

#[cfg(test)]
#[path = "svg_test.rs"]
mod tests;
