// TODO(auv-inference-yolo-render): annotation rendering is deferred to Task 7;
// this placeholder keeps earlier slices testable without claiming render
// behavior.
pub fn render_annotated_image(
  _image: &image::RgbImage,
  _detections: &[crate::Detection],
) -> image::RgbImage {
  unimplemented!("annotation rendering is deferred to Task 7")
}
