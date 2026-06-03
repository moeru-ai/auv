use crate::{BoundingBox, ImageFrame};
use image::{Rgb, RgbImage};
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

  let resized = resize_linear(&frame.image, resized_width, resized_height);
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

fn resize_linear(image: &RgbImage, width: u32, height: u32) -> RgbImage {
  let source_width = image.width();
  let source_height = image.height();
  let scale_x = source_width as f32 / width as f32;
  let scale_y = source_height as f32 / height as f32;
  RgbImage::from_fn(width, height, |x, y| {
    let source_x = (x as f32 + 0.5) * scale_x - 0.5;
    let source_y = (y as f32 + 0.5) * scale_y - 0.5;
    interpolate(image, source_x, source_y)
  })
}

// NOTICE: The fixture generator uses OpenCV `INTER_LINEAR`. `image::resize`
// uses different downsampling semantics, which changes low-confidence YOLO
// candidates enough to break Balatro parity.
fn interpolate(image: &RgbImage, x: f32, y: f32) -> Rgb<u8> {
  let x0_raw = x.floor();
  let y0_raw = y.floor();
  let x0 = clamp_index(x0_raw as i32, image.width());
  let y0 = clamp_index(y0_raw as i32, image.height());
  let x1 = clamp_index(x0_raw as i32 + 1, image.width());
  let y1 = clamp_index(y0_raw as i32 + 1, image.height());
  let x_weight = x - x0_raw;
  let y_weight = y - y0_raw;
  let top_left = image.get_pixel(x0, y0);
  let top_right = image.get_pixel(x1, y0);
  let bottom_left = image.get_pixel(x0, y1);
  let bottom_right = image.get_pixel(x1, y1);
  Rgb([
    interpolate_channel(
      top_left[0],
      top_right[0],
      bottom_left[0],
      bottom_right[0],
      x_weight,
      y_weight,
    ),
    interpolate_channel(
      top_left[1],
      top_right[1],
      bottom_left[1],
      bottom_right[1],
      x_weight,
      y_weight,
    ),
    interpolate_channel(
      top_left[2],
      top_right[2],
      bottom_left[2],
      bottom_right[2],
      x_weight,
      y_weight,
    ),
  ])
}

fn clamp_index(index: i32, len: u32) -> u32 {
  index.clamp(0, len as i32 - 1) as u32
}

fn interpolate_channel(
  top_left: u8,
  top_right: u8,
  bottom_left: u8,
  bottom_right: u8,
  x_weight: f32,
  y_weight: f32,
) -> u8 {
  let top = top_left as f32 * (1.0 - x_weight) + top_right as f32 * x_weight;
  let bottom = bottom_left as f32 * (1.0 - x_weight) + bottom_right as f32 * x_weight;
  (top * (1.0 - y_weight) + bottom * y_weight).round() as u8
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
  fn resize_uses_replicated_border_for_tiny_source_images() {
    let frame = ImageFrame::new(RgbImage::from_pixel(1, 1, Rgb([7, 11, 13])));

    let (tensor, letterbox) = prepare_input(&frame, 4);

    assert_eq!(letterbox.scale, 4.0);
    for y in 0..4 {
      for x in 0..4 {
        assert_eq!(tensor[[0, 0, y, x]], 7.0 / 255.0);
        assert_eq!(tensor[[0, 1, y, x]], 11.0 / 255.0);
        assert_eq!(tensor[[0, 2, y, x]], 13.0 / 255.0);
      }
    }
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
