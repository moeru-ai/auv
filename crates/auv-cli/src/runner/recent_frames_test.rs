use std::sync::{Arc, Mutex};

use auv_api_proto::auv::api::driver::v1 as proto;
use auv_api_proto::auv::api::driver::v1::recent_frames_service_server::RecentFramesService as _;
use tonic::Request;

use super::*;

#[derive(Clone)]
struct FakeFactory {
  opened: Arc<Mutex<Vec<CaptureTarget>>>,
  size: (u32, u32),
}

impl FrameSourceFactory for FakeFactory {
  fn open(&self, target: CaptureTarget) -> Result<Box<dyn FrameSource>, Status> {
    self.opened.lock().expect("opened mutex").push(target);
    Ok(Box::new(FakeSource { size: self.size }))
  }
}

struct FakeSource {
  size: (u32, u32),
}

impl FrameSource for FakeSource {
  fn capture(&mut self) -> Result<auv_driver::Capture, auv_driver::DriverError> {
    Ok(capture(self.size, 1))
  }
}

fn capture(size: (u32, u32), value: u8) -> auv_driver::Capture {
  auv_driver::Capture {
    origin: None,
    image: image::RgbaImage::from_pixel(size.0, size.1, image::Rgba([value, 2, 3, 255])),
    bounds: auv_driver::Rect::new(10.0, 20.0, f64::from(size.0), f64::from(size.1)),
    scale_factor: 1.0,
    backend: "fake".to_string(),
    fallback_reason: None,
  }
}

#[test]
fn history_retains_only_the_newest_native_rgba_frames() {
  let mut history = History::new(2);
  history.push(capture((3, 2), 1));
  history.push(capture((3, 2), 2));
  history.push(capture((3, 2), 3));

  let response = history.recent(1).expect("read recent frames");
  assert_eq!(response.frames.iter().map(|frame| frame.sequence).collect::<Vec<_>>(), [2, 3]);
  assert_eq!(response.dropped_frames, 1);
  let image = response.frames[1].frame.as_ref().and_then(|frame| frame.image.as_ref()).expect("RGBA frame");
  assert_eq!((image.width, image.height, image.data.len()), (3, 2, 24));
}

#[tokio::test]
async fn service_applies_caller_size_to_a_generic_region_target() {
  let opened = Arc::new(Mutex::new(Vec::new()));
  let service = Service::with_factory(Arc::new(FakeFactory {
    opened: Arc::clone(&opened),
    size: (4, 3),
  }));
  let response = service
    .open_frame_buffer(Request::new(proto::OpenFrameBufferRequest {
      target: Some(proto::CaptureTarget {
        target: Some(proto::capture_target::Target::Region(proto::RegionCaptureTarget {
          region: Some(proto::ScreenRect {
            x: 1.0,
            y: 2.0,
            width: 4.0,
            height: 3.0,
          }),
          selector: None,
        })),
      }),
      target_fps: 30,
      frame_capacity: 2,
      output_size: Some(auv_api_proto::auv::api::image::v1::PixelSize {
        width: 2,
        height: 1,
      }),
    }))
    .await
    .expect("open frame buffer")
    .into_inner();
  let reference = response.frame_buffer.expect("frame buffer reference");
  let batch = service
    .get_recent_frames(Request::new(proto::GetRecentFramesRequest {
      frame_buffer: Some(reference.clone()),
      after_sequence: 0,
    }))
    .await
    .expect("get frame")
    .into_inner();

  let image = batch.frames[0].frame.as_ref().and_then(|frame| frame.image.as_ref()).expect("RGBA frame");
  assert_eq!((image.width, image.height, image.data.len()), (2, 1, 8));
  assert_eq!(
    opened.lock().expect("opened targets").as_slice(),
    &[CaptureTarget::Region {
      region: auv_driver::Rect::new(1.0, 2.0, 4.0, 3.0),
      display: None,
    }]
  );

  service
    .close_frame_buffer(Request::new(proto::CloseFrameBufferRequest {
      frame_buffer: Some(reference),
    }))
    .await
    .expect("close frame buffer");
}

#[derive(Clone, Copy)]
enum Scripted {
  Frame(u8),
  StaleWindow,
  BackendFailure,
}

/// Replays scripted capture results, then repeats the last one.
struct ScriptedSource {
  results: std::collections::VecDeque<Scripted>,
}

impl FrameSource for ScriptedSource {
  fn capture(&mut self) -> Result<auv_driver::Capture, auv_driver::DriverError> {
    let result = if self.results.len() > 1 {
      self.results.pop_front()
    } else {
      self.results.front().copied()
    };
    match result.expect("scripted result") {
      Scripted::Frame(value) => Ok(capture((2, 2), value)),
      Scripted::StaleWindow => Err(auv_driver::DriverError::StaleUiReference {
        message: "window 1 was 955x558 pt at capture time but 1644x960 pt to its application".to_string(),
        recovery: None,
      }),
      Scripted::BackendFailure => Err(auv_driver::DriverError::Backend {
        message: "capture backend failed".to_string(),
      }),
    }
  }
}

/// Runs the producer until `done` holds for the history, then stops it.
fn run_until(results: Vec<Scripted>, done: impl Fn(&History) -> bool) -> Arc<Mutex<History>> {
  let history = Arc::new(Mutex::new(History::new(4)));
  let (stop, stop_requested) = std::sync::mpsc::channel();
  let producer_history = Arc::clone(&history);
  let mut source = ScriptedSource {
    results: results.into(),
  };
  let producer = std::thread::spawn(move || run_producer(&mut source, 120, None, producer_history, stop_requested));
  let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
  while !done(&history.lock().expect("history")) {
    assert!(std::time::Instant::now() < deadline, "producer did not reach the expected state");
    std::thread::sleep(std::time::Duration::from_millis(5));
  }
  let _ = stop.send(());
  producer.join().expect("producer thread");
  history
}

// ROOT CAUSE:
//
// If a window target was shown by Mission Control, its capture failed once and
// the producer recorded a fault and stopped for good.
//
// The fix pauses the buffer on stale-window captures, reports the pause, and
// keeps capturing.
#[test]
fn producer_pauses_on_a_stale_window_and_reports_it() {
  let history = run_until(vec![Scripted::StaleWindow], |history| history.pause.as_ref().is_some_and(|pause| pause.skipped_captures >= 2));

  let response = history.lock().expect("history").recent(0).expect("a paused buffer still answers");
  let pause = response.pause.expect("pause is reported");
  assert!(pause.reason.contains("955x558"));
  assert!(pause.skipped_captures >= 2);
  assert!(pause.since.is_some());
  assert!(response.frames.is_empty());
}

#[test]
fn producer_resumes_and_clears_the_pause_after_a_stale_window() {
  let history = run_until(
    vec![
      Scripted::StaleWindow,
      Scripted::StaleWindow,
      Scripted::Frame(9),
    ],
    |history| history.latest_sequence >= 1,
  );

  let response = history.lock().expect("history").recent(0).expect("read recent frames");
  assert!(response.pause.is_none());
  let image = response.frames[0].frame.as_ref().and_then(|frame| frame.image.as_ref()).expect("RGBA frame");
  assert_eq!(image.data[0], 9);
}

#[test]
fn producer_stops_on_other_capture_failures() {
  let history = run_until(vec![Scripted::BackendFailure], |history| history.fault.is_some());

  let error = history.lock().expect("history").recent(0).expect_err("a stopped buffer with no frames reports its fault");
  assert!(error.message().contains("capture backend failed"));
}
