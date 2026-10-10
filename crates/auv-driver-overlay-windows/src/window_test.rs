#![cfg(target_os = "windows")]

use auv_driver_common::{Rect, ScreenPoint};
use auv_driver_overlay_common::Layer;
use auv_driver_overlay_common::layers::{BuiltInCursor, Cursor, CursorImage, Outline, Status};
use auv_driver_overlay_common::style::{Color, CursorStyle, OutlineStyle, Shadow, Stroke};
use windows::Win32::Foundation::RECT;

use super::native::draw_layers;
use crate::canvas::Canvas;

const WIDTH: usize = 160;
const HEIGHT: usize = 100;

/// Renders `layers` through the production layer mapping into a small offscreen canvas
/// at the screen origin and returns its premultiplied BGRA pixels.
fn render(layers: &[Layer]) -> crate::AuvResult<Vec<u8>> {
  let canvas = Canvas::new(RECT {
    left: 0,
    top: 0,
    right: WIDTH as i32,
    bottom: HEIGHT as i32,
  })?;
  draw_layers(&canvas, layers)?;
  canvas.into_pixels()
}

fn pixel(pixels: &[u8], x: usize, y: usize) -> [u8; 4] {
  let index = (y * WIDTH + x) * 4;
  [
    pixels[index],
    pixels[index + 1],
    pixels[index + 2],
    pixels[index + 3],
  ]
}

fn alpha(pixels: &[u8], x: usize, y: usize) -> u8 {
  pixel(pixels, x, y)[3]
}

// ROOT CAUSE:
//
// If a GDI primitive was drawn, every covered pixel came out fully opaque and every
// other pixel fully transparent, because GDI ignores the alpha channel and the old
// renderer derived alpha by keying a sentinel color.
//
// Before the fix, the cursor's glow had a hard, aliased edge with no partial alpha.
// The fix draws with Direct2D, which writes premultiplied alpha, so the glow fades out.
#[test]
fn cursor_glow_edges_carry_partial_alpha() {
  let pixels = render(&[Layer::Cursor(Cursor::new(ScreenPoint::new(40.0, 40.0)))]).unwrap();

  assert_eq!(alpha(&pixels, 0, 0), 0, "untouched pixels stay transparent");
  let soft = (0..WIDTH).map(|x| alpha(&pixels, x, 62)).filter(|&a| 0 < a && a < 255).count();
  assert!(soft >= 4, "the glow must fade through partial alpha, got {soft} soft pixels on one row");
}

#[test]
fn status_background_opacity_reaches_the_alpha_channel() {
  // Default status background is AUV cyan at 0.88 alpha.
  let pixels = render(&[Layer::Status(Status::new(
    ScreenPoint::new(10.0, 50.0),
    "working",
  ))])
  .unwrap();

  let column = 30;
  let top = (0..HEIGHT).find(|&y| alpha(&pixels, column, y) > 0).expect("status pill");
  let [blue, green, red, a] = pixel(&pixels, column, top + 3);
  assert!((222..=226).contains(&a), "0.88 opacity is about 224, got {a}");
  assert!(blue <= a && green <= a, "premultiplied channels never exceed alpha");
  assert_eq!(red, 0);
}

/// Edge and fill of the default AUV cursor as premultiplied BGRA (`#0b3a4a`, `#49e3e4`).
const AUV_EDGE: [u8; 4] = [0x4a, 0x3a, 0x0b, 255];
const AUV_FILL: [u8; 4] = [0xe4, 0xe3, 0x49, 255];

fn no_glow() -> CursorStyle {
  CursorStyle::default().with_shadow(Some(Shadow {
    color: Color::CLEAR,
    ..Shadow::auv()
  }))
}

#[test]
fn builtin_cursor_tip_sits_exactly_on_the_target_point() {
  let pixels = render(&[Layer::Cursor(
    Cursor::new(ScreenPoint::new(40.0, 40.0)).with_style(no_glow()),
  )])
  .unwrap();

  // The art is a 12x12 grid of 2px cells; the tip is the top-left cell.
  assert_eq!([pixel(&pixels, 40, 40), pixel(&pixels, 41, 41)], [AUV_EDGE, AUV_EDGE], "tip cell");
  assert_eq!(alpha(&pixels, 39, 40), 0, "nothing left of the tip");
  assert_eq!(alpha(&pixels, 40, 39), 0, "nothing above the tip");
  assert_eq!(pixel(&pixels, 42, 44), AUV_FILL, "second cell of the third row is fill");
  assert_eq!(pixel(&pixels, 40, 52), AUV_EDGE, "the left edge runs down the arrow");
}

#[test]
fn builtin_cursor_art_edges_are_pixel_crisp() {
  let pixels = render(&[Layer::Cursor(
    Cursor::new(ScreenPoint::new(40.0, 40.0)).with_style(no_glow()),
  )])
  .unwrap();

  let soft =
    (40..64).flat_map(|y| (40..64).map(move |x| (x, y))).filter(|&(x, y)| 0 < alpha(&pixels, x, y) && alpha(&pixels, x, y) < 255).count();
  assert_eq!(soft, 0, "pixel art must not antialias across its cells");
}

#[test]
fn builtin_cursor_glows_unless_the_style_turns_it_off() {
  let glowing = render(&[Layer::Cursor(Cursor::new(ScreenPoint::new(40.0, 40.0)))]).unwrap();
  let plain = render(&[Layer::Cursor(
    Cursor::new(ScreenPoint::new(40.0, 40.0)).with_style(no_glow()),
  )])
  .unwrap();

  // Right of the arrow, outside its cells but inside the glow's reach.
  let halo = alpha(&glowing, 60, 62);
  assert!(halo > 0 && halo < 255, "default built-in glow, got {halo}");
  assert_eq!(alpha(&plain, 60, 62), 0, "a transparent shadow disables the default glow");
}

