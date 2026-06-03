use crate::{BoundingBox, Detection};
use image::{Rgb, RgbImage};

pub fn render_annotated_image(image: &RgbImage, detections: &[Detection]) -> RgbImage {
  let mut annotated = image.clone();
  if annotated.width() == 0 || annotated.height() == 0 {
    return annotated;
  }

  for detection in detections {
    let Some((x1, y1, x2, y2)) =
      clamped_bbox(detection.bbox, annotated.width(), annotated.height())
    else {
      continue;
    };
    let color = class_color(detection.class_id);

    for x in x1..=x2 {
      annotated.put_pixel(x, y1, color);
      annotated.put_pixel(x, y2, color);
    }
    for y in y1..=y2 {
      annotated.put_pixel(x1, y, color);
      annotated.put_pixel(x2, y, color);
    }
  }

  annotated
}

fn clamped_bbox(
  bbox: BoundingBox,
  image_width: u32,
  image_height: u32,
) -> Option<(u32, u32, u32, u32)> {
  if !bbox.x1.is_finite() || !bbox.y1.is_finite() || !bbox.x2.is_finite() || !bbox.y2.is_finite() {
    return None;
  }

  let min_x = bbox
    .x1
    .min(bbox.x2)
    .floor()
    .clamp(0.0, (image_width - 1) as f32) as u32;
  let max_x = bbox
    .x1
    .max(bbox.x2)
    .ceil()
    .clamp(0.0, (image_width - 1) as f32) as u32;
  let min_y = bbox
    .y1
    .min(bbox.y2)
    .floor()
    .clamp(0.0, (image_height - 1) as f32) as u32;
  let max_y = bbox
    .y1
    .max(bbox.y2)
    .ceil()
    .clamp(0.0, (image_height - 1) as f32) as u32;

  Some((min_x, min_y, max_x, max_y))
}

fn class_color(class_id: usize) -> Rgb<u8> {
  const PALETTE: [Rgb<u8>; 12] = [
    Rgb([230, 25, 75]),
    Rgb([60, 180, 75]),
    Rgb([0, 130, 200]),
    Rgb([245, 130, 48]),
    Rgb([145, 30, 180]),
    Rgb([70, 240, 240]),
    Rgb([240, 50, 230]),
    Rgb([210, 245, 60]),
    Rgb([250, 190, 190]),
    Rgb([0, 128, 128]),
    Rgb([230, 190, 255]),
    Rgb([170, 110, 40]),
  ];

  PALETTE[class_id % PALETTE.len()]
}

#[cfg(test)]
mod tests {
  use super::*;

  fn detection(class_id: usize, bbox: BoundingBox) -> Detection {
    Detection {
      class_id,
      label: format!("class-{class_id}"),
      confidence: 0.9,
      bbox,
    }
  }

  #[test]
  fn render_changes_bbox_border_and_preserves_background() {
    let source = RgbImage::from_pixel(8, 8, Rgb([8, 9, 10]));

    let rendered = render_annotated_image(
      &source,
      &[detection(
        1,
        BoundingBox {
          x1: 2.0,
          y1: 2.0,
          x2: 5.0,
          y2: 5.0,
        },
      )],
    );

    assert_ne!(rendered.get_pixel(2, 2), source.get_pixel(2, 2));
    assert_eq!(rendered.get_pixel(0, 0), source.get_pixel(0, 0));
  }

  #[test]
  fn render_clamps_bbox_to_image_bounds() {
    let source = RgbImage::from_pixel(3, 3, Rgb([20, 30, 40]));

    let rendered = render_annotated_image(
      &source,
      &[detection(
        2,
        BoundingBox {
          x1: -5.0,
          y1: -4.0,
          x2: 8.0,
          y2: 7.0,
        },
      )],
    );

    assert_ne!(rendered.get_pixel(0, 0), source.get_pixel(0, 0));
    assert_ne!(rendered.get_pixel(2, 2), source.get_pixel(2, 2));
  }
}
