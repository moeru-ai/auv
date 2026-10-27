use super::*;
use auv_tracing::{ArtifactUri, Attributes, ByteLength, Sha256Digest};
use std::sync::Arc;

fn registered(bytes: &[u8], content_type: &str) -> (tempfile::TempDir, ImageArtifacts, ArtifactMetadata, PathBuf) {
  let dir = tempfile::tempdir().unwrap();
  let store = Arc::new(FileTracingStore::open(dir.path()).unwrap());
  let metadata = ArtifactMetadata::new(
    ArtifactUri::from_ids(RunId::new(), ArtifactId::new()),
    "test.image".into(),
    content_type.into(),
    Some("bin".into()),
    ByteLength::new(bytes.len() as u64).unwrap(),
    Sha256Digest::new(Sha256::digest(bytes).into()),
    Attributes::default(),
  );
  let path = store.artifact_path(&metadata);
  std::fs::create_dir_all(path.parent().unwrap()).unwrap();
  std::fs::write(&path, bytes).unwrap();
  let catalog = ImageArtifacts::default();
  catalog.register(std::slice::from_ref(&metadata), &store);
  (dir, catalog, metadata, path)
}

fn png(width: u32, height: u32) -> Vec<u8> {
  let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(width, height, image::Rgb([12, 34, 56])));
  let mut output = Cursor::new(Vec::new());
  image.write_to(&mut output, ImageFormat::Png).unwrap();
  output.into_inner()
}

#[test]
fn image_fetch_returns_decodable_mcp_png_with_aspect_ratio() {
  let (_dir, catalog, metadata, _) = registered(&png(120, 60), "image/png");
  let result = catalog.read(&metadata.uri().run_id().to_string(), &metadata.uri().artifact_id().to_string(), 40, 40).unwrap();
  let wire = serde_json::to_value(result).unwrap();
  assert_eq!(wire["content"][1]["type"], "image");
  assert_eq!(wire["content"][1]["mimeType"], "image/png");
  let decoded = base64::engine::general_purpose::STANDARD.decode(wire["content"][1]["data"].as_str().unwrap()).unwrap();
  let image = image::load_from_memory(&decoded).unwrap();
  assert_eq!((image.width(), image.height()), (40, 20));
  assert_eq!(image.to_rgb8().get_pixel(0, 0).0, [12, 34, 56]);
}

// https://github.com/moeru-ai/auv/pull/430
#[test]
fn image_fetch_pr430_applies_allocation_budget_to_header_parsing() {
  use image::ImageEncoder as _;

  // ROOT CAUSE:
  //
  // If a PNG contained compressed metadata, the dimension probe expanded it
  // under the default 512 MiB budget before the 128 MiB decode budget applied.
  //
  // Before the fix, this header passed the probe and failed only during decode.
  // The fix applies the budget to the first header parse, before pixel decoding.
  let mut bytes = Vec::new();
  let mut encoder = image::codecs::png::PngEncoder::new(&mut bytes);
  encoder.set_icc_profile(vec![0; 127 * 1024 * 1024]).unwrap();
  // The profile nearly fills the header budget. A following metadata chunk must fail
  // in the header parser, rather than in a second decoder with a lower budget.
  encoder.set_exif_metadata(vec![0; 2 * 1024 * 1024]).unwrap();
  encoder.write_image(&[255], 1, 1, image::ExtendedColorType::L8).unwrap();
  // The encoder writes EXIF first. Move that complete chunk after iCCP so its
  // allocation exposes whether the profile consumed the configured budget.
  let mut chunks = Vec::new();
  let mut offset = 8;
  while offset < bytes.len() {
    let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
    let end = offset + 12 + length;
    chunks.push(&bytes[offset..end]);
    offset = end;
  }
  let exif = chunks.iter().find(|chunk| &chunk[4..8] == b"eXIf").unwrap();
  let mut ordered = bytes[..8].to_vec();
  for chunk in &chunks {
    if &chunk[4..8] == b"IDAT" {
      ordered.extend_from_slice(exif);
    }
    if &chunk[4..8] != b"eXIf" {
      ordered.extend_from_slice(chunk);
    }
  }
  let bytes = ordered;
  assert!(bytes.len() < MAX_SOURCE_BYTES as usize);
  let (_dir, catalog, metadata, _) = registered(&bytes, "image/png");
  let error = catalog.read(&metadata.uri().run_id().to_string(), &metadata.uri().artifact_id().to_string(), 1, 1).unwrap_err();
  assert!(error.message.starts_with("invalid image header"), "{error}");
}

