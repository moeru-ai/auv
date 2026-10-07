use std::time::{Duration, Instant};

use super::*;

/// A capture of `pixels` RGBA pixels (4 bytes each) of incompressible noise;
/// `seed` makes captures distinct.
fn noise(pixels: u32, seed: u32) -> auv_driver::Capture {
  let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
  let image = image::RgbaImage::from_fn(pixels, 1, |_, _| {
    state ^= state << 13;
    state ^= state >> 17;
    state ^= state << 5;
    image::Rgba(state.to_le_bytes())
  });
  capture(image)
}

/// A flat capture of `width`x`height`, which packs to a few bytes.
fn flat(width: u32, height: u32) -> auv_driver::Capture {
  capture(image::RgbaImage::from_pixel(width, height, image::Rgba([30, 60, 90, 255])))
}

fn capture(image: image::RgbaImage) -> auv_driver::Capture {
  auv_driver::Capture {
    origin: None,
    bounds: auv_driver::Rect::new(0.0, 0.0, f64::from(image.width()), f64::from(image.height())),
    image,
    scale_factor: 1.0,
    backend: "test".to_string(),
    fallback_reason: None,
  }
}

fn store(budget_bytes: usize, idle: Duration) -> CaptureStore {
  CaptureStore::new(CaptureStoreOptions {
    budget_bytes,
    idle,
    cold_after: Duration::from_secs(30),
  })
}

fn full() -> RegionKey {
  RegionKey::new(None, None)
}

fn normalized(x: f64, y: f64, width: f64, height: f64) -> auv_api_proto::auv::api::image::v1::NormalizedRect {
  auv_api_proto::auv::api::image::v1::NormalizedRect {
    x,
    y,
    width,
    height,
  }
}

#[test]
fn evicts_the_least_recently_used_capture_when_packing_cannot_fit_the_budget() {
  let store = store(100, Duration::from_secs(60));
  let start = Instant::now();
  let first = store.insert_at(noise(10, 1), start);
  let second = store.insert_at(noise(10, 2), start + Duration::from_secs(1));
  // Reading `first` makes `second` the least recently used.
  assert!(store.get_at(&first, start + Duration::from_secs(2)).is_some());

  let third = store.insert_at(noise(10, 3), start + Duration::from_secs(3));

  assert!(store.get_at(&first, start + Duration::from_secs(4)).is_some());
  assert!(store.get_at(&second, start + Duration::from_secs(4)).is_none());
  assert!(store.get_at(&third, start + Duration::from_secs(4)).is_some());
  assert_eq!(store.total_bytes(), 80);
}

#[test]
fn expires_captures_idle_longer_than_the_expiry() {
  let store = store(1_000, Duration::from_secs(60));
  let start = Instant::now();
  let stale = store.insert_at(noise(10, 1), start);
  let fresh = store.insert_at(noise(10, 2), start + Duration::from_secs(50));

  assert!(store.get_at(&stale, start + Duration::from_secs(61)).is_none());
  assert!(store.get_at(&fresh, start + Duration::from_secs(61)).is_some());
  assert_eq!(store.total_bytes(), 40);
}

#[test]
fn keeps_a_single_capture_larger_than_the_budget_until_the_next_insert() {
  let store = store(16, Duration::from_secs(60));
  let start = Instant::now();
  let large = store.insert_at(noise(10, 1), start);
  assert!(store.get_at(&large, start).is_some());

  let next = store.insert_at(noise(2, 2), start + Duration::from_secs(1));

  assert!(store.get_at(&large, start + Duration::from_secs(1)).is_none());
  assert!(store.get_at(&next, start + Duration::from_secs(1)).is_some());
  assert_eq!(store.total_bytes(), 8);
}

#[test]
fn unknown_references_are_not_found() {
  let store = store(100, Duration::from_secs(60));
  assert!(store.get("cap-0-0").is_none());
}

#[test]
fn identical_captures_share_their_pixels() {
  // Polling an unchanged window stores the same pixels again and again.
  let store = store(1_000, Duration::from_secs(60));
  let start = Instant::now();
  let first = store.insert_at(noise(10, 7), start);
  let second = store.insert_at(noise(10, 7), start);
  assert_ne!(first, second, "each capture keeps its own reference");
  assert_eq!(store.total_bytes(), 40, "the pixels are stored once");

  let mut different_metadata = noise(10, 7);
  different_metadata.backend = "other".to_string();
  store.insert_at(different_metadata, start);
  assert_eq!(store.total_bytes(), 80, "metadata is part of the identity");

  // Expiring one reference keeps the shared pixels for the other.
  store.get_at(&second, start + Duration::from_secs(50));
  assert!(store.get_at(&first, start + Duration::from_secs(70)).is_none());
  assert_eq!(store.get_at(&second, start + Duration::from_secs(70)).expect("still held").image, noise(10, 7).image);
}

