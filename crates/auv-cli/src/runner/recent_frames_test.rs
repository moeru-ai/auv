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
  fn capture(&mut self) -> Result<auv_driver::Capture, String> {
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
  let image = response.frames[1].capture.as_ref().and_then(|capture| capture.image.as_ref()).expect("RGBA frame");
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

  let image = batch.frames[0].capture.as_ref().and_then(|capture| capture.image.as_ref()).expect("RGBA frame");
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
