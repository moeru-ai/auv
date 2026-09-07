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
}

impl Cursor {
  pub fn new(point: ScreenPoint) -> Self {
    Self {
      point,
      label: None,
      label_visible: false,
      image: CursorImage::default(),
      style: CursorStyle::default(),
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
