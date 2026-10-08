//! Captures held by the Runner and the explicit calls that read their pixels.
//!
//! Capture, find-text and scroll-until results carry a [`RunnerCapture`]:
//! a reference plus metadata. Pixels stay in the Runner until
//! [`CapturesClient::image`] or [`CapturesClient::pixels`] asks for them
//! (see "Image Payloads" in `AGENTS.md`).

use auv_api_proto::auv::api::driver::v1 as proto;
use auv_api_proto::auv::api::image::v1 as image_proto;

use super::{CapabilityError, IMAGE_RPC_MESSAGE_SIZE_LIMIT, ImageRegion, RunnerClient, capability_status, position_from_proto, required};

/// Reference to a capture held in the capture store of the Runner that
/// produced it. It is a Runner resource: valid only on a Run-affine route to
/// that Runner, and `NotFound` once evicted or expired.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CaptureRef(String);

impl CaptureRef {
  /// Wraps a capture ID returned by the Runner.
  pub fn new(id: impl Into<String>) -> Self {
    Self(id.into())
  }

  /// The Runner-assigned capture ID.
  pub fn id(&self) -> &str {
    &self.0
  }
}

/// A capture's reference and metadata. Its pixels stay in the Runner.
#[derive(Clone, Debug, PartialEq)]
pub struct RunnerCapture {
  /// Reference for OCR, image fetches and evidence.
  pub reference: CaptureRef,
  /// Logical screen-space bounds represented by the pixels.
  pub bounds: auv_driver::Rect,
  /// Image top-left in its owning coordinate space, when bound.
  pub origin: Option<auv_driver::Position>,
  pub scale_factor: f64,
  /// Physical pixel size.
  pub pixel_size: auv_driver::PixelSize,
  pub backend: String,
  pub fallback_reason: Option<String>,
  /// ThumbHash of the whole capture for a blurred preview; empty from a
  /// Runner that predates it.
  pub thumbhash: Vec<u8>,
}

impl auv_driver::Positional for RunnerCapture {
  fn position(&self) -> auv_driver::DriverResult<auv_driver::Position> {
    self.origin.clone().ok_or_else(|| auv_driver::DriverError::InvalidInput {
      message: "capture has no bound coordinate origin".into(),
    })
  }
}

/// What OCR reads: a capture this Runner holds (no pixels travel) or a
/// caller-owned image (pixels are sent).
#[derive(Clone, Debug)]
pub enum RecognitionSource {
  Capture(CaptureRef),
  Image(auv_driver::Capture),
}

impl From<CaptureRef> for RecognitionSource {
  fn from(reference: CaptureRef) -> Self {
    Self::Capture(reference)
  }
}

impl From<&RunnerCapture> for RecognitionSource {
  fn from(capture: &RunnerCapture) -> Self {
    Self::Capture(capture.reference.clone())
  }
}

impl From<auv_driver::Capture> for RecognitionSource {
  fn from(image: auv_driver::Capture) -> Self {
    Self::Image(image)
  }
}

/// Encoding of fetched capture pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaptureImageEncoding {
  /// Tightly packed RGBA8 rows.
  #[default]
  Rgba,
  Png,
  /// Fixed quality 85.
  Jpeg,
  /// Lossless WebP.
  Webp,
}

/// How [`CapturesClient::image`] shapes the pixels it returns.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CaptureImageOptions {
  /// Crop to this part of the capture first.
  pub region: Option<ImageRegion>,
  /// Fit inside this pixel size, keeping the aspect ratio; never enlarges.
  pub max_size: Option<auv_driver::PixelSize>,
  pub encoding: CaptureImageEncoding,
}

/// Pixels fetched from the Runner: raw RGBA8 rows for
/// [`CaptureImageEncoding::Rgba`], otherwise container bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptureImage {
  pub encoding: CaptureImageEncoding,
  /// Size after cropping and fitting.
  pub size: auv_driver::PixelSize,
  pub data: Vec<u8>,
}

impl CaptureImage {
  /// The pixels as an `RgbaImage`; only for [`CaptureImageEncoding::Rgba`].
  pub fn into_rgba_image(self) -> Result<image::RgbaImage, CapabilityError> {
    if self.encoding != CaptureImageEncoding::Rgba {
      return Err(CapabilityError::InvalidResponse(format!("capture image is {:?}, not RGBA", self.encoding)));
    }
    image::RgbaImage::from_raw(self.size.width, self.size.height, self.data)
      .ok_or_else(|| CapabilityError::InvalidResponse("capture image contains malformed RGBA8 data".to_string()))
  }
}

