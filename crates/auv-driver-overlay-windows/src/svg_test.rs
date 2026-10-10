use auv_driver_overlay_common::layers::{BuiltInCursor, CursorPose};
use auv_driver_overlay_common::style::{Color, Shadow};

use super::{BUILT_IN_TIP, Sprite, SpriteLayout, built_in_source, rasterize};

/// A layout whose hotspot is the top-left corner of a `size` square, at rest.
fn square(size: u32) -> SpriteLayout {
  SpriteLayout {
    size,
    hotspot: (0.0, 0.0),
    pose: CursorPose::REST,
  }
}

fn rejection(source: &str, size: u32) -> String {
  match rasterize(source, &square(size), None) {
    Ok(_) => panic!("expected the SVG to be rejected"),
    Err(error) => error,
  }
}

/// The pixel `(x, y)` pixels right of and below the hotspot.
fn pixel(sprite: &Sprite, x: i32, y: i32) -> [u8; 4] {
  let (column, row) = (x + sprite.hotspot.0, y + sprite.hotspot.1);
  assert!((0..sprite.width as i32).contains(&column) && (0..sprite.height as i32).contains(&row), "({x}, {y}) is outside the sprite");
  let index = (row as usize * sprite.width as usize + column as usize) * 4;
  [
    sprite.bgra[index],
    sprite.bgra[index + 1],
    sprite.bgra[index + 2],
    sprite.bgra[index + 3],
  ]
}

/// A 4 x 20 opaque bar hanging straight down from the hotspot, in a 20 x 20 box.
const BAR: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><rect width="4" height="20" fill="#0000ff"/></svg>"##;

/// A 10 x 10 opaque square filling its box.
const SQUARE: &str =
  r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="#ff0000"/></svg>"##;

fn shadow(blur_radius: f64, offset_y: f64) -> Shadow {
  Shadow {
    color: Color::rgba(0.0, 0.0, 0.0, 0.5),
    blur_radius,
    offset_x: 0.0,
    offset_y,
  }
}

#[test]
fn solid_art_is_returned_as_premultiplied_bgra() {
  let source = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="#ff0000" fill-opacity="0.5"/></svg>"##;
  let sprite = rasterize(source, &square(10), None).unwrap();

  let [blue, green, red, alpha] = pixel(&sprite, 5, 5);
  assert!((127..=129).contains(&alpha), "half opacity, got {alpha}");
  assert_eq!(red, alpha, "premultiplied red equals alpha and sits in the third byte");
  assert_eq!((blue, green), (0, 0));
}

#[test]
fn art_scales_to_the_requested_sprite_size() {
  let source = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2 2"><rect x="1" width="1" height="2" fill="#0000ff"/></svg>"##;
  let sprite = rasterize(source, &square(48), None).unwrap();

  // One pixel of antialiasing room on every side.
  assert_eq!((sprite.width, sprite.height, sprite.hotspot), (50, 50, (1, 1)));
  assert_eq!(pixel(&sprite, 12, 24)[3], 0, "left half stays empty");
  assert_eq!(pixel(&sprite, 36, 24), [255, 0, 0, 255], "right half is opaque blue");
}

#[test]
fn builtin_cursor_art_rasterizes() {
  let source = BuiltInCursor::Auv.svg_source().unwrap();
  let sprite = rasterize(source, &square(24), None).unwrap();

  let covered = sprite.bgra.as_chunks::<4>().0.iter().filter(|pixel| pixel[3] > 0).count();
  assert!(covered > 24 * 24 / 4, "the arrow covers a good part of its box, got {covered}");
}

#[test]
fn the_windows_pointer_rim_ends_on_its_hotspot() {
  let layout = SpriteLayout {
    size: 24,
    hotspot: (24.0 * BUILT_IN_TIP, 24.0 * BUILT_IN_TIP),
    pose: CursorPose::REST,
  };
  let sprite = rasterize(&built_in_source(BuiltInCursor::Auv, None).unwrap(), &layout, None).unwrap();

  assert!(pixel(&sprite, 0, 0)[3] >= 200, "the pixel at the hotspot is the rim's tip, got {:?}", pixel(&sprite, 0, 0));
  assert_eq!(pixel(&sprite, -2, 0)[3], 0, "nothing left of the tip");
  assert_eq!(pixel(&sprite, 0, -2)[3], 0, "nothing above the tip");
  let edges = sprite.bgra.as_chunks::<4>().0.iter().filter(|pixel| pixel[3] > 0 && pixel[3] < 255).count();
  assert!(edges > 0, "the rounded outline is antialiased");
}

