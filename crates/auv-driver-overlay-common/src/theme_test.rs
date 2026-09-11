use super::*;
use crate::{
  components::{CaptureFrame, ClickTarget},
  layers::{Cursor, Status},
  style::OutlineStyle,
};
use auv_driver_common::{Rect, ScreenPoint};

#[test]
fn empty_theme_preserves_custom_layers_exactly() {
  let overlay = Overlay::new()
    .with_layer(
      CaptureFrame::new(Rect::new(1.0, 2.0, 30.0, 40.0)).with_label("capture").with_label_visible().with_style(OutlineStyle::selected()),
    )
    .with_layer(
      Cursor::new(ScreenPoint::new(8.0, 9.0))
        .with_label("host")
        .with_label_visible()
        .with_image(CursorImage::svg("<svg viewBox='0 0 8 8'/>")),
    )
    .with_layer(Status::new(ScreenPoint::new(10.0, 11.0), "ready"));
  assert_eq!(OverlayTheme::default().apply(&overlay).unwrap(), overlay);
}

#[test]
fn host_theme_reaches_capture_and_click_components_without_changing_geometry() {
  let overlay = Overlay::new().with_layer(CaptureFrame::new(Rect::new(1.0, 2.0, 100.0, 80.0)).with_label("screen")).with_layer(
    ClickTarget::new(ScreenPoint::new(50.0, 40.0))
      .with_outline(Rect::new(10.0, 20.0, 30.0, 15.0))
      .with_cursor_label("host")
      .with_cursor_label_visible()
      .with_status("delivered"),
  );
  let theme: OverlayTheme = serde_json::from_str(
    r##"{
    "outline_color":"#336699", "cursor_label_background":"#336699",
    "cursor_label_foreground":"#ffffff", "status_background":"#11223380",
    "status_foreground":"#eeeeee", "cursor_image":{"kind":"svg","source":"<svg viewBox='0 0 8 8'/>"}
  }"##,
  )
  .unwrap();
  let themed = theme.apply(&overlay).unwrap();
  let [
    Layer::Outline(capture),
    Layer::Outline(target),
    Layer::Cursor(cursor),
    Layer::Status(status),
  ] = themed.layers()
  else {
    panic!("expected ordered capture and click layers");
  };
  assert_eq!(capture.style().stroke.color, theme.outline_color.unwrap());
  assert_eq!(target.style().stroke.color, theme.outline_color.unwrap());
  assert_eq!(capture.style().stroke.width, OutlineStyle::capture().stroke.width);
  assert_eq!(target.style().stroke.width, OutlineStyle::selected().stroke.width);
  assert_eq!(capture.rect(), Rect::new(1.0, 2.0, 100.0, 80.0));
  assert_eq!(capture.label(), Some("screen"));
  assert!(!capture.label_visible());
  assert_eq!(cursor.point(), ScreenPoint::new(50.0, 40.0));
  assert_eq!(cursor.label(), Some("host"));
  assert!(cursor.label_visible());
  assert_eq!(cursor.style().label_background, theme.cursor_label_background.unwrap());
  assert_eq!(cursor.style().label_foreground, theme.cursor_label_foreground.unwrap());
  assert_eq!(cursor.image(), theme.cursor_image.as_ref().unwrap());
  assert_eq!(status.text(), "delivered");
  assert_eq!(status.style().background, theme.status_background.unwrap());
  assert_eq!(status.style().foreground, theme.status_foreground.unwrap());
  assert_eq!(theme.apply(&themed).unwrap(), themed);
  assert_ne!(overlay, themed);
}

#[test]
fn partial_theme_preserves_unset_colors_and_cursor_art() {
  let overlay = Overlay::new().with_layer(Cursor::new(ScreenPoint::new(3.0, 4.0)));
  let theme = OverlayTheme {
    cursor_label_background: Some(Color::WHITE),
    ..Default::default()
  };
  let themed = theme.apply(&overlay).unwrap();
  let (Layer::Cursor(before), Layer::Cursor(after)) = (&overlay.layers()[0], &themed.layers()[0]) else {
    panic!("cursor");
  };
  assert_eq!(after.image(), before.image());
  assert_eq!(
    after.style(),
    crate::style::CursorStyle {
      label_background: Color::WHITE,
      ..before.style()
    }
  );
}