// https://github.com/moeru-ai/auv/pull/430
#[test]
fn image_fetch_pr430_accepts_pixels_after_oversized_icc_is_discarded() {
  use image::ImageEncoder as _;

  let mut bytes = Vec::new();
  let mut encoder = image::codecs::png::PngEncoder::new(&mut bytes);
  encoder.set_icc_profile(vec![0; 160 * 1024 * 1024]).unwrap();
  encoder.write_image(&[255], 1, 1, image::ExtendedColorType::L8).unwrap();
  assert!(bytes.len() as u64 <= MAX_SOURCE_BYTES);

  // png 0.18.1 ignores an iCCP parsing error. Confirm that the over-budget
  // profile is absent, rather than assuming successful pixels retain it.
  let mut limits = Limits::default();
  limits.max_alloc = Some(128 * 1024 * 1024);
  let mut decoder = image::codecs::png::PngDecoder::with_limits(Cursor::new(&bytes), limits).unwrap();
  assert!(decoder.icc_profile().unwrap().is_none());
  drop(decoder);

  let (_dir, catalog, metadata, _) = registered(&bytes, "image/png");
  let result = catalog.read(&metadata.uri().run_id().to_string(), &metadata.uri().artifact_id().to_string(), 1, 1).unwrap();
  let wire = serde_json::to_value(result).unwrap();
  let decoded = base64::engine::general_purpose::STANDARD.decode(wire["content"][1]["data"].as_str().unwrap()).unwrap();
  let pixels = image::load_from_memory(&decoded).unwrap().to_luma8();
  assert_eq!(pixels.dimensions(), (1, 1));
  assert_eq!(pixels.get_pixel(0, 0).0, [255]);
}

// https://github.com/moeru-ai/auv/pull/430
#[test]
fn image_fetch_pr430_resizes_16_bit_png_without_pixel_corruption() {
  // ROOT CAUSE:
  //
  // If a 16-bit PNG shrank to a small thumbnail, image 0.25.10 accumulated its
  // pixels in u32, which overflowed for sufficiently large sampling regions.
  //
  // Before the fix, debug builds panicked and release builds changed the pixels.
  // The fix preserves the channel values during bounded image resizing.
  let pixel = image::Rgba([u16::MAX, 50_000, 30_000, u16::MAX]);
  let pixels = image::DynamicImage::ImageRgba16(image::ImageBuffer::from_pixel(512, 512, pixel));
  let mut bytes = Cursor::new(Vec::new());
  pixels.write_to(&mut bytes, ImageFormat::Png).unwrap();
  let (_dir, catalog, metadata, _) = registered(bytes.get_ref(), "image/png");
  let result = catalog.read(&metadata.uri().run_id().to_string(), &metadata.uri().artifact_id().to_string(), 1, 1).unwrap();
  let wire = serde_json::to_value(result).unwrap();
  let decoded = base64::engine::general_purpose::STANDARD.decode(wire["content"][1]["data"].as_str().unwrap()).unwrap();
  let image = image::load_from_memory(&decoded).unwrap().to_rgba16();
  assert_eq!(image.dimensions(), (1, 1));
  assert_eq!(*image.get_pixel(0, 0), pixel);
}

#[test]
fn image_fetch_accepts_supported_formats_without_upscaling() {
  let pixels = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(12, 6, image::Rgb([12, 34, 56])));
  for (format, content_type) in [
    (ImageFormat::Png, "image/png"),
    (ImageFormat::Jpeg, "image/jpeg"),
    (ImageFormat::WebP, "image/webp"),
  ] {
    let mut bytes = Cursor::new(Vec::new());
    pixels.write_to(&mut bytes, format).unwrap();
    let (_directory, catalog, metadata, _) = registered(&bytes.into_inner(), content_type);
    let result = catalog.read(&metadata.uri().run_id().to_string(), &metadata.uri().artifact_id().to_string(), 2048, 2048).unwrap();
    let wire = serde_json::to_value(result).unwrap();
    assert_eq!(wire["content"][1]["mimeType"], "image/png");
    let decoded = base64::engine::general_purpose::STANDARD.decode(wire["content"][1]["data"].as_str().unwrap()).unwrap();
    let image = image::load_from_memory(&decoded).unwrap();
    assert_eq!((image.width(), image.height()), (12, 6), "{content_type}");
  }
}

