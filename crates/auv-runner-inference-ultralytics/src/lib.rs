//! Ultralytics inference service hosted in one daemon-supervised Runner.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use auv_api_proto::auv::api::image::v1 as image_proto;
use auv_api_proto::auv::api::inference::v1 as proto;
use auv_api_proto::auv::api::inference::v1::object_detection_service_server::{ObjectDetectionService, ObjectDetectionServiceServer};
use auv_inference_common::{ImageFrame, ModelId};
use auv_inference_ultralytics::InferenceDevice;
use auv_task_object_detection::{DetectionOptions, DetectionResult, UltralyticsObjectDetector, UltralyticsObjectDetectorConfig};
use tonic::{Request, Response, Status};

#[derive(Debug, Default)]
struct Service {
  detectors: Arc<LazyResourceCache<DetectorKey, UltralyticsObjectDetector>>,
}

#[tonic::async_trait]
impl ObjectDetectionService for Service {
  async fn detect_objects(&self, request: Request<proto::DetectObjectsRequest>) -> Result<Response<proto::DetectObjectsResponse>, Status> {
    let request = request.into_inner();
    let detector = request.detector.ok_or_else(|| Status::invalid_argument("detector is required"))?;
    let frame = request.frame.ok_or_else(|| Status::invalid_argument("frame is required"))?;
    let detectors = Arc::clone(&self.detectors);
    tokio::task::spawn_blocking(move || detect_objects(&detectors, detector, frame))
      .await
      .map_err(|error| Status::internal(format!("object-detection task failed: {error}")))?
      .map(Response::new)
  }
}

#[derive(Debug)]
struct LazyResourceCache<K, V> {
  cells: Mutex<HashMap<K, ResourceCell<V>>>,
}

type ResourceCell<V> = Arc<OnceLock<Result<Arc<V>, Arc<str>>>>;

impl<K, V> Default for LazyResourceCache<K, V> {
  fn default() -> Self {
    Self {
      cells: Mutex::new(HashMap::new()),
    }
  }
}

impl<K: Clone + Eq + Hash, V> LazyResourceCache<K, V> {
  fn get_or_try_init(&self, key: K, load: impl FnOnce(&K) -> Result<V, Arc<str>>) -> Result<Arc<V>, Arc<str>> {
    let cell = {
      let mut cells = self.cells.lock().expect("lazy resource cache mutex poisoned");
      Arc::clone(cells.entry(key.clone()).or_insert_with(|| Arc::new(OnceLock::new())))
    };
    cell.get_or_init(|| load(&key).map(Arc::new)).clone()
  }
}

#[derive(Clone, Debug, Eq)]
struct DetectorKey {
  detector_id: String,
  model_path: PathBuf,
  input_size: Option<u32>,
  confidence_bits: u32,
  iou_bits: u32,
  max_detections: usize,
  device: DeviceKey,
  class_names: Vec<String>,
}

impl PartialEq for DetectorKey {
  fn eq(&self, other: &Self) -> bool {
    self.detector_id == other.detector_id
      && self.model_path == other.model_path
      && self.input_size == other.input_size
      && self.confidence_bits == other.confidence_bits
      && self.iou_bits == other.iou_bits
      && self.max_detections == other.max_detections
      && self.device == other.device
      && self.class_names == other.class_names
  }
}

impl Hash for DetectorKey {
  fn hash<H: Hasher>(&self, state: &mut H) {
    self.detector_id.hash(state);
    self.model_path.hash(state);
    self.input_size.hash(state);
    self.confidence_bits.hash(state);
    self.iou_bits.hash(state);
    self.max_detections.hash(state);
    self.device.hash(state);
    self.class_names.hash(state);
  }
}

impl DetectorKey {
  fn from_proto(spec: proto::ObjectDetectorSpec) -> Result<Self, Status> {
    if spec.detector_id.trim().is_empty() {
      return Err(Status::invalid_argument("detector.detector_id must not be empty"));
    }
    let model_path = PathBuf::from(spec.model_path);
    if !model_path.is_absolute() {
      return Err(Status::invalid_argument("detector.model_path must be absolute"));
    }
    let defaults = DetectionOptions::default();
    let max_detections = usize::try_from(spec.max_detections.unwrap_or(defaults.max_detections as u32))
      .map_err(|_| Status::invalid_argument("detector.max_detections is too large"))?;
    if max_detections == 0 {
      return Err(Status::invalid_argument("detector.max_detections must be positive"));
    }
    let confidence = spec.confidence_threshold.unwrap_or(defaults.confidence_threshold);
    let iou = spec.iou_threshold.unwrap_or(defaults.iou_threshold);
    if !(0.0..=1.0).contains(&confidence) || !(0.0..=1.0).contains(&iou) {
      return Err(Status::invalid_argument("detector thresholds must be within 0..=1"));
    }
    Ok(Self {
      detector_id: spec.detector_id,
      model_path,
      input_size: spec.input_size,
      confidence_bits: confidence.to_bits(),
      iou_bits: iou.to_bits(),
      max_detections,
      device: DeviceKey::from_proto(spec.device)?,
      class_names: spec.class_names,
    })
  }

