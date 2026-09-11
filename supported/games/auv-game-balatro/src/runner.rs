//! Balatro-owned object-detection Runner.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use auv_api_proto::auv::api::image::v1 as image_proto;
use auv_inference_common::{ImageFrame, ModelId};
use auv_inference_ultralytics::InferenceDevice;
use auv_task_object_detection::{DetectionOptions, UltralyticsObjectDetector, UltralyticsObjectDetectorConfig};
use hf_hub::HFClientSync;
use tonic::{Request, Response, Status};

use crate::api::v1 as proto;
use crate::api::v1::balatro_detection_service_server::{BalatroDetectionService, BalatroDetectionServiceServer};

#[derive(Debug, Default)]
struct Service {
  detectors: Arc<LazyResourceCache<DetectorKey, UltralyticsObjectDetector>>,
}

#[tonic::async_trait]
impl BalatroDetectionService for Service {
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

  async fn detect_objects_batch(
    &self,
    request: Request<proto::DetectObjectsBatchRequest>,
  ) -> Result<Response<proto::DetectObjectsBatchResponse>, Status> {
    let request = request.into_inner();
    let frame = request.frame.ok_or_else(|| Status::invalid_argument("frame is required"))?;
    let detectors = Arc::clone(&self.detectors);
    tokio::task::spawn_blocking(move || detect_objects_batch(&detectors, request.detectors, frame))
      .await
      .map_err(|error| Status::internal(format!("object-detection batch task failed: {error}")))?
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
    let model_path = resolve_model_source(spec.source)?;
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
}

fn resolve_model_source(source: Option<proto::object_detector_spec::Source>) -> Result<PathBuf, Status> {
  match source.ok_or_else(|| Status::invalid_argument("detector.source is required"))? {
    proto::object_detector_spec::Source::RunnerPath(path) if !path.trim().is_empty() => Ok(PathBuf::from(path)),
    proto::object_detector_spec::Source::RunnerPath(_) => Err(Status::invalid_argument("detector.runner_path must not be empty")),
    proto::object_detector_spec::Source::HuggingFace(asset) => {
      if asset.owner.trim().is_empty() || asset.repository.trim().is_empty() || asset.filename.trim().is_empty() {
        return Err(Status::invalid_argument("detector.hugging_face owner, repository, and filename are required"));
      }
      let kind = proto::hugging_face_asset::RepositoryKind::try_from(asset.repository_kind)
        .map_err(|_| Status::invalid_argument("detector.hugging_face.repository_kind is unknown"))?;
      let client =
        HFClientSync::new().map_err(|error| Status::failed_precondition(format!("failed to initialize Hugging Face client: {error}")))?;
      match kind {
        proto::hugging_face_asset::RepositoryKind::Model => {
          client.model(asset.owner, asset.repository).download_file().filename(asset.filename).send()
        }
        proto::hugging_face_asset::RepositoryKind::Dataset => {
          client.dataset(asset.owner, asset.repository).download_file().filename(asset.filename).send()
        }
        proto::hugging_face_asset::RepositoryKind::Unspecified => {
          return Err(Status::invalid_argument("detector.hugging_face.repository_kind is required"));
        }
      }
      .map_err(|error| Status::failed_precondition(format!("failed to resolve Runner model asset: {error}")))
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
}

fn detect_objects(
  detectors: &LazyResourceCache<DetectorKey, UltralyticsObjectDetector>,
  detector: proto::ObjectDetectorSpec,
  frame: image_proto::RgbFrame,
) -> Result<proto::DetectObjectsResponse, Status> {
  let frame = image_frame_from_proto(frame)?;
  detect_frame(detectors, detector, &frame)
}

fn detect_objects_batch(
  detectors: &LazyResourceCache<DetectorKey, UltralyticsObjectDetector>,
  specs: Vec<proto::ObjectDetectorSpec>,
  frame: image_proto::RgbFrame,
) -> Result<proto::DetectObjectsBatchResponse, Status> {
  if specs.is_empty() {
    return Err(Status::invalid_argument("detectors must not be empty"));
  }
  let frame = image_frame_from_proto(frame)?;
  let results = std::thread::scope(|scope| {
    let frame = &frame;
    specs
      .into_iter()
      .map(|spec| {
        let detector_id = spec.detector_id.clone();
        scope.spawn(move || {
          detect_frame(detectors, spec, frame).map(|result| proto::DetectObjectsBatchResult {
            detector_id,
            result: Some(result),
          })
        })
      })
      .collect::<Vec<_>>()
      .into_iter()
      .map(|thread| thread.join().map_err(|_| Status::internal("object detector thread panicked"))?)
      .collect::<Result<Vec<_>, Status>>()
  })?;
  Ok(proto::DetectObjectsBatchResponse { results })
}

fn image_frame_from_proto(frame: image_proto::RgbFrame) -> Result<ImageFrame, Status> {
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
  Ok(ImageFrame::new(image))
}

fn detect_frame(
  detectors: &LazyResourceCache<DetectorKey, UltralyticsObjectDetector>,
  detector: proto::ObjectDetectorSpec,
  frame: &ImageFrame,
) -> Result<proto::DetectObjectsResponse, Status> {
  let key = DetectorKey::from_proto(detector)?;
  let detector = detectors
    .get_or_try_init(key.clone(), |key| {
      let device = match key.device {
        DeviceKey::Cpu => InferenceDevice::Cpu,
        DeviceKey::Cuda(index) => {
          #[cfg(feature = "cuda")]
          {
            InferenceDevice::Cuda(index)
          }
          #[cfg(not(feature = "cuda"))]
          {
            return Err(Arc::<str>::from(format!(
              "CUDA device {index} was requested, but the Balatro Runner was built without its `cuda` feature"
            )));
          }
        }
        DeviceKey::CoreMl => InferenceDevice::CoreMl,
        DeviceKey::DirectMl(index) => InferenceDevice::DirectMl(index),
        DeviceKey::OpenVino => InferenceDevice::OpenVino,
        DeviceKey::Xnnpack => InferenceDevice::Xnnpack,
        DeviceKey::TensorRt(index) => InferenceDevice::TensorRt(index),
        DeviceKey::Rocm(index) => InferenceDevice::Rocm(index),
      };
      UltralyticsObjectDetector::load(UltralyticsObjectDetectorConfig {
        model_id: ModelId(key.detector_id.clone()),
        model_path: key.model_path.clone(),
        input_size: key.input_size,
        options: DetectionOptions {
          confidence_threshold: f32::from_bits(key.confidence_bits),
          iou_threshold: f32::from_bits(key.iou_bits),
          max_detections: key.max_detections,
        },
        device,
        class_names_override: (!key.class_names.is_empty()).then(|| key.class_names.clone()),
      })
      .map_err(|error| Arc::<str>::from(error.to_string()))
    })
    .map_err(|error| Status::failed_precondition(format!("failed to load detector {}: {error}", key.detector_id)))?;
  let result = detector.detect_frame(frame).map_err(|error| Status::internal(format!("object detection failed: {error}")))?;
  Ok(proto::DetectObjectsResponse {
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
  })
}

#[cfg(unix)]
pub async fn serve_inherited() -> Result<(), String> {
  let (incoming, parent_disconnected) = auv_api_server::runner_transport::inherited_transport()?.into_parts();
  let service = BalatroDetectionServiceServer::new(Service::default())
    .max_decoding_message_size(auv_api_proto::GRPC_MESSAGE_SIZE_UNLIMITED)
    .max_encoding_message_size(auv_api_proto::GRPC_MESSAGE_SIZE_UNLIMITED);
  let (health_reporter, health) = tonic_health::server::health_reporter();
  health_reporter.set_serving::<BalatroDetectionServiceServer<Service>>().await;
  let descriptor = crate::api::FILE_DESCRIPTOR_SET;
  let reflection =
    auv_api_server::reflection::service(descriptor).map_err(|error| format!("failed to build Balatro Runner reflection: {error}"))?;
  tonic::transport::Server::builder()
    .add_service(health)
    .add_service(reflection)
    .add_service(service)
    .serve_with_incoming_shutdown(incoming, parent_disconnected)
    .await
    .map_err(|error| format!("Balatro Runner transport failed: {error}"))
}

#[cfg(not(unix))]
pub async fn serve_inherited() -> Result<(), String> {
  // TODO(balatro-runner-windows-ipc): enable after the API server transport
  // supports daemon-owned inherited named-pipe handles.
  Err("the Balatro Runner currently requires Unix inherited IPC".to_string())
}

#[cfg(test)]
#[path = "runner_test.rs"]
mod tests;
