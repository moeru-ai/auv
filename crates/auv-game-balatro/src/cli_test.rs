use super::*;

#[test]
fn ui_digit_reader_segments_multiple_glyphs() {
  let image = synthetic_ui_digit_image("300");

  let reading = infer_ui_digit_text_from_image_with_foreground(&image, UiDigitForeground::Colored);

  assert_eq!(reading.as_deref(), Some("300"));
}

#[test]
fn ui_digit_score_reading_formats_mult_label() {
  assert_eq!(ui_digit_text_for_label("ui_score_mult", "3").as_deref(), Some("x3"));
  assert_eq!(ui_digit_text_for_label("ui_score_target_score", "300").as_deref(), Some("300"));
}

#[test]
fn ui_digit_score_reading_drops_round_score_chip_icon() {
  assert_eq!(ui_digit_text_for_label("ui_score_round_score", "00").as_deref(), Some("0"));
  assert_eq!(ui_digit_text_for_label("ui_score_round_score", "0300").as_deref(), Some("300"));
}

#[test]
fn ui_digit_reader_matches_balatro_thick_one() {
  let mask = mask_from_rows([
    "####.", "####.", ".###.", ".###.", ".###.", "#####", "#####",
  ]);

  assert_eq!(infer_ui_digit_from_mask(&mask), Some(1));
}

#[test]
fn white_ui_digit_reader_ignores_colored_score_background() {
  let mut image = RgbaImage::from_pixel(80, 56, image::Rgba([220, 70, 60, 255]));
  draw_synthetic_ui_digit(&mut image, '0', 20, image::Rgba([245, 245, 245, 255]));

  let reading = infer_ui_digit_text_from_image_with_foreground(&image, UiDigitForeground::White);

  assert_eq!(reading.as_deref(), Some("0"));
}

#[test]
fn ui_digit_reader_ignores_score_punctuation_sized_glyphs() {
  let mut image = RgbaImage::from_pixel(240, 56, image::Rgba([20, 25, 24, 255]));
  let color = image::Rgba([240, 80, 60, 255]);
  draw_synthetic_ui_digit_scaled(&mut image, '1', 0, 8, color);
  draw_synthetic_ui_digit_scaled(&mut image, '4', 44, 8, color);
  draw_synthetic_ui_digit_scaled(&mut image, '4', 90, 5, color);
  draw_synthetic_ui_digit_scaled(&mut image, '0', 132, 8, color);
  draw_synthetic_ui_digit_scaled(&mut image, '4', 176, 8, color);

  let reading = infer_ui_digit_text_from_image_with_foreground(&image, UiDigitForeground::Colored);

  assert_eq!(reading.as_deref(), Some("1404"));
}

fn synthetic_ui_digit_image(text: &str) -> RgbaImage {
  let scale = 8;
  let gap = 4;
  let width = text.len() as u32 * UI_DIGIT_MASK_W as u32 * scale + text.len().saturating_sub(1) as u32 * gap;
  let height = UI_DIGIT_MASK_H as u32 * scale;
  let mut image = RgbaImage::from_pixel(width, height, image::Rgba([20, 25, 24, 255]));
  let mut cursor_x = 0;
  for character in text.chars() {
    draw_synthetic_ui_digit(&mut image, character, cursor_x, image::Rgba([240, 80, 60, 255]));
    cursor_x += UI_DIGIT_MASK_W as u32 * scale + gap;
  }
  image
}

fn draw_synthetic_ui_digit(image: &mut RgbaImage, character: char, cursor_x: u32, color: image::Rgba<u8>) {
  draw_synthetic_ui_digit_scaled(image, character, cursor_x, 8, color);
}

fn draw_synthetic_ui_digit_scaled(image: &mut RgbaImage, character: char, cursor_x: u32, scale: u32, color: image::Rgba<u8>) {
  let digit = character.to_digit(10).unwrap() as u8;
  let template = UI_DIGIT_TEMPLATES.iter().find(|template| template.digit == digit).unwrap();
  for (row_index, row) in template.rows.iter().enumerate() {
    for (column_index, pixel) in row.chars().enumerate() {
      if pixel != '#' {
        continue;
      }
      for y in 0..scale {
        for x in 0..scale {
          image.put_pixel(cursor_x + column_index as u32 * scale + x, row_index as u32 * scale + y, color);
        }
      }
    }
  }
}

fn mask_from_rows(rows: [&str; 7]) -> [bool; UI_DIGIT_MASK_CELLS] {
  let mut mask = [false; UI_DIGIT_MASK_CELLS];
  for (row_index, row) in rows.iter().enumerate() {
    for (column_index, character) in row.chars().enumerate() {
      mask[row_index * UI_DIGIT_MASK_W + column_index] = character == '#';
    }
  }
  mask
}