#[test]
fn image_fetch_rejects_paths_wrong_run_and_other_connections() {
  let (_dir, catalog, metadata, _) = registered(&png(1, 1), "image/png");
  let run = metadata.uri().run_id().to_string();
  let artifact = metadata.uri().artifact_id().to_string();
  assert!(catalog.read("../../secret", &artifact, 10, 10).unwrap_err().message.contains("UUID"));
  assert!(catalog.read(&run, "C:\\secret.png", 10, 10).unwrap_err().message.contains("UUID"));
  assert!(catalog.read(&RunId::new().to_string(), &artifact, 10, 10).unwrap_err().message.contains("not available"));
  assert!(ImageArtifacts::default().read(&run, &artifact, 10, 10).unwrap_err().message.contains("not available"));
  assert!(catalog.read(&run, &artifact, 0, 10).unwrap_err().message.contains("1..=4096"));
  assert!(catalog.read(&run, &artifact, 10, 4097).unwrap_err().message.contains("1..=4096"));
}

#[test]
fn image_fetch_refuses_modified_missing_nonimage_and_mislabeled_bytes() {
  let (_dir, catalog, metadata, path) = registered(&png(1, 1), "image/png");
  let run = metadata.uri().run_id().to_string();
  let artifact = metadata.uri().artifact_id().to_string();
  let mut bytes = std::fs::read(&path).unwrap();
  bytes[0] ^= 1;
  std::fs::write(&path, bytes).unwrap();
  assert!(catalog.read(&run, &artifact, 10, 10).unwrap_err().message.contains("integrity"));
  std::fs::write(&path, b"short").unwrap();
  assert!(catalog.read(&run, &artifact, 10, 10).unwrap_err().message.contains("length"));
  std::fs::remove_file(&path).unwrap();
  assert!(catalog.read(&run, &artifact, 10, 10).unwrap_err().message.contains("cannot be opened"));
  let (_dir, catalog, metadata, _) = registered(b"private text", "text/plain");
  assert!(
    catalog
      .read(&metadata.uri().run_id().to_string(), &metadata.uri().artifact_id().to_string(), 10, 10)
      .unwrap_err()
      .message
      .contains("not available")
  );
  let (_dir, catalog, metadata, _) = registered(b"not an image", "image/png");
  assert!(
    catalog
      .read(&metadata.uri().run_id().to_string(), &metadata.uri().artifact_id().to_string(), 10, 10)
      .unwrap_err()
      .message
      .contains("content type")
  );
}

#[test]
fn image_fetch_limits_source_bytes_and_dimensions_before_decode() {
  let (_dir, catalog, metadata, _) = registered(&png(8193, 1), "image/png");
  assert!(
    catalog
      .read(&metadata.uri().run_id().to_string(), &metadata.uri().artifact_id().to_string(), 10, 10)
      .unwrap_err()
      .message
      .contains("dimension")
  );
  let (_dir, catalog, metadata, _) = registered(&vec![0; MAX_SOURCE_BYTES as usize + 1], "image/png");
  assert!(
    catalog
      .read(&metadata.uri().run_id().to_string(), &metadata.uri().artifact_id().to_string(), 10, 10)
      .unwrap_err()
      .message
      .contains("16 MiB")
  );
}

// https://github.com/moeru-ai/auv/pull/430
#[test]
fn image_fetch_pr430_enforces_output_limit_and_accepts_smaller_request() {
  // Incompressible RGB pixels keep the source below 16 MiB while forcing
  // native-size PNG output past 8 MiB. Exercise the real encoder and limit.
  let mut pixels = image::RgbImage::new(2048, 1536);
  let mut state = 0x1234_5678_u32;
  for byte in pixels.as_mut() {
    state ^= state << 13;
    state ^= state >> 17;
    state ^= state << 5;
    *byte = (state >> 24) as u8;
  }
  let mut bytes = Cursor::new(Vec::new());
  image::DynamicImage::ImageRgb8(pixels).write_to(&mut bytes, ImageFormat::Png).unwrap();
  assert!(bytes.get_ref().len() > MAX_OUTPUT_BYTES);
  assert!(bytes.get_ref().len() as u64 <= MAX_SOURCE_BYTES);
  let (_dir, catalog, metadata, _) = registered(bytes.get_ref(), "image/png");
  let run = metadata.uri().run_id().to_string();
  let artifact = metadata.uri().artifact_id().to_string();
  let error = catalog.read(&run, &artifact, 4096, 4096).unwrap_err();
  assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
  assert!(error.message.contains("encoded image exceeds 8 MiB"));
  let wire = serde_json::to_value(catalog.read(&run, &artifact, 1024, 1024).unwrap()).unwrap();
  let decoded = base64::engine::general_purpose::STANDARD.decode(wire["content"][1]["data"].as_str().unwrap()).unwrap();
  assert!(decoded.len() <= MAX_OUTPUT_BYTES);
  let image = image::load_from_memory(&decoded).unwrap();
  assert_eq!((image.width(), image.height()), (1024, 768));
}