#[test]
fn idle_pixels_are_packed_and_unpacked_unchanged() {
  let store = store(10_000_000, Duration::from_secs(600));
  let start = Instant::now();
  let id = store.insert_at(flat(200, 100), start);
  assert_eq!(store.total_bytes(), 80_000);

  store.sweep_at(start + Duration::from_secs(10));
  assert!(!store.is_packed(&id), "recently used pixels stay hot");

  store.sweep_at(start + Duration::from_secs(31));
  assert!(store.is_packed(&id));
  assert!(store.total_bytes() < 1_000, "a flat capture packs to a few bytes: {}", store.total_bytes());

  let unpacked = store.get_at(&id, start + Duration::from_secs(32)).expect("held");
  assert_eq!(unpacked.image, flat(200, 100).image);
  assert_eq!(unpacked.bounds, flat(200, 100).bounds);
  assert!(!store.is_packed(&id));
  assert_eq!(store.total_bytes(), 80_000);
}

#[test]
fn over_budget_captures_are_packed_before_any_is_evicted() {
  let store = store(100_000, Duration::from_secs(600));
  let start = Instant::now();
  let ids: Vec<_> = (0..4)
    .map(|index| {
      let mut capture = flat(100, 100);
      capture.backend = format!("distinct-{index}");
      store.insert_at(capture, start + Duration::from_secs(index))
    })
    .collect();

  // Four 40 KB captures exceed 100 KB; packing fits them all.
  for id in &ids {
    assert!(store.get_at(id, start + Duration::from_secs(10)).is_some(), "{id} was evicted");
  }
}

#[test]
fn derived_caches_are_dropped_before_pixels_are_packed() {
  let store = store(50_000, Duration::from_secs(600));
  let start = Instant::now();
  let id = store.insert_at(flat(100, 100), start);
  let key = ImageKey::new(full(), Some((10, 10)), 3);
  store.remember_image(
    &id,
    key,
    EncodedImage {
      encoding: 3,
      width: 10,
      height: 10,
      data: vec![0; 5_000],
    },
  );
  assert!(store.image(&id, &key).is_some());

  // 40 KB pixels + 5 KB image + 8 KB new capture exceed 50 KB; dropping the image fits.
  let other = store.insert_at(noise(2_000, 9), start + Duration::from_secs(1));

  assert!(store.image(&id, &key).is_none(), "the cached image went first");
  assert!(!store.is_packed(&id), "the pixels did not need packing");
  assert!(store.get(&other).is_some());
}

#[test]
fn recognition_results_are_cached_per_region_and_options() {
  let store = store(1_000_000, Duration::from_secs(600));
  let first = store.insert(noise(10, 3));
  let duplicate = store.insert(noise(10, 3));
  let words = vec!["AUV".to_string()];
  let key = RecognitionKey::new(full(), &words, &[]);
  let recognition = auv_driver::TextRecognition {
    origin: None,
    text: "hello".to_string(),
    regions: Vec::new(),
  };

  assert!(store.recognition(&first, &key).is_none());
  store.remember_recognition(&first, key.clone(), recognition.clone());

  assert_eq!(store.recognition(&first, &key).as_deref(), Some(&recognition));
  assert_eq!(store.recognition(&duplicate, &key).as_deref(), Some(&recognition), "identical pixels share results");
  let other_region = RecognitionKey::new(RegionKey::new(Some(&normalized(0.0, 0.0, 0.5, 1.0)), None), &words, &[]);
  assert!(store.recognition(&first, &other_region).is_none());
  let screen = auv_api_proto::auv::api::driver::v1::ScreenRect {
    x: 0.0,
    y: 0.0,
    width: 0.5,
    height: 1.0,
  };
  let same_numbers_in_screen_space = RecognitionKey::new(RegionKey::new(None, Some(&screen)), &words, &[]);
  assert!(store.recognition(&first, &same_numbers_in_screen_space).is_none(), "screen and normalized regions never share keys");
  assert!(store.recognition(&first, &RecognitionKey::new(full(), &[], &[])).is_none());
}
