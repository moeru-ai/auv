use futures_util::AsyncReadExt as _;

use super::*;

struct Encoded {
  content_type: String,
  extension: Option<String>,
  purpose: String,
  attributes: Attributes,
  body: Vec<u8>,
}

fn detached(artifact: NewArtifact<Cursor<Vec<u8>>>) -> Encoded {
  let mut detached = artifact.detach();
  let mut body = Vec::new();
  futures_executor::block_on(detached.body.read_to_end(&mut body)).unwrap();
  Encoded {
    content_type: detached.content_type.as_str().to_string(),
    extension: detached.file_extension,
    purpose: detached.purpose.as_str().to_string(),
    attributes: detached.attributes,
    body,
  }
}

#[test]
fn native_image_artifacts_are_lossless_webp_with_their_purpose() {
  let image = image::RgbaImage::from_fn(5, 3, |x, y| image::Rgba([x as u8 * 40, y as u8 * 80, 7, 200]));
  let encoded = detached(image_artifact(EmitBytesOptions::new().with_purpose("auv.test.capture"), &image, ImageResolution::Native).unwrap());
  assert_eq!(
    (encoded.content_type.as_str(), encoded.extension.as_deref(), encoded.purpose.as_str()),
    ("image/webp", Some("webp"), "auv.test.capture")
  );
  assert_eq!(image::load_from_memory(&encoded.body).unwrap().to_rgba8(), image, "native evidence keeps every pixel");
  assert!(encoded.attributes.is_empty());
}

#[test]
fn rgb_images_encode_without_an_alpha_copy() {
  let image = image::RgbImage::from_pixel(4, 4, image::Rgb([10, 20, 30]));
  let encoded = detached(image_artifact(EmitBytesOptions::new().with_purpose("auv.test.overlay"), &image, ImageResolution::Native).unwrap());
  assert_eq!(image::load_from_memory(&encoded.body).unwrap().to_rgb8(), image);
}

#[test]
fn logical_artifacts_average_backing_pixels_and_record_their_source() {
  // Each 2x2 block of a Retina capture becomes one logical pixel.
  let image = image::RgbaImage::from_fn(6, 4, |x, _| {
    if x % 2 == 0 {
      image::Rgba([0, 0, 0, 255])
    } else {
      image::Rgba([200, 100, 50, 255])
    }
  });
  let options = EmitBytesOptions::new()
    .with_purpose("auv.test.capture")
    .with_attributes(Attributes::from_iter([("source", AttributeValue::string("window:7"))]));
  let encoded = detached(image_artifact(options, &image, ImageResolution::Logical(2.0)).unwrap());

  let decoded = image::load_from_memory(&encoded.body).unwrap().to_rgba8();
  assert_eq!(decoded.dimensions(), (3, 2));
  assert_eq!(decoded.get_pixel(1, 1), &image::Rgba([100, 50, 25, 255]), "areas are averaged, not sampled");
  assert_eq!(encoded.attributes.get("source"), Some(&AttributeValue::string("window:7")), "caller attributes are kept");
  assert_eq!(encoded.attributes.get("image.source_width"), Some(&AttributeValue::integer(6)));
  assert_eq!(encoded.attributes.get("image.source_height"), Some(&AttributeValue::integer(4)));
  assert_eq!(encoded.attributes.get("image.scale_factor"), Some(&AttributeValue::float(2.0).unwrap()));
}

#[test]
fn logical_artifacts_at_one_x_or_invalid_scale_keep_native_pixels() {
  let image = image::RgbaImage::from_pixel(4, 2, image::Rgba([1, 2, 3, 255]));
  for scale in [1.0, 0.5, f64::NAN] {
    let encoded =
      detached(image_artifact(EmitBytesOptions::new().with_purpose("auv.test.capture"), &image, ImageResolution::Logical(scale)).unwrap());
    assert_eq!(image::load_from_memory(&encoded.body).unwrap().to_rgba8().dimensions(), (4, 2), "scale {scale}");
    assert!(encoded.attributes.is_empty(), "scale {scale}");
  }
}
