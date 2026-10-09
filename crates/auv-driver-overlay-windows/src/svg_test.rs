use auv_driver_overlay_common::layers::BuiltInCursor;

use super::rasterize;

fn rejection(source: &str, size: u32) -> String {
  match rasterize(source, size) {
    Ok(_) => panic!("expected the SVG to be rejected"),
    Err(error) => error,
  }
}

fn pixel(bgra: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
  let index = ((y * size + x) * 4) as usize;
  [
    bgra[index],
    bgra[index + 1],
    bgra[index + 2],
    bgra[index + 3],
  ]
}

#[test]
fn solid_art_is_returned_as_premultiplied_bgra() {
  let source = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="#ff0000" fill-opacity="0.5"/></svg>"##;
  let sprite = rasterize(source, 10).unwrap();

  let [blue, green, red, alpha] = pixel(&sprite.bgra, sprite.size, 5, 5);
  assert!((127..=129).contains(&alpha), "half opacity, got {alpha}");
  assert_eq!(red, alpha, "premultiplied red equals alpha and sits in the third byte");
  assert_eq!((blue, green), (0, 0));
}

#[test]
fn art_scales_to_the_requested_sprite_size() {
  let source = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2 2"><rect x="1" width="1" height="2" fill="#0000ff"/></svg>"##;
  let sprite = rasterize(source, 48).unwrap();

  assert_eq!(sprite.bgra.len(), 48 * 48 * 4);
  assert_eq!(pixel(&sprite.bgra, 48, 12, 24)[3], 0, "left half stays empty");
  assert_eq!(pixel(&sprite.bgra, 48, 36, 24), [255, 0, 0, 255], "right half is opaque blue");
}

#[test]
fn builtin_cursor_art_rasterizes() {
  let source = BuiltInCursor::Auv.svg_source().unwrap();
  let sprite = rasterize(source, 24).unwrap();

  let covered = sprite.bgra.as_chunks::<4>().0.iter().filter(|pixel| pixel[3] > 0).count();
  assert!(covered > 24 * 24 / 4, "the arrow covers a good part of its box, got {covered}");
  let edges = sprite.bgra.as_chunks::<4>().0.iter().filter(|pixel| pixel[3] > 0 && pixel[3] < 255).count();
  assert!(edges > 0, "curved edges are antialiased");
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
fn external_image_references_are_not_loaded() {
  // A file this test process can certainly read: an SVG that would paint the sprite red.
  let directory = std::env::temp_dir().join(format!("auv-overlay-svg-{}", std::process::id()));
  std::fs::create_dir_all(&directory).unwrap();
  let local = directory.join("red.svg");
  std::fs::write(&local, r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect width="4" height="4" fill="red"/></svg>"#)
    .unwrap();
  let href = local.display().to_string().replace('\\', "/");
  let source = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><image href="{href}" width="4" height="4"/></svg>"#);

  let sprite = rasterize(&source, 4).unwrap();
  let _ = std::fs::remove_dir_all(&directory);
  assert!(sprite.bgra.iter().all(|&byte| byte == 0), "the local file must not be read into the cursor");
}