// https://github.com/moeru-ai/auv/pull/430
#[test]
fn image_fetch_pr430_recovers_existing_receipt_after_catalog_poison() {
  let (_dir, catalog, metadata, _) = registered(&png(1, 1), "image/png");
  assert!(
    std::panic::catch_unwind(|| {
      let _guard = catalog.entries.lock().unwrap();
      panic!("interrupt catalog access");
    })
    .is_err()
  );
  let image = catalog.read(&metadata.uri().run_id().to_string(), &metadata.uri().artifact_id().to_string(), 1, 1).unwrap();
  assert_ne!(image.is_error, Some(true));
  assert!(!catalog.entries.is_poisoned());
}

// https://github.com/moeru-ai/auv/pull/430
#[test]
fn image_fetch_pr430_panicking_duplicate_preserves_existing_receipt() {
  // ROOT CAUSE:
  //
  // If a duplicate receipt had an invalid extension, path construction panicked
  // after registration had already removed the original receipt with that URI.
  //
  // Before the fix, lock recovery could not restore the removed receipt.
  // The fix prepares the replacement before changing the catalog.
  let (directory, catalog, metadata, _) = registered(&png(1, 1), "image/png");
  let store = FileTracingStore::open(directory.path()).unwrap();
  let invalid = ArtifactMetadata::new(
    metadata.uri().clone(),
    metadata.purpose().clone(),
    metadata.content_type().clone(),
    Some("invalid/extension".into()),
    metadata.byte_length(),
    metadata.sha256(),
    metadata.attributes().clone(),
  );
  assert!(std::panic::catch_unwind(|| catalog.register(&[invalid], &store)).is_err());
  let result = catalog.read(&metadata.uri().run_id().to_string(), &metadata.uri().artifact_id().to_string(), 1, 1).unwrap();
  let wire = serde_json::to_value(result).unwrap();
  let bytes = base64::engine::general_purpose::STANDARD.decode(wire["content"][1]["data"].as_str().unwrap()).unwrap();
  assert_eq!(image::load_from_memory(&bytes).unwrap().to_rgb8().get_pixel(0, 0).0, [12, 34, 56]);
  assert!(!catalog.entries.is_poisoned());
}

#[test]
fn image_fetch_retains_the_latest_returned_receipts_at_capacity() {
  // ROOT CAUSE:
  //
  // If receipt order differed from UUID order, the newest image could expire
  // because the catalog evicted the smallest UUID.
  //
  // Before the fix, a newly returned image could be unavailable immediately.
  // The fix expires the oldest returned receipt.
  let bytes = png(1, 1);
  let (directory, catalog, initial, _) = registered(&bytes, "image/png");
  let store = Arc::new(FileTracingStore::open(directory.path()).unwrap());
  let run = initial.uri().run_id();
  let mut returned = Vec::new();
  for value in (0..=MAX_SESSION_IMAGES).rev() {
    let artifact: ArtifactId = format!("00000000-0000-4000-8000-{value:012x}").parse().unwrap();
    let metadata = ArtifactMetadata::new(
      ArtifactUri::from_ids(run, artifact),
      "test.image".into(),
      "image/png".into(),
      Some("png".into()),
      ByteLength::new(bytes.len() as u64).unwrap(),
      Sha256Digest::new(Sha256::digest(&bytes).into()),
      Attributes::default(),
    );
    let path = store.artifact_path(&metadata);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, &bytes).unwrap();
    catalog.register(std::slice::from_ref(&metadata), &store);
    returned.push(artifact);
  }
  let run = run.to_string();
  assert!(catalog.read(&run, &returned.last().unwrap().to_string(), 10, 10).is_ok());
  assert!(catalog.read(&run, &returned[1].to_string(), 10, 10).is_ok());
  assert!(catalog.read(&run, &returned[0].to_string(), 10, 10).unwrap_err().message.contains("not available"));
}