  fn config(&self) -> UltralyticsObjectDetectorConfig {
    UltralyticsObjectDetectorConfig {
      model_id: ModelId(self.detector_id.clone()),
      model_path: self.model_path.clone(),
      input_size: self.input_size,
      options: DetectionOptions {
        confidence_threshold: f32::from_bits(self.confidence_bits),
        iou_threshold: f32::from_bits(self.iou_bits),
        max_detections: self.max_detections,
      },
      device: self.device.to_device(),
      class_names_override: (!self.class_names.is_empty()).then(|| self.class_names.clone()),
    }
  }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum DeviceKey {
  Cpu,
  Cuda(usize),
  CoreMl,
  DirectMl(usize),
  OpenVino,
  Xnnpack,
  TensorRt(usize),
  Rocm(usize),
}

impl DeviceKey {
  fn from_proto(device: Option<proto::InferenceDevice>) -> Result<Self, Status> {
    let device = device.unwrap_or_default();
    let kind = proto::InferenceDeviceKind::try_from(device.kind).map_err(|_| Status::invalid_argument("detector.device.kind is unknown"))?;
    let indexed =
      |name: &str| device.index.map(|value| value as usize).ok_or_else(|| Status::invalid_argument(format!("{name} requires device.index")));
    let unindexed = |value, name: &str| {
      if device.index.is_some() {
        Err(Status::invalid_argument(format!("{name} must not set device.index")))
      } else {
        Ok(value)
      }
    };
    match kind {
      proto::InferenceDeviceKind::Unspecified | proto::InferenceDeviceKind::Cpu => unindexed(Self::Cpu, "CPU"),
      proto::InferenceDeviceKind::Cuda => indexed("CUDA").map(Self::Cuda),
      proto::InferenceDeviceKind::CoreMl => unindexed(Self::CoreMl, "CoreML"),
      proto::InferenceDeviceKind::DirectMl => indexed("DirectML").map(Self::DirectMl),
      proto::InferenceDeviceKind::OpenVino => unindexed(Self::OpenVino, "OpenVINO"),
      proto::InferenceDeviceKind::Xnnpack => unindexed(Self::Xnnpack, "XNNPACK"),
      proto::InferenceDeviceKind::TensorRt => indexed("TensorRT").map(Self::TensorRt),
      proto::InferenceDeviceKind::Rocm => indexed("ROCm").map(Self::Rocm),
    }
  }