/// Explicit pixel access for captures held by one routed Runner.
#[derive(Clone, Debug)]
pub struct CapturesClient {
  pub(super) runner: RunnerClient,
}

impl CapturesClient {
  /// Fetches a capture's pixels, cropped, bounded and encoded as requested.
  pub async fn image(&self, capture: &CaptureRef, options: CaptureImageOptions) -> Result<CaptureImage, CapabilityError> {
    let response = proto::capture_service_client::CaptureServiceClient::new(self.runner.transport()?)
      .max_decoding_message_size(IMAGE_RPC_MESSAGE_SIZE_LIMIT)
      .get_capture_image(proto::GetCaptureImageRequest {
        capture: Some(proto::CaptureRef {
          capture_id: capture.id().to_string(),
        }),
        region: super::relative_region(options.region),
        screen_region: super::screen_region(options.region),
        max_size: options.max_size.map(|size| image_proto::PixelSize {
          width: size.width,
          height: size.height,
        }),
        encoding: encoding_to_proto(options.encoding) as i32,
      })
      .await
      .map_err(capability_status)?
      .into_inner();
    let image = required(response.image, "GetCaptureImage response omitted its image")?;
    Ok(CaptureImage {
      encoding: encoding_from_proto(image.encoding)?,
      size: auv_driver::PixelSize::new(image.width, image.height),
      data: image.data,
    })
  }

  /// Fetches a capture's full-resolution pixels as a driver `Capture`, for
  /// code that processes pixels locally (for example artifact writers).
  pub async fn pixels(&self, capture: &RunnerCapture) -> Result<auv_driver::Capture, CapabilityError> {
    let image = self.image(&capture.reference, CaptureImageOptions::default()).await?.into_rgba_image()?;
    Ok(auv_driver::Capture {
      origin: capture.origin.clone(),
      image,
      bounds: capture.bounds,
      scale_factor: capture.scale_factor,
      backend: capture.backend.clone(),
      fallback_reason: capture.fallback_reason.clone(),
    })
  }
}

/// A Runner capture's reference and metadata; the frame must carry a reference.
pub(super) fn runner_capture_from_proto(capture: proto::CapturedFrame) -> Result<RunnerCapture, CapabilityError> {
  let reference = required(capture.r#ref, "CapturedFrame omitted its capture reference")?;
  if reference.capture_id.trim().is_empty() {
    return Err(CapabilityError::InvalidResponse("CapturedFrame has an empty capture reference".into()));
  }
  let bounds = required(capture.bounds, "CapturedFrame omitted its screen bounds")?;
  let size = required(capture.pixel_size, "CapturedFrame omitted its pixel size")?;
  Ok(RunnerCapture {
    reference: CaptureRef::new(reference.capture_id),
    bounds: auv_driver::Rect::new(bounds.x, bounds.y, bounds.width, bounds.height),
    origin: capture.origin.map(position_from_proto).transpose()?,
    scale_factor: capture.scale_factor,
    pixel_size: auv_driver::PixelSize::new(size.width, size.height),
    backend: capture.backend,
    fallback_reason: capture.fallback_reason,
    thumbhash: capture.thumbhash,
  })
}

fn encoding_to_proto(encoding: CaptureImageEncoding) -> image_proto::ImageEncoding {
  match encoding {
    CaptureImageEncoding::Rgba => image_proto::ImageEncoding::Rgba,
    CaptureImageEncoding::Png => image_proto::ImageEncoding::Png,
    CaptureImageEncoding::Jpeg => image_proto::ImageEncoding::Jpeg,
    CaptureImageEncoding::Webp => image_proto::ImageEncoding::Webp,
  }
}

fn encoding_from_proto(value: i32) -> Result<CaptureImageEncoding, CapabilityError> {
  match image_proto::ImageEncoding::try_from(value) {
    Ok(image_proto::ImageEncoding::Rgba) => Ok(CaptureImageEncoding::Rgba),
    Ok(image_proto::ImageEncoding::Png) => Ok(CaptureImageEncoding::Png),
    Ok(image_proto::ImageEncoding::Jpeg) => Ok(CaptureImageEncoding::Jpeg),
    Ok(image_proto::ImageEncoding::Webp) => Ok(CaptureImageEncoding::Webp),
    _ => Err(CapabilityError::InvalidResponse("capture image has an unknown encoding".into())),
  }
}
