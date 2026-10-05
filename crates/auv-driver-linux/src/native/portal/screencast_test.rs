use super::*;

#[test]
fn stream_maps_global_point_to_local_point() {
  let stream = ScreenCastStream {
    id: 7,
    position: Some((100, 50)),
    size: Some((800, 600)),
    source_type: Some(SourceType::Monitor as u32),
    mapping_id: None,
  };

  let point = stream.local_point(Point::new(120.0, 80.0)).expect("point maps into stream");

  assert_eq!(point, Point::new(20.0, 30.0));
}

#[test]
fn stream_rejects_outside_point() {
  let stream = ScreenCastStream {
    id: 7,
    position: Some((100, 50)),
    size: Some((800, 600)),
    source_type: Some(SourceType::Monitor as u32),
    mapping_id: None,
  };

  assert!(stream.local_point(Point::new(50.0, 80.0)).is_err());
}

#[test]
fn bgrx_pixel_converts_to_rgba() {
  let mut dest = [0, 0, 0, 0];

  write_rgba_pixel(spa::param::video::VideoFormat::BGRx, &[3, 2, 1, 0], &mut dest).expect("BGRx converts");

  assert_eq!(dest, [1, 2, 3, 255]);
}

#[test]
fn xrgb_pixel_converts_to_rgba() {
  let mut dest = [0, 0, 0, 0];

  write_rgba_pixel(spa::param::video::VideoFormat::xRGB, &[0, 1, 2, 3], &mut dest).expect("xRGB converts");

  assert_eq!(dest, [1, 2, 3, 255]);
}

#[test]
fn frame_receiver_pool_reuses_a_connected_receiver_for_repeated_captures() {
  use std::sync::atomic::{AtomicUsize, Ordering};

  struct FakeReceiver;

  impl FrameReceiver for FakeReceiver {
    fn capture_frame(&mut self) -> DriverResult<image::RgbaImage> {
      Ok(image::RgbaImage::new(1, 1))
    }
  }

  // ROOT CAUSE:
  //
  // If callers requested consecutive frames from one portal stream, AUV
  // rebuilt the PipeWire remote, main loop, core, and stream for every frame
  // because no receiver lived beyond `read_pipewire_frame`.
  //
  // Before the fix, two captures implied two complete PipeWire negotiations.
  // The fix keeps one connected receiver per stream until it fails.
  let creations = AtomicUsize::new(0);
  let mut pool = FrameReceiverPool::default();
  for _ in 0..2 {
    pool
      .capture(7, || {
        creations.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(FakeReceiver))
      })
      .expect("frame capture succeeds");
  }

  assert_eq!(creations.load(Ordering::Relaxed), 1);
}

#[test]
fn frame_receiver_pool_recreates_a_receiver_after_capture_failure() {
  struct FailingReceiver;

  impl FrameReceiver for FailingReceiver {
    fn capture_frame(&mut self) -> DriverResult<image::RgbaImage> {
      Err(backend("stream stopped"))
    }
  }

  struct WorkingReceiver;

  impl FrameReceiver for WorkingReceiver {
    fn capture_frame(&mut self) -> DriverResult<image::RgbaImage> {
      Ok(image::RgbaImage::new(1, 1))
    }
  }

  let mut pool = FrameReceiverPool::default();
  assert!(pool.capture(7, || Ok(Box::new(FailingReceiver))).is_err());
  pool.capture(7, || Ok(Box::new(WorkingReceiver))).expect("failed receiver was invalidated");
}

#[test]
fn static_stream_returns_the_latest_frame_after_the_refresh_wait() {
  // ROOT CAUSE:
  //
  // If a Wayland surface had no new damage, PipeWire could legitimately stop
  // delivering buffers. AUV treated the absence of a newer buffer as capture
  // failure even though the connected stream's latest frame was still valid.
  //
  // Before the fix, the second capture waited five seconds and entered the
  // interactive Screenshot fallback. The fix returns the cached frame after a
  // short refresh opportunity.
  let now = Instant::now();
  let (sender, receiver) = mpsc::sync_channel(1);
  let pending = RefCell::new(Some(PendingFrameRequest {
    sender,
    stale_after: Some(now - Duration::from_millis(1)),
  }));
  let expected = Arc::new(image::RgbaImage::new(2, 3));
  let mut cached = LatestFrame::default();
  cached.store_decoded(Arc::clone(&expected));
  let latest = RefCell::new(cached);

  let (sender, image) = take_stale_frame_response(&pending, &latest, now).expect("cached frame is ready");
  sender.send(image).expect("request is still receiving");

  let actual = receiver.recv().expect("response arrives").expect("frame succeeds");
  assert_eq!(actual.dimensions(), expected.dimensions());
  assert!(pending.borrow().is_none());
}