  fn to_device(self) -> InferenceDevice {
    match self {
      Self::Cpu => InferenceDevice::Cpu,
      Self::Cuda(index) => InferenceDevice::Cuda(index),
      Self::CoreMl => InferenceDevice::CoreMl,
      Self::DirectMl(index) => InferenceDevice::DirectMl(index),
      Self::OpenVino => InferenceDevice::OpenVino,
      Self::Xnnpack => InferenceDevice::Xnnpack,
      Self::TensorRt(index) => InferenceDevice::TensorRt(index),
      Self::Rocm(index) => InferenceDevice::Rocm(index),
    }
  }
}

fn detect_objects(
  detectors: &LazyResourceCache<DetectorKey, UltralyticsObjectDetector>,
  detector: proto::ObjectDetectorSpec,
  frame: image_proto::RgbFrame,
) -> Result<proto::DetectObjectsResponse, Status> {
  let expected_len = usize::try_from(frame.width)
    .ok()
    .and_then(|width| usize::try_from(frame.height).ok().and_then(|height| width.checked_mul(height)))
    .and_then(|pixels| pixels.checked_mul(3))
    .ok_or_else(|| Status::invalid_argument("frame dimensions overflow"))?;
  if frame.data.len() != expected_len {
    return Err(Status::invalid_argument(format!(
      "frame.data has {} bytes; expected {expected_len} for {}x{} RGB8",
      frame.data.len(),
      frame.width,
      frame.height
    )));
  }
  let image = image::RgbImage::from_raw(frame.width, frame.height, frame.data)
    .ok_or_else(|| Status::invalid_argument("frame.data is not a valid tightly packed RGB8 image"))?;
  let key = DetectorKey::from_proto(detector)?;
  let detector = detectors
    .get_or_try_init(key.clone(), |key| UltralyticsObjectDetector::load(key.config()).map_err(|error| Arc::<str>::from(error.to_string())))
    .map_err(|error| Status::failed_precondition(format!("failed to load detector {}: {error}", key.detector_id)))?;
  let result =
    detector.detect_frame(&ImageFrame::new(image)).map_err(|error| Status::internal(format!("object detection failed: {error}")))?;
  Ok(result_to_proto(result))
}

fn result_to_proto(result: DetectionResult) -> proto::DetectObjectsResponse {
  proto::DetectObjectsResponse {
    image_size: Some(proto::ImageSize {
      width: result.image_size.width,
      height: result.image_size.height,
    }),
    detections: result
      .detections
      .into_iter()
      .map(|detection| proto::Detection {
        class_id: u32::try_from(detection.class_id).unwrap_or(u32::MAX),
        label: detection.label,
        confidence: detection.confidence,
        bounding_box: Some(proto::BoundingBox {
          x1: detection.bbox.x1,
          y1: detection.bbox.y1,
          x2: detection.bbox.x2,
          y2: detection.bbox.y2,
        }),
      })
      .collect(),
  }
}

#[cfg(unix)]
pub async fn serve_inherited() -> Result<(), String> {
  let (incoming, parent_disconnected) = auv_runner_protocol::inherited_transport()?.into_parts();
  let runtime = auv_runner_protocol::RuntimeControl::ready(auv_runner_protocol::RuntimeMetadata {
    runner_class: "auv.inference.ultralytics".to_string(),
    display_name: "AUV Ultralytics inference".to_string(),
    labels: Default::default(),
    operation_capacity: 1,
  })?;
  let runtime_service = runtime.service();
  let service = ObjectDetectionServiceServer::new(Service::default())
    .max_decoding_message_size(auv_api_proto::MAX_CAPTURE_GRPC_MESSAGE_BYTES)
    .max_encoding_message_size(auv_api_proto::MAX_CAPTURE_GRPC_MESSAGE_BYTES);
  let (health_reporter, health) = tonic_health::server::health_reporter();
  health_reporter.set_serving::<ObjectDetectionServiceServer<Service>>().await;
  health_reporter
    .set_serving::<auv_api_proto::auv::api::runner::v1::runner_runtime_service_server::RunnerRuntimeServiceServer<
      auv_runner_protocol::RuntimeControl,
    >>()
    .await;
  let descriptor = auv_runner_protocol::RuntimeControl::descriptor_set_for_services(&["auv.api.inference.v1.ObjectDetectionService"])?;
  let reflection = auv_runner_protocol::reflection_service(&descriptor)?;
  tonic::transport::Server::builder()
    .add_service(health)
    .add_service(reflection)
    .add_service(runtime_service)
    .add_service(runtime.track(service))
    .serve_with_incoming_shutdown(incoming, parent_disconnected)
    .await
    .map_err(|error| format!("inference Runner transport failed: {error}"))
}

#[cfg(not(unix))]
pub async fn serve_inherited() -> Result<(), String> {
  // TODO(inference-runner-windows-ipc): enable after auv-runner-protocol grows
  // the daemon-owned inherited named-pipe transport.
  Err("the inference Runner currently requires Unix inherited IPC".to_string())
}

#[cfg(test)]
mod tests {
  use std::sync::atomic::{AtomicUsize, Ordering};

  use super::*;

  #[test]
  fn rejects_malformed_rgb_frame_before_model_loading() {
    let error = detect_objects(
      &LazyResourceCache::default(),
      proto::ObjectDetectorSpec {
        detector_id: "test".to_string(),
        model_path: "/missing/model.onnx".to_string(),
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
  fn lazy_resource_cache_initializes_once_per_key() {
    let cache = LazyResourceCache::<String, usize>::default();
    let loads = AtomicUsize::new(0);
    let first = cache.get_or_try_init("detector".to_string(), |_| Ok(loads.fetch_add(1, Ordering::SeqCst))).expect("first load");
    let second = cache.get_or_try_init("detector".to_string(), |_| Ok(loads.fetch_add(1, Ordering::SeqCst))).expect("cached load");
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(loads.load(Ordering::SeqCst), 1);
  }
}
