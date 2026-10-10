//! Renders a fixed overlay scene on the real desktop and saves a screen capture of it.
//!
//! Produces the before/after evidence for Windows renderer changes:
//!
//! ```text
//! cargo run -p auv-driver-overlay-windows --example overlay_gallery -- <out.bmp>
//! ```
//!
//! The scene is drawn over a topmost backdrop window this example owns (a light and a
//! dark checkerboard, so antialiasing and translucency are visible), and only the
//! backdrop's rectangle is captured. The overlay window is created after the backdrop,
//! so it stacks above it, and no other window's content appears in the image.
//!
//! Layers the renderer rejects are reported and left out instead of aborting, so the
//! same scene can be run against an older renderer.

#[cfg(target_os = "windows")]
#[allow(dead_code)]
#[path = "support/backdrop.rs"]
mod backdrop;

#[cfg(target_os = "windows")]
fn main() {
  let output = std::env::args().nth(1).unwrap_or_else(|| "overlay-gallery.bmp".to_string());
  if let Err(error) = gallery::run(std::path::Path::new(&output)) {
    eprintln!("overlay_gallery failed: {error}");
    std::process::exit(1);
  }
}

#[cfg(not(target_os = "windows"))]
fn main() {
  eprintln!("overlay_gallery only runs on Windows");
}

#[cfg(target_os = "windows")]
mod gallery {
  use std::path::Path;
  use std::time::Duration;

  use auv_driver_common::{Rect, ScreenPoint};
  use auv_driver_overlay_common::layers::{BuiltInCursor, Cursor, CursorImage, Outline, Status};
  use auv_driver_overlay_common::style::{CursorStyle, Shadow};
  use auv_driver_overlay_common::{Layer, LifecycleOptions, Overlay, ShowOptions};

  use super::backdrop::{Backdrop, pump, write_bmp};

  const LEFT: i32 = 200;
  const TOP: i32 = 200;
  const WIDTH: i32 = 720;
  const HEIGHT: i32 = 300;

  /// Screen point at `(x, y)` inside the backdrop.
  fn at(x: f64, y: f64) -> ScreenPoint {
    ScreenPoint::new(f64::from(LEFT) + x, f64::from(TOP) + y)
  }

  fn glowing() -> CursorStyle {
    CursorStyle::default().with_shadow(Some(Shadow::auv()))
  }

  /// The scene: every feature the Direct2D renderer covers, light half then dark half.
  fn scene() -> Vec<(&'static str, Layer)> {
    let auv_art = BuiltInCursor::Auv.svg_source().expect("built-in art");
    let click_art = BuiltInCursor::AuvClick.svg_source().expect("built-in art");
    vec![
      ("built-in cursor", Layer::Cursor(Cursor::new(at(50.0, 50.0)).with_label("auv").with_label_visible())),
      (
        "built-in click cursor",
        Layer::Cursor(
          Cursor::new(at(50.0, 140.0))
            .with_image(CursorImage::built_in(BuiltInCursor::AuvClick))
            .with_style(CursorStyle::auv_click())
            .with_label("click")
            .with_label_visible(),
        ),
      ),
      (
        "outline",
        Layer::Outline(
          Outline::new(Rect::new(f64::from(LEFT) + 190.0, f64::from(TOP) + 40.0, 140.0, 110.0)).with_label("outline").with_label_visible(),
        ),
      ),
      ("translucent status", Layer::Status(Status::new(at(30.0, 240.0), "status 0.88 alpha"))),
      (
        "svg cursor + glow",
        Layer::Cursor(
          Cursor::new(at(400.0, 30.0))
            .with_image(CursorImage::svg(auv_art))
            .with_style(glowing())
            .with_label("svg + glow")
            .with_label_visible(),
        ),
      ),
      (
        "svg click cursor + CJK label",
        Layer::Cursor(
          Cursor::new(at(400.0, 110.0))
            .with_image(CursorImage::svg(click_art))
            .with_style(glowing())
            .with_label("播放 · QQ音乐")
            .with_label_visible(),
        ),
      ),
      ("built-in cursor, explicit glow", Layer::Cursor(Cursor::new(at(620.0, 60.0)).with_style(glowing()))),
      (
        "svg cursor without shadow",
        Layer::Cursor(Cursor::new(at(600.0, 130.0)).with_image(CursorImage::svg(auv_art)).with_label("svg").with_label_visible()),
      ),
      ("translucent status on dark", Layer::Status(Status::new(at(400.0, 240.0), "状态 status 半透明"))),
    ]
  }

  pub(super) fn run(output: &Path) -> Result<(), String> {
    let backdrop = Backdrop::show(LEFT, TOP, WIDTH, HEIGHT)?;
    let manual = ShowOptions::new().with_lifecycle_options(LifecycleOptions::manual());

    // Probe each layer on its own so a renderer that rejects some still shows the rest.
    let mut overlay = Overlay::new();
    for (name, layer) in scene() {
      match auv_driver_overlay_windows::render(&Overlay::new().with_layer(layer.clone()), manual) {
        Ok(()) => {
          println!("rendered: {name}");
          overlay = overlay.with_layer(layer);
        }
        Err(error) => println!("REJECTED: {name}: {error}"),
      }
    }
    auv_driver_overlay_windows::render(&overlay, manual)?;
    pump(Duration::from_millis(400));

    let captured = backdrop.capture();
    let _ = auv_driver_overlay_windows::remove();
    backdrop.close();
    write_bmp(output, WIDTH, HEIGHT, &captured?)?;
    println!("saved {}", output.display());
    Ok(())
  }
}
