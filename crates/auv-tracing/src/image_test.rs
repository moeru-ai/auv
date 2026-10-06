use futures_util::AsyncReadExt as _;

use super::*;

/// Content type, extension, purpose and body of an encoded artifact.
fn detached(artifact: NewArtifact<Cursor<Vec<u8>>>) -> (String, Option<String>, String, Vec<u8>) {
  let mut detached = artifact.detach();
  let mut body = Vec::new();
  futures_executor::block_on(detached.body.read_to_end(&mut body)).unwrap();
  (detached.content_type.as_str().to_string(), detached.file_extension, detached.purpose.as_str().to_string(), body)
}

#[test]
fn image_artifacts_are_lossless_webp_with_their_purpose() {
  let image = image::RgbaImage::from_fn(5, 3, |x, y| image::Rgba([x as u8 * 40, y as u8 * 80, 7, 200]));
  let (content_type, extension, purpose, body) =
    detached(image_artifact(EmitBytesOptions::new().with_purpose("auv.test.capture"), &image).unwrap());
  assert_eq!((content_type.as_str(), extension.as_deref(), purpose.as_str()), ("image/webp", Some("webp"), "auv.test.capture"));
  assert_eq!(image::load_from_memory(&body).unwrap().to_rgba8(), image, "evidence keeps every pixel");
}

#[test]
fn rgb_images_encode_without_an_alpha_copy() {
  let image = image::RgbImage::from_pixel(4, 4, image::Rgb([10, 20, 30]));
  let (_, _, _, body) = detached(image_artifact(EmitBytesOptions::new().with_purpose("auv.test.overlay"), &image).unwrap());
  assert_eq!(image::load_from_memory(&body).unwrap().to_rgb8(), image);
}
