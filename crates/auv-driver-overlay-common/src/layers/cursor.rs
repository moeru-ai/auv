use auv_driver_common::ScreenPoint;
use serde::{Deserialize, Serialize};

use crate::style::CursorStyle;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cursor {
  point: ScreenPoint,
  label: Option<String>,
  label_visible: bool,
  image: CursorImage,
  style: CursorStyle,
  // NOTICE: only live overlays pose the cursor (`MotionScene`), and the Runner wire format
  // does not carry a pose (`overlay_to_proto` in `crates/auv/src/client/runner.rs`),
  // so one-shot overlays always draw the rest pose. See TODO(overlay-motion-event-wire).
  #[serde(default, skip_serializing_if = "CursorPose::is_rest")]
  pose: CursorPose,
}

impl Cursor {
  pub fn new(point: ScreenPoint) -> Self {
    Self {
      point,
      label: None,
      label_visible: false,
      image: CursorImage::default(),
      style: CursorStyle::default(),
      pose: CursorPose::REST,
    }
  }

  pub fn with_label(mut self, label: impl Into<String>) -> Self {
    self.label = Some(label.into());
    self
  }

  pub fn with_label_visible(mut self) -> Self {
    self.label_visible = true;
    self
  }

  pub fn with_image(mut self, image: CursorImage) -> Self {
    self.image = image;
    self
  }

  pub fn with_style(mut self, style: CursorStyle) -> Self {
    self.style = style;
    self
  }

  pub fn with_pose(mut self, pose: CursorPose) -> Self {
    self.pose = pose;
    self
  }

  pub fn point(&self) -> ScreenPoint {
    self.point
  }

  pub fn label(&self) -> Option<&str> {
    self.label.as_deref()
  }

  pub fn label_visible(&self) -> bool {
    self.label_visible
  }

  pub fn image(&self) -> &CursorImage {
    &self.image
  }

  pub fn style(&self) -> CursorStyle {
    self.style
  }

  pub fn pose(&self) -> CursorPose {
    self.pose
  }
}

/// How cursor art is turned and sized about its hotspot, the point the cursor acts on.
/// Posing about the hotspot keeps that point exactly where the operation acted.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CursorPose {
  /// Clockwise rotation on screen, in degrees.
  pub tilt_degrees: f64,
  /// Uniform scale; 1 draws the art at its sprite size.
  pub scale: f64,
}

impl CursorPose {
  pub const REST: Self = Self {
    tilt_degrees: 0.0,
    scale: 1.0,
  };

  pub fn is_rest(&self) -> bool {
    *self == Self::REST
  }

  /// Rejects poses a renderer cannot draw before they reach native code.
  pub fn validate(self) -> Result<(), String> {
    if !self.tilt_degrees.is_finite() || !self.scale.is_finite() || self.scale <= 0.0 {
      return Err("cursor pose requires a finite tilt and a finite positive scale".to_string());
    }
    Ok(())
  }
}

impl Default for CursorPose {
  fn default() -> Self {
    Self::REST
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CursorImage {
  BuiltIn { variant: BuiltInCursor },
  Svg { source: String },
}

impl CursorImage {
  pub fn built_in(variant: BuiltInCursor) -> Self {
    Self::BuiltIn { variant }
  }

  pub fn svg(source: impl Into<String>) -> Self {
    Self::Svg {
      source: source.into(),
    }
  }
}

impl Default for CursorImage {
  fn default() -> Self {
    Self::built_in(BuiltInCursor::Auv)
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuiltInCursor {
  #[default]
  Auv,
  AuvClick,
  You,
}

impl BuiltInCursor {
  /// Returns canonical vector artwork for the compact AUV cursor variants.
  /// Native adapters can render this at the configured sprite size. The user
  /// cursor retains its existing platform artwork and returns no SVG source.
  pub fn svg_source(self) -> Option<&'static str> {
    match self {
      Self::Auv => Some(include_str!("../../assets/cursor-auv.svg")),
      Self::AuvClick => Some(include_str!("../../assets/cursor-auv-click.svg")),
      Self::You => None,
    }
  }

  pub fn as_str(self) -> &'static str {
    match self {
      Self::Auv => "auv",
      Self::AuvClick => "auv-click",
      Self::You => "you",
    }
  }
}
