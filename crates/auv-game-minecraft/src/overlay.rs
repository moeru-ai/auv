use image::{Rgb, RgbImage};

use crate::types::{MinecraftProjectedPoint, RaycastHit};

const RAYCAST_MARKER_SIZE: i32 = 4;

pub fn render_projection_overlay(mut image: RgbImage, projected: &MinecraftProjectedPoint, raycast_hit: Option<&RaycastHit>) -> RgbImage {
  let width = image.width();
  let height = image.height();
  if width == 0 || height == 0 {
    return image;
  }

  if let Some(screen_point) = projected.screen_point {
    let center_x = screen_point.x.round() as i32;
    let center_y = screen_point.y.round() as i32;
    let radius = projected.match_radius_px.round().max(1.0) as i32;
    draw_crosshair(&mut image, center_x, center_y, radius, Rgb([255, 0, 0]), width, height);
    draw_box(&mut image, center_x, center_y, radius, Rgb([255, 255, 0]), width, height);
  }

  if raycast_hit.is_some() {
    draw_marker(&mut image, 6, 6, Rgb([0, 255, 255]), width, height);
  }

  image
}

fn draw_crosshair(image: &mut RgbImage, center_x: i32, center_y: i32, radius: i32, color: Rgb<u8>, width: u32, height: u32) {
  let Some((min_x, max_x)) = clip_range(center_x - radius, center_x + radius, width) else {
    return;
  };
  let Some((min_y, max_y)) = clip_range(center_y - radius, center_y + radius, height) else {
    return;
  };
  let Some(center_x) = clip_point(center_x, width) else {
    return;
  };
  let Some(center_y) = clip_point(center_y, height) else {
    return;
  };

  for x in min_x..=max_x {
    image.put_pixel(x, center_y, color);
  }
  for y in min_y..=max_y {
    image.put_pixel(center_x, y, color);
  }
}

fn draw_box(image: &mut RgbImage, center_x: i32, center_y: i32, radius: i32, color: Rgb<u8>, width: u32, height: u32) {
  let Some((min_x, max_x)) = clip_range(center_x - radius, center_x + radius, width) else {
    return;
  };
  let Some((min_y, max_y)) = clip_range(center_y - radius, center_y + radius, height) else {
    return;
  };

  for x in min_x..=max_x {
    image.put_pixel(x, min_y, color);
    image.put_pixel(x, max_y, color);
  }
  for y in min_y..=max_y {
    image.put_pixel(min_x, y, color);
    image.put_pixel(max_x, y, color);
  }
}

fn draw_marker(image: &mut RgbImage, x: i32, y: i32, color: Rgb<u8>, width: u32, height: u32) {
  let Some((min_x, max_x)) = clip_range(x, x + RAYCAST_MARKER_SIZE - 1, width) else {
    return;
  };
  let Some((min_y, max_y)) = clip_range(y, y + RAYCAST_MARKER_SIZE - 1, height) else {
    return;
  };

  for pixel_x in min_x..=max_x {
    for pixel_y in min_y..=max_y {
      image.put_pixel(pixel_x, pixel_y, color);
    }
  }
}

fn clip_point(value: i32, size: u32) -> Option<u32> {
  if size == 0 || value < 0 || value >= size as i32 {
    return None;
  }

  Some(value as u32)
}

fn clip_range(start: i32, end: i32, size: u32) -> Option<(u32, u32)> {
  if size == 0 {
    return None;
  }

  let max = size as i32 - 1;
  if end < 0 || start > max {
    return None;
  }

  Some((start.clamp(0, max) as u32, end.clamp(0, max) as u32))
}

#[cfg(test)]
mod tests {
  use image::RgbImage;

  use super::*;
  use crate::types::{BlockFace, BlockPosition, ProjectionVisibility};

  #[test]
  fn overlay_marks_projected_region_and_raycast_badge() {
    let image = RgbImage::from_pixel(32, 32, Rgb([0, 0, 0]));
    let projected = MinecraftProjectedPoint {
      screen_point: Some(auv_driver::geometry::Point::new(16.0, 16.0)),
      visibility: ProjectionVisibility::Visible,
      match_radius_px: 4.0,
      basis_frame_id: "frame-1".to_string(),
      confidence: 1.0,
    };
    let raycast_hit = RaycastHit {
      block_pos: BlockPosition::new(1, 2, 3),
      face: BlockFace::North,
      block_id: "minecraft:stone".to_string(),
    };

    let overlay = render_projection_overlay(image, &projected, Some(&raycast_hit));

    assert_eq!(overlay.width(), 32);
    assert_eq!(overlay.height(), 32);
    assert_eq!(overlay.get_pixel(16, 16), &Rgb([255, 0, 0]));
    assert_eq!(overlay.get_pixel(6, 6), &Rgb([0, 255, 255]));
  }

  #[test]
  fn overlay_clamps_projected_region_at_image_edge() {
    let image = RgbImage::from_pixel(8, 8, Rgb([0, 0, 0]));
    let projected = MinecraftProjectedPoint {
      screen_point: Some(auv_driver::geometry::Point::new(0.0, 0.0)),
      visibility: ProjectionVisibility::Visible,
      match_radius_px: 4.0,
      basis_frame_id: "frame-1".to_string(),
      confidence: 1.0,
    };

    let overlay = render_projection_overlay(image, &projected, None);

    assert_eq!(overlay.get_pixel(0, 0), &Rgb([255, 255, 0]));
    assert_eq!(overlay.get_pixel(4, 0), &Rgb([255, 255, 0]));
    assert_eq!(overlay.get_pixel(0, 4), &Rgb([255, 255, 0]));
  }

  #[test]
  fn overlay_skips_projected_region_when_point_is_fully_outside_image() {
    let image = RgbImage::from_pixel(8, 8, Rgb([0, 0, 0]));
    let projected = MinecraftProjectedPoint {
      screen_point: Some(auv_driver::geometry::Point::new(32.0, 32.0)),
      visibility: ProjectionVisibility::Visible,
      match_radius_px: 4.0,
      basis_frame_id: "frame-1".to_string(),
      confidence: 1.0,
    };

    let overlay = render_projection_overlay(image, &projected, None);

    assert!(overlay.pixels().all(|pixel| pixel == &Rgb([0, 0, 0])));
  }
}
