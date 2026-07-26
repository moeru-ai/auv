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
