use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Color {
  pub red: f64,
  pub green: f64,
  pub blue: f64,
  pub alpha: f64,
}

impl Color {
  pub const WHITE: Self = Self::rgb(1.0, 1.0, 1.0);
  pub const CLEAR: Self = Self::rgba(0.0, 0.0, 0.0, 0.0);
  pub const AUV_CYAN: Self = Self::rgb(0.0, 0.608, 0.651);
  pub const AUV_LIME: Self = Self::rgb(0.498, 0.816, 0.188);
  pub const YOU_SLATE: Self = Self::rgb(0.164, 0.227, 0.322);

  pub const fn rgb(red: f64, green: f64, blue: f64) -> Self {
    Self::rgba(red, green, blue, 1.0)
  }

  pub const fn rgba(red: f64, green: f64, blue: f64, alpha: f64) -> Self {
    Self {
      red,
      green,
      blue,
      alpha,
    }
  }

  pub const fn with_alpha(self, alpha: f64) -> Self {
    Self { alpha, ..self }
  }
}

/// Parses CLI and host theme colors without slicing unvalidated UTF-8.
impl std::str::FromStr for Color {
  type Err = String;

  fn from_str(raw: &str) -> Result<Self, Self::Err> {
    let hex = raw.strip_prefix('#').unwrap_or(raw);
    if !matches!(hex.len(), 6 | 8) || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
      return Err("expected #RRGGBB or #RRGGBBAA".to_string());
    }
    let channel = |offset| u8::from_str_radix(&hex[offset..offset + 2], 16).expect("validated hex") as f64 / 255.0;
    Ok(Self::rgba(channel(0), channel(2), channel(4), if hex.len() == 8 { channel(6) } else { 1.0 }))
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Insets {
  pub top: f64,
  pub right: f64,
  pub bottom: f64,
  pub left: f64,
}

impl Insets {
  pub const fn all(value: f64) -> Self {
    Self {
      top: value,
      right: value,
      bottom: value,
      left: value,
    }
  }

