use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use super::*;

#[test]
fn rejects_malformed_rgb_frame_before_model_loading() {
  let error = detect_objects(
    &LazyResourceCache::default(),
    proto::ObjectDetectorSpec {
      detector_id: "test".to_string(),
      source: Some(proto::object_detector_spec::Source::RunnerPath("/missing/model.onnx".to_string())),
      ..Default::default()
    },
    image_proto::RgbFrame {
      width: 2,
      height: 2,
      data: vec![0; 11],
    },
  )
  .expect_err("invalid frame");
  assert_eq!(error.code(), tonic::Code::InvalidArgument);
  assert!(error.message().contains("expected 12"));
}

#[test]
fn rejects_empty_detector_batch_before_frame_decoding() {
  let error = detect_objects_batch(
    &LazyResourceCache::default(),
    Vec::new(),
    image_proto::RgbFrame {
      width: 0,
      height: 0,
      data: Vec::new(),
    },
  )
  .expect_err("empty detector batch");

  assert_eq!(error.code(), tonic::Code::InvalidArgument);
  assert!(error.message().contains("detectors must not be empty"));
}

#[test]
fn lazy_resource_cache_initializes_once_per_key() {
  let cache = LazyResourceCache::<String, usize>::default();
  let loads = AtomicUsize::new(0);
  let first = cache.get_or_try_init("detector".to_string(), |_| Ok(loads.fetch_add(1, Ordering::SeqCst))).expect("first load");
  let second = cache.get_or_try_init("detector".to_string(), |_| Ok(loads.fetch_add(1, Ordering::SeqCst))).expect("cached load");
  assert!(Arc::ptr_eq(&first, &second));
  assert_eq!(loads.load(Ordering::SeqCst), 1);
}

#[test]
fn lazy_resource_cache_loads_distinct_models_concurrently() {
  // ROOT CAUSE:
  //
  // If model construction held the cache map lock, the four independent card
  // attribute sessions would load serially before inference could overlap.
  // The cache must hold that lock only while resolving each per-key OnceLock.
  let cache = Arc::new(LazyResourceCache::<String, usize>::default());
  let (entered_tx, entered_rx) = mpsc::channel();
  let (release_tx, release_rx) = mpsc::channel();
  let release_rx = Arc::new(Mutex::new(release_rx));
  let mut threads = Vec::new();
  for key in ["identity", "edition"] {
    let cache = Arc::clone(&cache);
    let entered_tx = entered_tx.clone();
    let release_rx = Arc::clone(&release_rx);
    threads.push(std::thread::spawn(move || {
      cache
        .get_or_try_init(key.to_string(), |_| {
          entered_tx.send(key).expect("announce model load");
          release_rx.lock().expect("release receiver mutex").recv().expect("release model load");
          Ok(1)
        })
        .expect("model load")
    }));
  }

  let first = entered_rx.recv_timeout(Duration::from_secs(1)).expect("first model entered loader");
  let second = entered_rx.recv_timeout(Duration::from_secs(1)).expect("second model entered loader without waiting for first");
  assert_ne!(first, second);
  release_tx.send(()).expect("release first model");
  release_tx.send(()).expect("release second model");
  for thread in threads {
    thread.join().expect("loader thread");
  }
}