fn bgrx_layout(width: u32, height: u32, offset: u32, stride: i32, available: usize) -> DriverResult<FrameLayout> {
  let mut format = spa::param::video::VideoInfoRaw::new();
  format.set_format(spa::param::video::VideoFormat::BGRx);
  format.set_size(spa::utils::Rectangle { width, height });
  FrameLayout::new(format, offset, stride, available)
}

fn stale_request(now: Instant) -> (RefCell<Option<PendingFrameRequest>>, mpsc::Receiver<WorkerFrameResult>) {
  let (sender, receiver) = mpsc::sync_channel(1);
  let pending = RefCell::new(Some(PendingFrameRequest {
    sender,
    stale_after: Some(now - Duration::from_millis(1)),
  }));
  (pending, receiver)
}

// https://github.com/moeru-ai/auv/issues/244
#[test]
fn stale_fallback_returns_the_newest_idle_frame_issue_244() {
  // ROOT CAUSE:
  //
  // If frames arrived while no capture was waiting, the receiver released them
  // without updating its cached frame, because only waited-for frames were
  // converted. GNOME sends frames only on damage, so once the screen went
  // static the refresh wait expired and the stale fallback returned the frame
  // converted for the first capture.
  //
  // Before the fix, every capture after the first in one driver session
  // returned the same image while the page scrolled between captures.
  // The fix keeps a raw copy of the newest idle frame and converts it on demand.
  let layout = bgrx_layout(1, 1, 0, 4, 4).expect("valid layout");
  let mut latest = LatestFrame::default();
  latest.store_decoded(Arc::new(layout.convert(&[0, 0, 255, 0]).expect("first frame converts")));
  latest.store_raw(&[255, 0, 0, 0], layout);
  let latest = RefCell::new(latest);
  let now = Instant::now();
  let (pending, receiver) = stale_request(now);

  let (sender, image) = take_stale_frame_response(&pending, &latest, now).expect("cached frame is ready");
  sender.send(image).expect("request is still receiving");

  let actual = receiver.recv().expect("response arrives").expect("frame succeeds");
  assert_eq!(actual.get_pixel(0, 0).0, [0, 0, 255, 255], "BGRx blue idle frame, not the red first frame");
}

#[test]
fn raw_idle_frame_copies_only_the_frame_region() {
  // Two 1x2 BGRx rows with an 8-byte offset and 8-byte stride padding.
  let mut buffer = vec![9_u8; 8];
  buffer.extend_from_slice(&[1, 2, 3, 0, 9, 9, 9, 9]);
  buffer.extend_from_slice(&[4, 5, 6, 0]);
  let layout = bgrx_layout(1, 2, 8, 8, buffer.len()).expect("valid layout");
  let mut latest = LatestFrame::default();

  latest.store_raw(&buffer, layout);

  let raw = latest.raw.as_ref().expect("raw frame stored");
  assert_eq!(raw.bytes, [1, 2, 3, 0, 9, 9, 9, 9, 4, 5, 6, 0]);
  let image = latest.image().expect("frame present").expect("frame converts");
  assert_eq!(image.get_pixel(0, 0).0, [3, 2, 1, 255]);
  assert_eq!(image.get_pixel(0, 1).0, [6, 5, 4, 255]);
}

#[test]
fn frame_layout_rejects_a_buffer_shorter_than_the_last_row() {
  let error = bgrx_layout(2, 2, 0, 8, 15).expect_err("last row needs 16 bytes");
  assert!(error.to_string().contains("need 16 bytes, have 15"), "{error}");
}

#[test]
fn stale_fallback_without_a_cached_frame_keeps_waiting() {
  let latest = RefCell::new(LatestFrame::default());
  let now = Instant::now();
  let (pending, _receiver) = stale_request(now);

  assert!(take_stale_frame_response(&pending, &latest, now).is_none());
  assert!(pending.borrow().is_some(), "the request still waits for a real frame");
}