  pub const fn symmetric(vertical: f64, horizontal: f64) -> Self {
    Self {
      top: vertical,
      right: horizontal,
      bottom: vertical,
      left: horizontal,
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
  pub color: Color,
  pub width: f64,
}

impl Stroke {
  pub const fn new(color: Color, width: f64) -> Self {
    Self { color, width }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct OutlineStyle {
  pub stroke: Stroke,
  pub padding: Insets,
  pub corner_radius: f64,
}

impl OutlineStyle {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn selected() -> Self {
    Self {
      stroke: Stroke::new(Color::AUV_LIME, 3.0),
      ..Self::default()
    }
  }

  pub fn capture() -> Self {
    Self {
      stroke: Stroke::new(Color::WHITE.with_alpha(0.95), 4.0),
      padding: Insets::default(),
      corner_radius: 10.0,
    }
  }

  pub fn with_stroke(mut self, stroke: Stroke) -> Self {
    self.stroke = stroke;
    self
  }

  pub fn with_padding(mut self, padding: Insets) -> Self {
    self.padding = padding;
    self
  }

  pub fn with_corner_radius(mut self, corner_radius: f64) -> Self {
    self.corner_radius = corner_radius;
    self
  }
}

impl Default for OutlineStyle {
  fn default() -> Self {
    Self {
      stroke: Stroke::new(Color::AUV_CYAN, 3.0),
      padding: Insets::default(),
      corner_radius: 8.0,
    }
  }
}

/// Native shadow applied to the composited cursor silhouette, excluding its label.
/// All dimensions are logical display points, independent of SVG viewBox scaling.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shadow {
  /// Normalized RGBA shadow color, including opacity.
  pub color: Color,
  /// Finite non-negative native blur radius in logical points.
  pub blur_radius: f64,
  /// Horizontal displacement; positive moves the shadow right.
  pub offset_x: f64,
  /// Vertical displacement; positive moves the shadow down.
  pub offset_y: f64,
}

impl Shadow {
  /// Pale-mint native glow used by the macOS built-in AUV cursors.
  pub fn auv() -> Self {
    Self {
      color: Color::rgba(206.0 / 255.0, 1.0, 253.0 / 255.0, 0.55),
      blur_radius: 8.0,
      offset_x: 0.0,
      offset_y: 2.0,
    }
  }

  /// Rejects invalid native drawing dimensions and color channels before FFI.
  pub fn validate(self) -> Result<(), String> {
    if !self.blur_radius.is_finite() || self.blur_radius < 0.0 || !self.offset_x.is_finite() || !self.offset_y.is_finite() {
      return Err("shadow requires finite offsets and a finite non-negative blur radius".to_string());
    }
    if ![
      self.color.red,
      self.color.green,
      self.color.blue,
      self.color.alpha,
    ]
    .into_iter()
    .all(|value| value.is_finite() && (0.0..=1.0).contains(&value))
    {
      return Err("shadow color channels must be finite and between 0 and 1".to_string());
    }
    Ok(())
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CursorStyle {
  pub label_foreground: Color,
  pub label_background: Color,
  pub label_padding: Insets,
  pub label_corner_radius: f64,
  pub sprite_size: f64,
  pub label_gap: f64,
  /// Optional native silhouette shadow override, currently supported on macOS.
  /// None uses renderer defaults: built-in AUV cursors glow; custom SVGs do not.
  /// An explicit transparent shadow disables a built-in's default glow.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub shadow: Option<Shadow>,
}

impl CursorStyle {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn auv() -> Self {
    Self::default()
  }

  pub fn auv_click() -> Self {
    Self {
      label_background: Color::AUV_CYAN,
      ..Self::default()
    }
  }

  pub fn you() -> Self {
    Self {
      label_background: Color::YOU_SLATE,
      ..Self::default()
    }
  }

  pub fn with_label_foreground(mut self, color: Color) -> Self {
    self.label_foreground = color;
    self
  }

  pub fn with_label_background(mut self, color: Color) -> Self {
    self.label_background = color;
    self
  }

  pub fn with_label_padding(mut self, padding: Insets) -> Self {
    self.label_padding = padding;
    self
  }

  pub fn with_label_corner_radius(mut self, radius: f64) -> Self {
    self.label_corner_radius = radius;
    self
  }

  pub fn with_sprite_size(mut self, size: f64) -> Self {
    self.sprite_size = size;
    self
  }

  /// Sets or clears an explicit shadow override without changing sprite size.
  /// Use a transparent shadow to suppress a built-in renderer's default glow.
  pub fn with_shadow(mut self, shadow: Option<Shadow>) -> Self {
    self.shadow = shadow;
    self
  }

  pub fn with_label_gap(mut self, gap: f64) -> Self {
    self.label_gap = gap;
    self
  }
}

impl Default for CursorStyle {
  fn default() -> Self {
    Self {
      label_foreground: Color::WHITE,
      label_background: Color::AUV_CYAN,
      label_padding: Insets::symmetric(3.0, 8.0),
      label_corner_radius: 999.0,
      sprite_size: 24.0,
      label_gap: 6.0,
      shadow: None,
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StatusStyle {
  pub foreground: Color,
  pub background: Color,
  pub padding: Insets,
  pub corner_radius: f64,
}

impl StatusStyle {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn action() -> Self {
    Self::default()
  }

  pub fn with_foreground(mut self, color: Color) -> Self {
    self.foreground = color;
    self
  }

  pub fn with_background(mut self, color: Color) -> Self {
    self.background = color;
    self
  }

  pub fn with_padding(mut self, padding: Insets) -> Self {
    self.padding = padding;
    self
  }

  pub fn with_corner_radius(mut self, radius: f64) -> Self {
    self.corner_radius = radius;
    self
  }
}

impl Default for StatusStyle {
  fn default() -> Self {
    Self {
      foreground: Color::WHITE,
      background: Color::AUV_CYAN.with_alpha(0.88),
      padding: Insets::symmetric(7.0, 12.0),
      corner_radius: 9.0,
    }
  }
}
