use auv_driver_common::vision::TextRecognitionOptions;
use auv_driver_linux::recognize_text_in_rgba;

const GLYPH_WIDTH: usize = 5;
const GLYPH_HEIGHT: usize = 7;
const SCALE: usize = 12;
const MARGIN: usize = 24;
const GLYPHS: [[&str; GLYPH_HEIGHT]; 5] = [
  [
    "10001", "10001", "10001", "11111", "10001", "10001", "10001",
  ],
  [
    "11111", "10000", "10000", "11110", "10000", "10000", "11111",
  ],
  [
    "10000", "10000", "10000", "10000", "10000", "10000", "11111",
  ],
  [
    "10000", "10000", "10000", "10000", "10000", "10000", "11111",
  ],
  [
    "01110", "10001", "10001", "10001", "10001", "10001", "01110",
  ],
];

fn main() {
  let spacing = SCALE;
  let width = MARGIN * 2 + GLYPHS.len() * GLYPH_WIDTH * SCALE + (GLYPHS.len() - 1) * spacing;
  let height = MARGIN * 2 + GLYPH_HEIGHT * SCALE;
  let mut rgba = vec![255; width * height * 4];

  for (glyph_index, glyph) in GLYPHS.iter().enumerate() {
    let origin_x = MARGIN + glyph_index * (GLYPH_WIDTH * SCALE + spacing);
    for (row, pixels) in glyph.iter().enumerate() {
      for (column, pixel) in pixels.bytes().enumerate() {
        if pixel != b'1' {
          continue;
        }
        for y in MARGIN + row * SCALE..MARGIN + (row + 1) * SCALE {
          for x in origin_x + column * SCALE..origin_x + (column + 1) * SCALE {
            let offset = (y * width + x) * 4;
            rgba[offset..offset + 3].fill(0);
          }
        }
      }
    }
  }

  let recognition = recognize_text_in_rgba(&rgba, width as u32, height as u32, &TextRecognitionOptions::default())
    .unwrap_or_else(|error| panic!("bundled OCR failed: {error}"));
  let normalized = recognition.text.chars().filter(|character| character.is_alphanumeric()).collect::<String>().to_lowercase();
  assert!(normalized.starts_with("hell"), "expected bundled OCR to recognize the HELLO fixture, got {:?}", recognition.text);
  println!("{}", recognition.text.trim());
}
