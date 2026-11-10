use super::*;

fn retina_capture() -> Capture {
  // 6x4 backing pixels for a 3x2 point window: alternating black and color columns.
  Capture {
    origin: None,
    image: RgbaImage::from_fn(6, 4, |x, _| {
      if x % 2 == 0 {
        image::Rgba([0, 0, 0, 255])
      } else {
        image::Rgba([200, 100, 50, 255])
      }
    }),
    bounds: Rect::new(10.0, 20.0, 3.0, 2.0),
    scale_factor: 2.0,
    backend: "fixture".to_string(),
    fallback_reason: None,
  }
}

#[test]
fn logical_resolution_averages_backing_pixels_into_points() {
  let logical = retina_capture().at_resolution(CaptureResolution::Logical);
  assert_eq!(logical.image.dimensions(), (3, 2), "one pixel per point");
  assert_eq!(logical.image.get_pixel(1, 1), &image::Rgba([100, 50, 25, 255]), "areas are averaged");
  assert_eq!(logical.scale_factor, 1.0);
  assert_eq!(logical.bounds, Rect::new(10.0, 20.0, 3.0, 2.0), "bounds stay in points");
}

#[test]
fn native_resolution_and_one_x_captures_are_unchanged() {
  assert_eq!(retina_capture().at_resolution(CaptureResolution::Native), retina_capture());
  let one_x = retina_capture().at_resolution(CaptureResolution::Logical);
  assert_eq!(one_x.clone().at_resolution(CaptureResolution::Logical), one_x);
}
