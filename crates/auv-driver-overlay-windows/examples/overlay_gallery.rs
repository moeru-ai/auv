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
  use std::time::{Duration, Instant};

  use auv_driver_common::{Rect, ScreenPoint};
  use auv_driver_overlay_common::layers::{BuiltInCursor, Cursor, CursorImage, Outline, Status};
  use auv_driver_overlay_common::style::{CursorStyle, Shadow};
  use auv_driver_overlay_common::{Layer, LifecycleOptions, Overlay, ShowOptions};
  use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
  use windows::Win32::Graphics::Gdi::{
    BITMAPINFO, BITMAPINFOHEADER, BeginPaint, BitBlt, CAPTUREBLT, CreateCompatibleBitmap, CreateCompatibleDC, CreateSolidBrush,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, EndPaint, FillRect, GetDC, GetDIBits, PAINTSTRUCT, ReleaseDC, SRCCOPY, SelectObject,
    UpdateWindow,
  };
  use windows::Win32::System::LibraryLoader::GetModuleHandleW;
  use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, RegisterClassExW, SW_SHOWNOACTIVATE,
    ShowWindow, TranslateMessage, WM_PAINT, WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
  };
  use windows::core::w;

  const LEFT: i32 = 200;
  const TOP: i32 = 200;
  const WIDTH: i32 = 720;
  const HEIGHT: i32 = 300;
  const CELL: i32 = 12;

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
      ("disc cursor", Layer::Cursor(Cursor::new(at(50.0, 50.0)).with_label("auv").with_label_visible())),
      (
        "click disc cursor",
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
      ("disc cursor + glow", Layer::Cursor(Cursor::new(at(620.0, 60.0)).with_style(glowing()))),
      (
        "svg cursor without shadow",
        Layer::Cursor(Cursor::new(at(600.0, 130.0)).with_image(CursorImage::svg(auv_art)).with_label("svg").with_label_visible()),
      ),
      ("translucent status on dark", Layer::Status(Status::new(at(400.0, 240.0), "状态 status 半透明"))),
    ]
  }

  extern "system" fn backdrop_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == WM_PAINT {
      unsafe {
        let mut paint = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut paint);
        // Light checkerboard on the left half, dark on the right half.
        let colors = [
          (COLORREF(0x00FF_FFFF), COLORREF(0x00E0_E0E0)),
          (COLORREF(0x0030_241F), COLORREF(0x0042_322B)),
        ];
        let brushes = colors.map(|(a, b)| (CreateSolidBrush(a), CreateSolidBrush(b)));
        for row in 0..HEIGHT / CELL + 1 {
          for column in 0..WIDTH / CELL + 1 {
            let half = usize::from(column * CELL >= WIDTH / 2);
            let brush = if (row + column) % 2 == 0 {
              brushes[half].0
            } else {
              brushes[half].1
            };
            let cell = RECT {
              left: column * CELL,
              top: row * CELL,
              right: column * CELL + CELL,
              bottom: row * CELL + CELL,
            };
            FillRect(dc, &cell, brush);
          }
        }
        for (a, b) in brushes {
          let _ = DeleteObject(a);
          let _ = DeleteObject(b);
        }
        let _ = EndPaint(hwnd, &paint);
      }
      return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
  }

  fn pump(duration: Duration) {
    let started = Instant::now();
    while started.elapsed() < duration {
      unsafe {
        let mut message = MSG::default();
        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
          let _ = TranslateMessage(&message);
          DispatchMessageW(&message);
        }
      }
      std::thread::sleep(Duration::from_millis(10));
    }
  }

  fn show_backdrop() -> Result<HWND, String> {
    let instance = unsafe { GetModuleHandleW(None) }.map_err(|error| error.to_string())?;
    let class = WNDCLASSEXW {
      cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
      lpfnWndProc: Some(backdrop_proc),
      hInstance: instance.into(),
      lpszClassName: w!("AuvOverlayGalleryBackdrop"),
      ..Default::default()
    };
    unsafe {
      let _ = RegisterClassExW(&class);
    }
    let hwnd = unsafe {
      CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        w!("AuvOverlayGalleryBackdrop"),
        w!("AUV overlay gallery"),
        WS_POPUP,
        LEFT,
        TOP,
        WIDTH,
        HEIGHT,
        None,
        None,
        instance,
        None,
      )
    }
    .map_err(|error| format!("failed to create backdrop: {error}"))?;
    unsafe {
      let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
      let _ = UpdateWindow(hwnd);
    }
    pump(Duration::from_millis(200));
    Ok(hwnd)
  }

  /// Captures the backdrop rectangle from the composited screen, including layered windows.
  fn capture() -> Result<Vec<u8>, String> {
    unsafe {
      let screen = GetDC(None);
      let memory = CreateCompatibleDC(screen);
      let bitmap = CreateCompatibleBitmap(screen, WIDTH, HEIGHT);
      let previous = SelectObject(memory, bitmap);
      let copied = BitBlt(memory, 0, 0, WIDTH, HEIGHT, screen, LEFT, TOP, SRCCOPY | CAPTUREBLT);
      SelectObject(memory, previous);

      let mut info = BITMAPINFO::default();
      info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
      info.bmiHeader.biWidth = WIDTH;
      info.bmiHeader.biHeight = -HEIGHT;
      info.bmiHeader.biPlanes = 1;
      info.bmiHeader.biBitCount = 32;
      let mut pixels = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
      let rows = GetDIBits(memory, bitmap, 0, HEIGHT as u32, Some(pixels.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);

      let _ = DeleteObject(bitmap);
      let _ = DeleteDC(memory);
      ReleaseDC(None, screen);
      copied.map_err(|error| format!("screen capture failed: {error}"))?;
      if rows != HEIGHT {
        return Err("failed to read captured pixels".to_string());
      }
      Ok(pixels)
    }
  }

  /// Writes top-down 32-bit BGRX pixels as a BMP file.
  fn write_bmp(path: &Path, pixels: &[u8]) -> Result<(), String> {
    let mut file = Vec::with_capacity(54 + pixels.len());
    let file_size = (54 + pixels.len()) as u32;
    file.extend_from_slice(b"BM");
    file.extend_from_slice(&file_size.to_le_bytes());
    file.extend_from_slice(&0u32.to_le_bytes());
    file.extend_from_slice(&54u32.to_le_bytes());
    file.extend_from_slice(&40u32.to_le_bytes());
    file.extend_from_slice(&WIDTH.to_le_bytes());
    file.extend_from_slice(&(-HEIGHT).to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&32u16.to_le_bytes());
    file.extend_from_slice(&[0u8; 24]);
    file.extend_from_slice(pixels);
    std::fs::write(path, file).map_err(|error| format!("failed to write {}: {error}", path.display()))
  }

  pub(super) fn run(output: &Path) -> Result<(), String> {
    let backdrop = show_backdrop()?;
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

    let captured = capture();
    let _ = auv_driver_overlay_windows::remove();
    unsafe {
      let _ = DestroyWindow(backdrop);
    }
    write_bmp(output, &captured?)?;
    println!("saved {}", output.display());
    Ok(())
  }
}
