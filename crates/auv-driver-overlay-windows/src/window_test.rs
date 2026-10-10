#![cfg(target_os = "windows")]

use auv_driver_common::{Rect, ScreenPoint};
use auv_driver_overlay_common::Layer;
use auv_driver_overlay_common::layers::{BuiltInCursor, Cursor, CursorImage, Outline, Status};
use auv_driver_overlay_common::style::{CursorStyle, OutlineStyle, Shadow, Stroke};
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
// Before the fix, the cursor disc had a hard, aliased edge with no partial alpha.
// The fix draws with Direct2D, which writes premultiplied alpha, so edge pixels blend.
#[test]
fn cursor_disc_edges_carry_partial_alpha() {
  let pixels = render(&[Layer::Cursor(Cursor::new(ScreenPoint::new(40.0, 40.0)))]).unwrap();

  assert_eq!(alpha(&pixels, 40, 40), 255, "disc interior is opaque");
  assert_eq!(alpha(&pixels, 0, 0), 0, "untouched pixels stay transparent");
  let edge = (0..WIDTH).map(|x| alpha(&pixels, x, 40)).filter(|&a| 0 < a && a < 255).count();
  assert!(edge >= 2, "both disc edges must have 0 < alpha < 255");
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

#[test]
fn explicit_cursor_shadow_renders_a_glow() {
  let style = CursorStyle::default().with_shadow(Some(Shadow::auv()));
  let cursor = Cursor::new(ScreenPoint::new(40.0, 40.0)).with_style(style);
  let pixels = render(&[Layer::Cursor(cursor)]).unwrap();

  // The disc has radius 12; the glow (offset 2 down) shows beyond it.
  let below = alpha(&pixels, 40, 40 + 16);
  assert!(below > 0 && below < 255, "soft glow below the disc, got {below}");
  assert_eq!(alpha(&pixels, 40, 40), 255, "the sprite is drawn over its glow");
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
  let cursor = Cursor::new(ScreenPoint::new(30.0, 40.0)).with_label("auv").with_label_visible();
  let pixels = render(&[Layer::Cursor(cursor)]).unwrap();

  // Disc radius 12 plus the 6px label gap puts the pill's left edge at x = 48.
  assert_eq!(alpha(&pixels, 46, 40), 0, "gap between sprite and pill");
  assert!(alpha(&pixels, 52, 40) > 0, "pill starts after the gap");
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
