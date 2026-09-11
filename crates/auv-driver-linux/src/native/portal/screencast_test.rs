use super::*;

#[test]
fn stream_maps_global_point_to_local_point() {
  let stream = ScreenCastStream {
    id: 7,
    position: Some((100, 50)),
    size: Some((800, 600)),
    source_type: Some(SOURCE_MONITOR),
    mapping_id: None,
    pipewire_serial: None,
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
    source_type: Some(SOURCE_MONITOR),
    mapping_id: None,
    pipewire_serial: None,
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
  let latest = RefCell::new(Some(Arc::clone(&expected)));

  let (sender, image) = take_stale_frame_response(&pending, &latest, now).expect("cached frame is ready");
  sender.send(Ok(image)).expect("request is still receiving");

  let actual = receiver.recv().expect("response arrives").expect("frame succeeds");
  assert_eq!(actual.dimensions(), expected.dimensions());
  assert!(pending.borrow().is_none());
}