// `shade` is fitted so that the live overlay's cyan cursor matches the hand-picked art.
#[test]
fn a_cyan_accent_shades_like_the_default_pointer() {
  let cyan = "#2fd3df".parse::<Color>().unwrap();
  let rest = built_in_source(BuiltInCursor::Auv, Some(cyan)).unwrap();
  assert!(rest.contains(r##"stop-color="#2fd3df""##) && rest.contains(r##"stop-color="#0794a6""##), "{rest}");
  let pressed = built_in_source(BuiltInCursor::AuvClick, Some(cyan)).unwrap();
  assert!(pressed.contains(r##"stop-color="#8de7ed""##) && pressed.contains(r##"stop-color="#23c0ce""##), "{pressed}");
}

#[test]
fn an_accent_tints_the_pointer_body() {
  let layout = SpriteLayout {
    size: 24,
    hotspot: (24.0 * BUILT_IN_TIP, 24.0 * BUILT_IN_TIP),
    pose: CursorPose::REST,
  };
  let pink = "#ff74b1".parse::<Color>().unwrap();
  let sprite = rasterize(&built_in_source(BuiltInCursor::Auv, Some(pink)).unwrap(), &layout, None).unwrap();

  let [blue, green, red, alpha] = pixel(&sprite, 4, 9);
  assert_eq!(alpha, 255, "inside the body");
  assert!(red > 200 && green < 120 && blue > 90, "a pink body, got rgb({red}, {green}, {blue})");
  // The rim stays white; the tip pixel only picks up a trace of the body's color.
  let [blue, green, red, _] = pixel(&sprite, 0, 0);
  assert!(red >= 200 && green >= 200 && blue >= 200, "the rim at the tip stays white, got rgb({red}, {green}, {blue})");
}

#[test]
fn an_accent_must_be_a_real_color() {
  let error = built_in_source(BuiltInCursor::Auv, Some(Color::rgb(f64::NAN, 0.0, 0.0))).unwrap_err();
  assert!(error.contains("accent"), "{error}");
  assert!(built_in_source(BuiltInCursor::Auv, Some(Color::rgb(1.5, 0.0, 0.0))).is_err());
}

#[test]
fn a_tilt_turns_the_art_clockwise_about_the_hotspot() {
  let layout = SpriteLayout {
    size: 20,
    hotspot: (2.0, 0.0),
    pose: CursorPose {
      tilt_degrees: 90.0,
      scale: 1.0,
    },
  };
  let sprite = rasterize(BAR, &layout, None).unwrap();

  // Turned a quarter clockwise, the bar that hung down from the hotspot points left.
  assert_eq!(pixel(&sprite, -15, 0), [255, 0, 0, 255], "the bar now runs left of the hotspot");
  assert_eq!(pixel(&sprite, 0, 15)[3], 0, "nothing hangs below the hotspot any more");
}

#[test]
fn a_scale_shrinks_the_art_toward_the_hotspot() {
  let layout = SpriteLayout {
    size: 20,
    hotspot: (2.0, 0.0),
    pose: CursorPose {
      tilt_degrees: 0.0,
      scale: 0.5,
    },
  };
  let sprite = rasterize(BAR, &layout, None).unwrap();

  assert_eq!(pixel(&sprite, 0, 5)[3], 255, "the bar still starts at the hotspot");
  assert_eq!(pixel(&sprite, 0, 10)[3], 0, "and ends halfway down");
  assert!(sprite.height <= 13, "the bitmap shrinks with the art, got {} rows", sprite.height);
}

#[test]
fn the_shadow_is_the_art_silhouette_offset_under_the_art() {
  let sprite = rasterize(SQUARE, &square(10), Some(&shadow(0.0, 4.0))).unwrap();

  assert_eq!(pixel(&sprite, 5, 5), [0, 0, 255, 255], "opaque art hides the shadow beneath it");
  // Below the art, the unblurred shadow is the square's shape at the shadow's opacity.
  let [blue, green, red, alpha] = pixel(&sprite, 5, 12);
  assert_eq!((blue, green, red), (0, 0, 0));
  assert!((126..=129).contains(&alpha), "half-opaque black shadow, got {alpha}");
  assert_eq!(pixel(&sprite, 5, 14)[3], 0, "the shadow is exactly the 4px lowered square");
  assert_eq!(pixel(&sprite, 12, 12)[3], 0, "and not wider than the art");
}

#[test]
fn a_blurred_shadow_fades_with_distance_and_stays_inside_the_sprite() {
  let sprite = rasterize(SQUARE, &square(10), Some(&shadow(4.0, 0.0))).unwrap();

  let ray = (10..sprite.width as i32 - sprite.hotspot.0).map(|x| pixel(&sprite, x, 5)[3]).collect::<Vec<_>>();
  assert!(ray[0] > 0 && ray[0] < 128, "half-covered next to the art edge: {ray:?}");
  assert!(ray.windows(2).all(|pair| pair[1] <= pair[0]), "fades away from the art: {ray:?}");
  assert_eq!(ray.last().copied(), Some(0), "fully faded before the bitmap edge: {ray:?}");
}

#[test]
fn a_transparent_shadow_adds_nothing() {
  let plain = rasterize(SQUARE, &square(10), None).unwrap();
  let clear = Shadow {
    color: Color::CLEAR,
    ..shadow(4.0, 4.0)
  };
  let shadowed = rasterize(SQUARE, &square(10), Some(&clear)).unwrap();
  let drawn = |sprite: &Sprite| sprite.bgra.as_chunks::<4>().0.iter().filter(|pixel| pixel[3] > 0).count();
  assert_eq!(drawn(&shadowed), drawn(&plain));
}

#[test]
fn malformed_svg_is_rejected() {
  let error = rejection("<svg", 24);
  assert!(error.contains("invalid cursor SVG"), "{error}");
}

#[test]
fn oversized_inputs_are_rejected() {
  let huge = format!("<svg xmlns=\"http://www.w3.org/2000/svg\">{}</svg>", " ".repeat(256 * 1024));
  assert!(rejection(&huge, 24).contains("256 KiB"));
  let small = r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>"#;
  assert!(rejection(small, 0).contains("sprite size"));
  assert!(rejection(small, 4096).contains("sprite size"));
}

#[test]
fn poses_that_cannot_be_drawn_are_rejected() {
  let posed = |tilt_degrees, scale| SpriteLayout {
    pose: CursorPose {
      tilt_degrees,
      scale,
    },
    ..square(24)
  };
  for (tilt, scale) in [
    (f64::NAN, 1.0),
    (0.0, 0.0),
    (0.0, -1.0),
    (0.0, f64::INFINITY),
  ] {
    let error = rasterize(SQUARE, &posed(tilt, scale), None).err().expect("rejected");
    assert!(error.contains("cursor pose"), "{error}");
  }
  let error = rasterize(SQUARE, &posed(0.0, 1.0e6), None).err().expect("rejected");
  assert!(error.contains("2048"), "a pose may not blow up the bitmap: {error}");
}

#[test]
fn external_image_references_are_not_loaded() {
  // A file this test process can certainly read: an SVG that would paint the sprite red.
  let directory = std::env::temp_dir().join(format!("auv-overlay-svg-{}", std::process::id()));
  std::fs::create_dir_all(&directory).unwrap();
  let local = directory.join("red.svg");
  std::fs::write(&local, r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect width="4" height="4" fill="red"/></svg>"#)
    .unwrap();
  let href = local.display().to_string().replace('\\', "/");
  let source = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><image href="{href}" width="4" height="4"/></svg>"#);

  let sprite = rasterize(&source, &square(4), None).unwrap();
  let _ = std::fs::remove_dir_all(&directory);
  assert!(sprite.bgra.iter().all(|&byte| byte == 0), "the local file must not be read into the cursor");
}

/// Group `g{depth}` uses `g{depth-1}` twice, so the document expands to 2^depth rects.
fn use_bomb(depth: usize, prefix: &str) -> String {
  let mut defs = String::from(r##"<g id="g0"><rect width="24" height="24" fill="#f00"/></g>"##);
  for level in 1..=depth {
    defs.push_str(&format!(r##"<g id="g{level}"><{prefix}use href="#g{}"/><{prefix}use href="#g{}"/></g>"##, level - 1, level - 1));
  }
  let namespace = if prefix.is_empty() {
    String::new()
  } else {
    r#" xmlns:s="http://www.w3.org/2000/svg""#.to_string()
  };
  format!(
    r##"<svg xmlns="http://www.w3.org/2000/svg"{namespace} viewBox="0 0 24 24"><defs>{defs}</defs><{prefix}use href="#g{depth}"/></svg>"##
  )
}

// ROOT CAUSE:
//
// If a cursor SVG nested `<use>` references, usvg expanded them multiplicatively at parse
// time, so a ~1 KB document could keep `present()` busy for many seconds even though it was
// far below the 256 KiB size limit.
//
// Before the fix, a 16-level doubling document took about 5 s to rasterize.
// The fix rejects documents with more than 16 `<use>` elements before parsing, which bounds
// the expansion whatever the nesting.
#[test]
fn nested_use_expansion_is_rejected_before_it_can_stall_the_renderer() {
  for prefix in ["", "s:"] {
    let started = std::time::Instant::now();
    let error = rejection(&use_bomb(16, prefix), 24);
    assert!(error.contains("<use>"), "{error}");
    assert!(started.elapsed() < std::time::Duration::from_millis(500), "rejected after {:?}", started.elapsed());
  }
}

#[test]
fn a_few_use_elements_still_render() {
  let sprite = rasterize(&use_bomb(3, ""), &square(24), None).unwrap();
  assert_eq!(pixel(&sprite, 12, 12), [0, 0, 255, 255], "the reused red rect paints");
}
