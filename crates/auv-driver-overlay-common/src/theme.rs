use serde::{Deserialize, Deserializer, Serialize};

use crate::{
  Layer, Overlay,
  layers::CursorImage,
  style::{Color, Shadow},
};

/// Partial host appearance overrides, independent of a UI framework or renderer.
///
/// Unset fields retain each layer's existing appearance. Colors accept hex strings
/// or normalized RGBA objects when deserialized. Geometry, timing, labels and
/// visibility are never changed. Apply after composing capture/click components.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverlayTheme {
  /// Border color for all outline layers, including capture frames.
  #[serde(
    default,
    skip_serializing_if = "Option::is_none",
    deserialize_with = "optional_color"
  )]
  pub outline_color: Option<Color>,
  /// Foreground of the optional cursor label.
  #[serde(
    default,
    skip_serializing_if = "Option::is_none",
    deserialize_with = "optional_color"
  )]
  pub cursor_label_foreground: Option<Color>,
  /// Background of the cursor label; also colors the Windows built-in sprite.
  #[serde(
    default,
    skip_serializing_if = "Option::is_none",
    deserialize_with = "optional_color"
  )]
  pub cursor_label_background: Option<Color>,
  /// Replacement art for cursor layers. SVG support depends on the adapter.
  /// Built-in artwork has a fixed palette; supply themed SVG art to recolor it.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub cursor_image: Option<CursorImage>,
  /// Native silhouette shadow; independent of SVG art and sprite size.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub cursor_shadow: Option<Shadow>,
  /// Status pill text color.
  #[serde(
    default,
    skip_serializing_if = "Option::is_none",
    deserialize_with = "optional_color"
  )]
  pub status_foreground: Option<Color>,
  /// Status pill background color, including its opacity.
  #[serde(
    default,
    skip_serializing_if = "Option::is_none",
    deserialize_with = "optional_color"
  )]
  pub status_background: Option<Color>,
}

impl OverlayTheme {
  /// Validates host input before any native rendering takes place.
  pub fn validate(&self) -> Result<(), String> {
    if let Some(shadow) = self.cursor_shadow {
      shadow.validate()?;
    }
    for (name, color) in [
      ("outline_color", self.outline_color),
      ("cursor_label_foreground", self.cursor_label_foreground),
      ("cursor_label_background", self.cursor_label_background),
      ("status_foreground", self.status_foreground),
      ("status_background", self.status_background),
    ] {
      if let Some(color) = color
        && ![color.red, color.green, color.blue, color.alpha]
          .into_iter()
          .all(|channel| channel.is_finite() && (0.0..=1.0).contains(&channel))
      {
        return Err(format!("{name} channels must be finite and between 0 and 1"));
      }
    }
    if let Some(CursorImage::Svg { source }) = &self.cursor_image
      && (source.trim().is_empty() || source.len() > 256 * 1024)
    {
      // Match the existing overlay.cursor CLI payload bound before crossing FFI.
      return Err("cursor_image SVG must be non-empty and at most 256 KiB".to_string());
    }
    Ok(())
  }

  /// Produces themed layers without mutating the caller's overlay.
  ///
  /// Use for typed composition or before serializing an overlay to a remote
  /// Runner. Configured fields override layer styles; unspecified fields retain
  /// their current values. Invalid colors or oversized SVG return an error.
  pub fn apply(&self, overlay: &Overlay) -> Result<Overlay, String> {
    self.validate()?;
    let mut themed = Overlay::new();

    for layer in overlay.layers() {
      let layer = match layer {
        Layer::Outline(outline) => {
          let mut style = outline.style();
          if let Some(color) = self.outline_color {
            style.stroke.color = color;
          }

          Layer::Outline(outline.clone().with_style(style))
        }
        Layer::Cursor(cursor) => {
          let mut style = cursor.style();
          if let Some(shadow) = self.cursor_shadow {
            style.shadow = Some(shadow);
          }
          if let Some(color) = self.cursor_label_foreground {
            style.label_foreground = color;
          }
          if let Some(color) = self.cursor_label_background {
            style.label_background = color;
          }

          let mut cursor = cursor.clone().with_style(style);
          if let Some(image) = &self.cursor_image {
            cursor = cursor.with_image(image.clone());
          }

          Layer::Cursor(cursor)
        }
        Layer::Status(status) => {
          let mut style = status.style();
          if let Some(color) = self.status_foreground {
            style.foreground = color;
          }
          if let Some(color) = self.status_background {
            style.background = color;
          }

          Layer::Status(status.clone().with_style(style))
        }
      };
      themed = themed.with_layer(layer);
    }

    Ok(themed)
  }
}

/// Accepts existing typed RGBA values as well as host-friendly hex colors.
///
/// NOTICE(serde-json-arbitrary-precision): this dispatches on the input type with
/// a visitor instead of `#[serde(untagged)]`. An untagged enum buffers its input,
/// and with `serde_json/arbitrary_precision` (enabled by `auv-tracing` in every
/// shipped binary) buffered numbers no longer deserialize as `f64`, so RGBA
/// objects in `AUV_OVERLAY_THEME` were rejected.
fn optional_color<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Color>, D::Error> {
  struct ColorInput;

  impl<'de> serde::de::Visitor<'de> for ColorInput {
    type Value = Option<Color>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
      formatter.write_str("a hex color string or an RGBA object")
    }

    fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
      Ok(None)
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
      Ok(None)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
      deserializer.deserialize_any(self)
    }

    fn visit_str<E: serde::de::Error>(self, hex: &str) -> Result<Self::Value, E> {
      hex.parse().map(Some).map_err(E::custom)
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
      Color::deserialize(serde::de::value::MapAccessDeserializer::new(map)).map(Some)
    }
  }

  deserializer.deserialize_option(ColorInput)
}

#[cfg(test)]
#[path = "theme_test.rs"]
mod tests;
