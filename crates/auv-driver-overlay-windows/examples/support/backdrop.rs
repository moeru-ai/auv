//! Backdrop window and screen capture shared by the overlay examples.
//!
//! The backdrop is a topmost, non-activating window this example owns: a light checkerboard
//! on its left half and a dark one on its right, so antialiasing and translucency are
//! visible. Only its rectangle is captured, so no other window's content ends up in an
//! image. It never takes focus and never touches another window.

use std::path::Path;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
  BITMAPINFO, BITMAPINFOHEADER, BeginPaint, BitBlt, CAPTUREBLT, CreateCompatibleBitmap, CreateCompatibleDC, CreateSolidBrush,
  DIB_RGB_COLORS, DeleteDC, DeleteObject, EndPaint, FillRect, GetDC, GetDIBits, PAINTSTRUCT, ReleaseDC, SRCCOPY, SelectObject, UpdateWindow,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
  CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, MSG, PM_REMOVE, PeekMessageW, RegisterClassExW,
  SW_SHOWNOACTIVATE, ShowWindow, TranslateMessage, WM_PAINT, WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::w;

const CELL: i32 = 12;

pub struct Backdrop {
  hwnd: HWND,
  pub left: i32,
  pub top: i32,
  pub width: i32,
  pub height: i32,
}

extern "system" fn backdrop_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
  if message == WM_PAINT {
    unsafe {
      let mut paint = PAINTSTRUCT::default();
      let dc = BeginPaint(hwnd, &mut paint);
      let mut client = RECT::default();
      let _ = GetClientRect(hwnd, &mut client);
      let (width, height) = (client.right, client.bottom);
      let colors = [
        (COLORREF(0x00FF_FFFF), COLORREF(0x00E0_E0E0)),
        (COLORREF(0x0030_241F), COLORREF(0x0042_322B)),
      ];
      let brushes = colors.map(|(a, b)| (CreateSolidBrush(a), CreateSolidBrush(b)));
      for row in 0..height / CELL + 1 {
        for column in 0..width / CELL + 1 {
          let half = usize::from(column * CELL >= width / 2);
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

/// Dispatches this thread's window messages for `duration`, so the backdrop paints.
pub fn pump(duration: Duration) {
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

impl Backdrop {
  pub fn show(left: i32, top: i32, width: i32, height: i32) -> Result<Self, String> {
    let instance = unsafe { GetModuleHandleW(None) }.map_err(|error| error.to_string())?;
    let class = WNDCLASSEXW {
      cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
      lpfnWndProc: Some(backdrop_proc),
      hInstance: instance.into(),
      lpszClassName: w!("AuvOverlayExampleBackdrop"),
      ..Default::default()
    };
    unsafe {
      let _ = RegisterClassExW(&class);
    }
    let hwnd = unsafe {
      CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        w!("AuvOverlayExampleBackdrop"),
        w!("AUV overlay example"),
        WS_POPUP,
        left,
        top,
        width,
        height,
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
    Ok(Self {
      hwnd,
      left,
      top,
      width,
      height,
    })
  }

  /// Captures the backdrop rectangle from the composited screen, including layered windows,
  /// as top-down 32-bit BGRX pixels.
  pub fn capture(&self) -> Result<Vec<u8>, String> {
    capture_region(self.left, self.top, self.width, self.height)
  }

  pub fn close(self) {
    unsafe {
      let _ = DestroyWindow(self.hwnd);
    }
  }
}

/// Captures a screen rectangle from the composited desktop, including layered windows, as
/// top-down 32-bit BGRX pixels.
pub fn capture_region(left: i32, top: i32, width: i32, height: i32) -> Result<Vec<u8>, String> {
  unsafe {
    let screen = GetDC(None);
    let memory = CreateCompatibleDC(screen);
    let bitmap = CreateCompatibleBitmap(screen, width, height);
    let previous = SelectObject(memory, bitmap);
    let copied = BitBlt(memory, 0, 0, width, height, screen, left, top, SRCCOPY | CAPTUREBLT);
    SelectObject(memory, previous);

    let mut info = BITMAPINFO::default();
    info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = width;
    info.bmiHeader.biHeight = -height;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let rows = GetDIBits(memory, bitmap, 0, height as u32, Some(pixels.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);

    let _ = DeleteObject(bitmap);
    let _ = DeleteDC(memory);
    ReleaseDC(None, screen);
    copied.map_err(|error| format!("screen capture failed: {error}"))?;
    if rows != height {
      return Err("failed to read captured pixels".to_string());
    }
    Ok(pixels)
  }
}

/// Counts the pixels of a top-down BGRX capture `width` pixels wide, inside `x0..x1` by
/// `y0..y1`, whose color is within `tolerance` of `rgb` (straight RGB distance).
pub fn count_near(pixels: &[u8], width: i32, (x0, x1): (i32, i32), (y0, y1): (i32, i32), rgb: [u8; 3], tolerance: f64) -> usize {
  let height = (pixels.len() / 4) as i32 / width;
  let mut count = 0;
  for y in y0.max(0)..y1.min(height) {
    for x in x0.max(0)..x1.min(width) {
      let index = ((y * width + x) * 4) as usize;
      let (blue, green, red) = (pixels[index], pixels[index + 1], pixels[index + 2]);
      let channel = |a: u8, b: u8| f64::from(a) - f64::from(b);
      if channel(red, rgb[0]).hypot(channel(green, rgb[1])).hypot(channel(blue, rgb[2])) <= tolerance {
        count += 1;
      }
    }
  }
  count
}

/// Writes top-down 32-bit BGRX pixels as a BMP file.
pub fn write_bmp(path: &Path, width: i32, height: i32, pixels: &[u8]) -> Result<(), String> {
  let mut file = Vec::with_capacity(54 + pixels.len());
  let file_size = (54 + pixels.len()) as u32;
  file.extend_from_slice(b"BM");
  file.extend_from_slice(&file_size.to_le_bytes());
  file.extend_from_slice(&0u32.to_le_bytes());
  file.extend_from_slice(&54u32.to_le_bytes());
  file.extend_from_slice(&40u32.to_le_bytes());
  file.extend_from_slice(&width.to_le_bytes());
  file.extend_from_slice(&(-height).to_le_bytes());
  file.extend_from_slice(&1u16.to_le_bytes());
  file.extend_from_slice(&32u16.to_le_bytes());
  file.extend_from_slice(&[0u8; 24]);
  file.extend_from_slice(pixels);
  std::fs::write(path, file).map_err(|error| format!("failed to write {}: {error}", path.display()))
}