#[test]
fn hex_colors_reject_invalid_utf8_boundaries_without_panicking() {
  // Hex parsing previously sliced arbitrary UTF-8 at byte offsets in the CLI.
  // Shared theme/CLI parsing validates ASCII before decoding channel pairs.
  for value in ["a中aa", "#中中", "#fffffg", "#fff", "#123456789", ""] {
    assert!(value.parse::<Color>().is_err(), "{value}");
  }
  assert_eq!("ff008080".parse::<Color>().unwrap(), Color::rgba(1.0, 0.0, 128.0 / 255.0, 128.0 / 255.0));
}

#[test]
fn typed_colors_round_trip_and_invalid_channels_fail_before_rendering() {
  let theme = OverlayTheme {
    outline_color: Some(Color::WHITE),
    ..Default::default()
  };
  assert_eq!(serde_json::from_str::<OverlayTheme>(&serde_json::to_string(&theme).unwrap()).unwrap(), theme);
  for red in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
    let theme = OverlayTheme {
      outline_color: Some(Color::rgb(red, 0.0, 0.0)),
      ..Default::default()
    };
    assert!(theme.apply(&Overlay::new()).is_err());
  }
}

#[test]
fn host_svg_payloads_are_bounded_before_native_decoding() {
  for source in [String::new(), " ".into(), "x".repeat(256 * 1024 + 1)] {
    let theme = OverlayTheme {
      cursor_image: Some(CursorImage::svg(source)),
      ..Default::default()
    };
    assert!(theme.validate().is_err());
  }
}

#[test]
fn native_cursor_shadow_preserves_sprite_size_and_svg_content() {
  let source = "<svg viewBox='0 0 24 24'><path d='M2 2v20l8-8h10Z'/></svg>";
  let overlay = Overlay::new().with_layer(Cursor::new(ScreenPoint::new(10.0, 20.0)).with_image(CursorImage::svg(source)));
  let theme: OverlayTheme = serde_json::from_str(
    r#"{"cursor_shadow":{"color":{"red":1,"green":0.6,"blue":0.15,"alpha":0.65},"blur_radius":8,"offset_x":0,"offset_y":2}}"#,
  )
  .unwrap();
  let themed = theme.apply(&overlay).unwrap();
  let Layer::Cursor(cursor) = &themed.layers()[0] else {
    panic!("cursor");
  };
  assert_eq!(cursor.style().sprite_size, 24.0);
  assert_eq!(cursor.image(), &CursorImage::svg(source));
  assert_eq!(cursor.style().shadow, theme.cursor_shadow);
  assert_eq!(cursor.point(), ScreenPoint::new(10.0, 20.0));
  assert_eq!(serde_json::from_str::<Overlay>(&serde_json::to_string(&themed).unwrap()).unwrap(), themed);
}

#[test]
fn native_cursor_shadow_rejects_invalid_native_drawing_parameters() {
  let shadow = crate::style::Shadow {
    color: Color::WHITE,
    blur_radius: 8.0,
    offset_x: 0.0,
    offset_y: 2.0,
  };
  for invalid in [
    crate::style::Shadow {
      blur_radius: -1.0,
      ..shadow
    },
    crate::style::Shadow {
      offset_x: f64::NAN,
      ..shadow
    },
    crate::style::Shadow {
      offset_y: f64::INFINITY,
      ..shadow
    },
    crate::style::Shadow {
      color: Color::rgb(2.0, 0.0, 0.0),
      ..shadow
    },
  ] {
    assert!(
      OverlayTheme {
        cursor_shadow: Some(invalid),
        ..Default::default()
      }
      .apply(&Overlay::new())
      .is_err()
    );
  }
}