#[test]
fn builtin_variants_draw_distinct_art() {
  let fill_of = |variant| {
    let cursor = Cursor::new(ScreenPoint::new(40.0, 40.0)).with_image(CursorImage::built_in(variant)).with_style(no_glow());
    pixel(&render(&[Layer::Cursor(cursor)]).unwrap(), 42, 44)
  };
  assert_eq!(fill_of(BuiltInCursor::Auv), AUV_FILL);
  assert_ne!(fill_of(BuiltInCursor::AuvClick), AUV_FILL, "the pressed pointer is lighter");
  assert_eq!(fill_of(BuiltInCursor::You), [255, 255, 255, 255], "the user cursor is white");
}

#[test]
fn invalid_cursor_shadow_is_rejected() {
  let shadow = Shadow {
    blur_radius: f64::NAN,
    ..Shadow::auv()
  };
  let cursor = Cursor::new(ScreenPoint::new(40.0, 40.0)).with_style(CursorStyle::default().with_shadow(Some(shadow)));
  let error = render(&[Layer::Cursor(cursor)]).unwrap_err();
  assert!(error.contains("blur radius"), "{error}");
}

#[test]
fn outline_straight_edges_stay_crisp_and_corners_antialias() {
  let style = OutlineStyle::default().with_stroke(Stroke::new(auv_driver_overlay_common::style::Color::AUV_CYAN, 3.0));
  let outline = Outline::new(Rect::new(20.0, 20.0, 80.0, 50.0)).with_style(style);
  let pixels = render(&[Layer::Outline(outline)]).unwrap();

  // A 3px stroke centered on x = 20 covers exactly pixels 19, 20 and 21.
  let row = 45;
  assert_eq!([18, 19, 20, 21, 22].map(|x| alpha(&pixels, x, row)), [0, 255, 255, 255, 0]);
  let corner = (15..30).flat_map(|y| (15..30).map(move |x| (x, y))).filter(|&(x, y)| {
    let a = alpha(&pixels, x, y);
    0 < a && a < 255
  });
  assert!(corner.count() > 0, "the rounded corner blends with partial alpha");
}

#[test]
fn cursor_label_pill_sits_right_of_the_sprite() {
  let cursor = Cursor::new(ScreenPoint::new(30.0, 40.0)).with_style(no_glow()).with_label("auv").with_label_visible();
  let pixels = render(&[Layer::Cursor(cursor)]).unwrap();

  // The 24px sprite box ends at x = 54; the 6px label gap puts the pill's left edge at 60.
  assert_eq!(alpha(&pixels, 57, 52), 0, "gap between sprite and pill");
  assert!(alpha(&pixels, 64, 52) > 0, "pill starts after the gap");
}

/// A 24x24 opaque red square, so placement can be checked pixel by pixel.
const RED_SQUARE: &str =
  r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24"><rect width="24" height="24" fill="#ff0000"/></svg>"##;

#[test]
fn svg_cursor_art_renders_beside_the_target_point() {
  let cursor = Cursor::new(ScreenPoint::new(40.0, 40.0)).with_image(CursorImage::svg(RED_SQUARE));
  let pixels = render(&[Layer::Cursor(cursor)]).unwrap();

  // Like macOS, the 24px sprite box starts 4px right of and below the target.
  assert_eq!(alpha(&pixels, 40, 40), 0, "the target point itself stays uncovered");
  assert_eq!(pixel(&pixels, 44, 44), [0, 0, 255, 255], "box top-left");
  assert_eq!(pixel(&pixels, 67, 67), [0, 0, 255, 255], "box bottom-right");
  assert_eq!(alpha(&pixels, 68, 56), 0, "the box is exactly 24px wide");
}

#[test]
fn svg_cursor_label_attaches_to_the_sprite_box() {
  let cursor = Cursor::new(ScreenPoint::new(40.0, 40.0)).with_image(CursorImage::svg(RED_SQUARE)).with_label("auv").with_label_visible();
  let pixels = render(&[Layer::Cursor(cursor)]).unwrap();

  // Box right edge 68 plus the 6px gap: the pill starts at x = 74, centered on y = 56.
  assert_eq!(alpha(&pixels, 71, 56), 0, "gap between sprite and pill");
  assert!(alpha(&pixels, 77, 56) > 0, "pill after the gap");
}

#[test]
fn builtin_cursor_art_renders_with_a_glow_behind_it() {
  let source = BuiltInCursor::Auv.svg_source().unwrap();
  let style = CursorStyle::default().with_shadow(Some(Shadow::auv()));
  let cursor = Cursor::new(ScreenPoint::new(40.0, 40.0)).with_image(CursorImage::svg(source)).with_style(style);
  let pixels = render(&[Layer::Cursor(cursor)]).unwrap();

  let in_box = (44..68).flat_map(|y| (44..68).map(move |x| (x, y)));
  assert!(in_box.clone().any(|(x, y)| alpha(&pixels, x, y) == 255), "the arrow art is opaque");
  // Outside the 24px box but inside the glow's reach.
  let halo = alpha(&pixels, 56, 74);
  assert!(halo > 0 && halo < 255, "soft glow beyond the sprite box, got {halo}");
}

#[test]
fn malformed_svg_cursor_fails_instead_of_drawing_nothing() {
  let cursor = Cursor::new(ScreenPoint::new(40.0, 40.0)).with_image(CursorImage::svg("<svg"));
  let error = render(&[Layer::Cursor(cursor)]).unwrap_err();
  assert!(error.contains("invalid cursor SVG"), "{error}");
}
