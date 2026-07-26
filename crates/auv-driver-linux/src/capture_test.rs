use super::*;

#[test]
fn portal_crop_maps_logical_bounds_to_screenshot_pixels() {
  let mut image = image::RgbaImage::new(100, 50);
  image.put_pixel(20, 10, image::Rgba([1, 2, 3, 4]));

  // ROOT CAUSE:
  //
  // If the portal returned an image in a different pixel size than GNOME's
  // logical display bounds, direct coordinate cropping rejected valid regions.
  //
  // Before the fix, a logical 200x100 display could not crop from a 100x50
  // portal image. The fix maps source bounds to image pixels before cropping.
  let cropped = crop_portal_screenshot_to_region(image, Rect::new(0.0, 0.0, 200.0, 100.0), Rect::new(40.0, 20.0, 20.0, 20.0))
    .expect("portal crop maps through source bounds");

  assert_eq!(cropped.width(), 10);
  assert_eq!(cropped.height(), 10);
  assert_eq!(*cropped.get_pixel(0, 0), image::Rgba([1, 2, 3, 4]));
}
