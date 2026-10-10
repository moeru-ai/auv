#![cfg(target_os = "windows")]

use auv_driver_common::{Rect, ScreenPoint};
use auv_driver_overlay_common::Layer;
use auv_driver_overlay_common::layers::{BuiltInCursor, Cursor, CursorImage, CursorPose, Outline, Status};
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
// The fix draws with Direct2D, which writes premultiplied alpha, so the shadow fades out.
#[test]
fn cursor_shadow_edges_carry_partial_alpha() {
  let pixels = render(&[Layer::Cursor(Cursor::new(ScreenPoint::new(40.0, 40.0)))]).unwrap();

  assert_eq!(alpha(&pixels, 0, 0), 0, "untouched pixels stay transparent");
  // Just below the pointer's lower tip, which ends at y = 62: only its shadow reaches here.
  let soft = (0..WIDTH).map(|x| alpha(&pixels, x, 64)).filter(|&a| 0 < a && a < 255).count();
  assert!(soft >= 4, "the shadow must fade through partial alpha, got {soft} soft pixels on one row");
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

fn no_shadow() -> CursorStyle {
  CursorStyle::default().with_shadow(Some(Shadow {
    color: Color::CLEAR,
    ..Shadow::auv()
  }))
}

/// A built-in pointer at (40, 40) without its shadow, so only the art is drawn.
fn plain_pointer() -> Cursor {
  Cursor::new(ScreenPoint::new(40.0, 40.0)).with_style(no_shadow())
}

/// Alpha-weighted center of everything drawn.
fn centroid(pixels: &[u8]) -> (f64, f64) {
  let (mut x_sum, mut y_sum, mut total) = (0.0, 0.0, 0.0);
  for y in 0..HEIGHT {
    for x in 0..WIDTH {
      let weight = f64::from(alpha(pixels, x, y));
      x_sum += x as f64 * weight;
      y_sum += y as f64 * weight;
      total += weight;
    }
  }
  (x_sum / total, y_sum / total)
}

#[test]
fn builtin_cursor_tip_sits_exactly_on_the_target_point() {
  let pixels = render(&[Layer::Cursor(plain_pointer())]).unwrap();

  // The pointer's hotspot is the outermost point of its rounded white tip: the target
  // pixel is almost fully covered by the rim, and nothing reaches two pixels further out.
  let [blue, green, red, tip] = pixel(&pixels, 40, 40);
  assert!(tip >= 200 && red >= 180 && green >= 180 && blue >= 180, "white rim on the tip pixel, got {:?}", [blue, green, red, tip]);
  for (x, y) in [(38, 40), (40, 38), (38, 38)] {
    assert_eq!(alpha(&pixels, x, y), 0, "nothing beyond the tip at ({x}, {y})");
  }
  // The rim runs down the left edge, white over transparency.
  let [blue, green, red, rim] = pixel(&pixels, 39, 50);
  assert!(rim > 120 && blue == red && green == red, "white rim left of the body, got {:?}", [blue, green, red, rim]);
  // Inside the rim the body is opaque AUV cyan.
  let [blue, green, red, body] = pixel(&pixels, 44, 49);
  assert!(body == 255 && blue > 150 && green > 150 && red < 80, "cyan body, got {:?}", [blue, green, red, body]);
}

#[test]
fn builtin_cursor_turns_and_shrinks_about_its_tip() {
  let rest = render(&[Layer::Cursor(plain_pointer())]).unwrap();
  let tilted = render(&[Layer::Cursor(plain_pointer().with_pose(CursorPose {
    tilt_degrees: 14.0,
    scale: 1.0,
  }))])
  .unwrap();
  let pressed = render(&[Layer::Cursor(plain_pointer().with_pose(CursorPose {
    tilt_degrees: 0.0,
    scale: 0.84,
  }))])
  .unwrap();

  for (name, pixels) in [("tilted", &tilted), ("pressed", &pressed)] {
    assert!(alpha(pixels, 40, 40) >= 200, "the {name} tip stays on the target, got {}", alpha(pixels, 40, 40));
  }
  let (rest_x, rest_y) = centroid(&rest);
  let (tilted_x, _) = centroid(&tilted);
  assert!(tilted_x < rest_x - 1.0, "a clockwise tilt swings the body left: {rest_x:.1} -> {tilted_x:.1}");
  let (pressed_x, pressed_y) = centroid(&pressed);
  assert!(pressed_x < rest_x && pressed_y < rest_y, "a press shrinks the body toward the tip");
}

#[test]
fn invalid_cursor_pose_is_rejected() {
  let cursor = plain_pointer().with_pose(CursorPose {
    tilt_degrees: f64::NAN,
    scale: 1.0,
  });
  let error = render(&[Layer::Cursor(cursor)]).unwrap_err();
  assert!(error.contains("cursor pose"), "{error}");
}

#[test]
fn builtin_cursor_casts_a_shadow_unless_the_style_turns_it_off() {
  let shadowed = render(&[Layer::Cursor(Cursor::new(ScreenPoint::new(40.0, 40.0)))]).unwrap();
  let plain = render(&[Layer::Cursor(plain_pointer())]).unwrap();

  // Below the pointer's lower tip (y = 62), where only its lowered shadow reaches.
  let shade = pixel(&shadowed, 41, 64);
  assert!(shade[3] > 0 && shade[3] < 255, "default soft shadow, got {shade:?}");
  assert!(shade[0] == 0 && shade[1] == 0 && shade[2] == 0, "the default shadow is dark, got {shade:?}");
  assert_eq!(alpha(&plain, 41, 64), 0, "a transparent shadow turns the default off");
}

#[test]
fn builtin_variants_draw_distinct_art() {
  let body_of = |variant| {
    let cursor = plain_pointer().with_image(CursorImage::built_in(variant));
    pixel(&render(&[Layer::Cursor(cursor)]).unwrap(), 44, 49)
  };
  let [auv_blue, _, auv_red, _] = body_of(BuiltInCursor::Auv);
  let [click_blue, _, click_red, _] = body_of(BuiltInCursor::AuvClick);
  assert!(click_red > auv_red + 30 && click_blue >= auv_blue, "the pressed pointer is lighter");
  let [blue, green, red, opaque] = body_of(BuiltInCursor::You);
  assert!(opaque == 255 && blue > red && blue < 140 && green < 140, "the user cursor is slate, got {:?}", [blue, green, red, opaque]);
}

#[test]
fn builtin_cursor_is_drawn_in_its_style_accent() {
  let pink = "#ff74b1".parse::<Color>().unwrap();
  let cursor = plain_pointer().with_style(no_shadow().with_accent(Some(pink)));
  let [blue, green, red, body] = pixel(&render(&[Layer::Cursor(cursor)]).unwrap(), 44, 49);
  assert!(body == 255 && red > 200 && green < 120 && blue > 90, "a pink body, got {:?}", [blue, green, red, body]);
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
  let cursor = Cursor::new(ScreenPoint::new(30.0, 40.0)).with_style(no_shadow()).with_label("auv").with_label_visible();
  let pixels = render(&[Layer::Cursor(cursor)]).unwrap();

  // The hotspot is 1px inside the 24px sprite box, so the box spans x = 29..53; the 6px
  // label gap puts the pill's left edge at 59.
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
fn custom_svg_art_casts_its_style_shadow_under_its_silhouette() {
  // The macOS brand pointer, used as custom art with the macOS glow.
  let source = BuiltInCursor::Auv.svg_source().unwrap();
  let style = CursorStyle::default().with_shadow(Some(Shadow::auv()));
  let cursor = Cursor::new(ScreenPoint::new(40.0, 40.0)).with_image(CursorImage::svg(source)).with_style(style);
  let pixels = render(&[Layer::Cursor(cursor)]).unwrap();

  let in_box = (44..68).flat_map(|y| (44..68).map(move |x| (x, y)));
  assert!(in_box.clone().any(|(x, y)| alpha(&pixels, x, y) == 255), "the arrow art is opaque");
  // The art's column at x = 48 ends at y = 62; its glow is lowered 2px and blurred.
  let halo = alpha(&pixels, 48, 66);
  assert!(halo > 0 && halo < 255, "soft glow just below the art, got {halo}");
  // Right of the art's lower row (which ends at x = 60) the glow fades with distance.
  assert!(alpha(&pixels, 62, 58) > alpha(&pixels, 70, 58), "the glow follows the silhouette and fades");
  assert_eq!(alpha(&pixels, 48, 95), 0, "the glow ends within three blur sigmas");
}

#[test]
fn malformed_svg_cursor_fails_instead_of_drawing_nothing() {
  let cursor = Cursor::new(ScreenPoint::new(40.0, 40.0)).with_image(CursorImage::svg("<svg"));
  let error = render(&[Layer::Cursor(cursor)]).unwrap_err();
  assert!(error.contains("invalid cursor SVG"), "{error}");
}
