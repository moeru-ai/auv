use crate::{BoundingBox, ImageFrame};
use image::{Rgb, RgbImage, imageops::FilterType};
use ndarray::Array4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Letterbox {
  pub source_width: u32,
  pub source_height: u32,
  pub input_size: u32,
  pub scale: f32,
  pub pad_left: u32,
  pub pad_top: u32,
}

pub fn prepare_input(frame: &ImageFrame, input_size: u32) -> (Array4<f32>, Letterbox) {
  let source_width = frame.image.width();
  let source_height = frame.image.height();
  assert!(source_width > 0, "letterbox source width must be non-zero");
  assert!(
    source_height > 0,
    "letterbox source height must be non-zero"
  );
  assert!(input_size > 0, "letterbox input size must be non-zero");

  let scale =
    (input_size as f32 / source_width as f32).min(input_size as f32 / source_height as f32);
  let resized_width = ((source_width as f32 * scale).round() as u32).max(1);
  let resized_height = ((source_height as f32 * scale).round() as u32).max(1);
  let pad_left = (input_size - resized_width) / 2;
  let pad_top = (input_size - resized_height) / 2;

  let resized = image::imageops::resize(
    &frame.image,
    resized_width,
    resized_height,
    FilterType::Triangle,
  );
  let mut padded = RgbImage::from_pixel(input_size, input_size, Rgb([114, 114, 114]));
  for y in 0..resized_height {
    for x in 0..resized_width {
      padded.put_pixel(pad_left + x, pad_top + y, *resized.get_pixel(x, y));
    }
  }

  let mut tensor = Array4::zeros((1, 3, input_size as usize, input_size as usize));
  for (x, y, pixel) in padded.enumerate_pixels() {
    let y = y as usize;
    let x = x as usize;
    tensor[[0, 0, y, x]] = pixel[0] as f32 / 255.0;
    tensor[[0, 1, y, x]] = pixel[1] as f32 / 255.0;
    tensor[[0, 2, y, x]] = pixel[2] as f32 / 255.0;
  }

  (
    tensor,
    Letterbox {
      source_width,
      source_height,
      input_size,
      scale,
      pad_left,
      pad_top,
    },
  )
}

pub fn project_model_bbox_to_source(bbox: BoundingBox, letterbox: Letterbox) -> BoundingBox {
  let project_x = |x: f32| {
    ((x - letterbox.pad_left as f32) / letterbox.scale).clamp(0.0, letterbox.source_width as f32)
  };
  let project_y = |y: f32| {
    ((y - letterbox.pad_top as f32) / letterbox.scale).clamp(0.0, letterbox.source_height as f32)
  };

  BoundingBox {
    x1: project_x(bbox.x1),
    y1: project_y(bbox.y1),
    x2: project_x(bbox.x2),
    y2: project_y(bbox.y2),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use image::{Rgb, RgbImage};

  #[test]
  fn wide_image_into_square_records_scale_padding_and_tensor_shape() {
    let frame = ImageFrame::new(RgbImage::new(1280, 720));

    let (tensor, letterbox) = prepare_input(&frame, 640);

    assert_eq!(tensor.shape(), &[1, 3, 640, 640]);
    assert_eq!(
      letterbox,
      Letterbox {
        source_width: 1280,
        source_height: 720,
        input_size: 640,
        scale: 0.5,
        pad_left: 0,
        pad_top: 140,
      }
    );
  }

  #[test]
  fn no_resize_image_converts_rgb_to_normalized_chw_values() {
    let image = RgbImage::from_fn(2, 2, |x, y| match (x, y) {
      (0, 0) => Rgb([255, 128, 0]),
      _ => Rgb([0, 0, 0]),
    });
    let frame = ImageFrame::new(image);

    let (tensor, _) = prepare_input(&frame, 2);

    assert_eq!(tensor[[0, 0, 0, 0]], 1.0);
    assert_eq!(tensor[[0, 1, 0, 0]], 128.0 / 255.0);
    assert_eq!(tensor[[0, 2, 0, 0]], 0.0);
  }

  #[test]
  fn projection_removes_padding_and_scale() {
    let bbox = BoundingBox {
      x1: 100.0,
      y1: 130.0,
      x2: 200.0,
      y2: 230.0,
    };
    let letterbox = Letterbox {
      source_width: 1280,
      source_height: 720,
      input_size: 640,
      scale: 0.5,
      pad_left: 0,
      pad_top: 100,
    };

    let projected = project_model_bbox_to_source(bbox, letterbox);

    assert_eq!(
      projected,
      BoundingBox {
        x1: 200.0,
        y1: 60.0,
        x2: 400.0,
        y2: 260.0,
      }
    );
  }

  #[test]
  fn projection_clamps_to_source_bounds() {
    let bbox = BoundingBox {
      x1: -10.0,
      y1: 20.0,
      x2: 700.0,
      y2: 500.0,
    };
    let letterbox = Letterbox {
      source_width: 100,
      source_height: 50,
      input_size: 640,
      scale: 2.0,
      pad_left: 20,
      pad_top: 40,
    };

    let projected = project_model_bbox_to_source(bbox, letterbox);

    assert_eq!(
      projected,
      BoundingBox {
        x1: 0.0,
        y1: 0.0,
        x2: 100.0,
        y2: 50.0,
      }
    );
  }
}
