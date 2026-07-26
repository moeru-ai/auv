use super::*;

#[test]
fn derive_bbox_projects_point_and_radius() {
  let bbox = derive_bbox(98.0, 69.0, 1512, 949, (2.4713542, 2.4713542, 123.333336, 0.0, 79.083336)).expect("bbox");

  assert!((bbox.x1 - 286.4377).abs() < 0.1);
  assert!((bbox.y1 - 91.4311).abs() < 0.1);
  assert!((bbox.x2 - 444.6044).abs() < 0.1);
  assert!((bbox.y2 - 249.5978).abs() < 0.1);
}

#[test]
fn derive_bbox_rejects_out_of_bounds_boxes() {
  let error = derive_bbox(0.0, 0.0, 100, 100, (1.0, 1.0, 0.0, 0.0, 60.0)).expect_err("bbox should fail");
  assert!(error.contains("exceeds image bounds"));
}

#[test]
fn visibility_rule_skips_late_after_dispatch_frames() {
  assert!(frame_is_visible(&CapturePhase::BeforeDispatch, 999));
  assert!(frame_is_visible(&CapturePhase::AfterDispatch, 128));
  assert!(!frame_is_visible(&CapturePhase::AfterDispatch, 129));
}

#[test]
fn format_yolo_label_normalizes_bbox() {
  let plan = ExportFramePlan {
    frame: FrameKey::from_parts(0, CapturePhase::BeforeDispatch, "capture.png"),
    source_capture_path: PathBuf::from("capture.png"),
    source_capture_file: "capture.png".to_string(),
    image_file: "capture.png".to_string(),
    label_file: "capture.txt".to_string(),
    overlay_file: "capture.png".to_string(),
    class_id: 0,
    label: "hit_circle".to_string(),
    bbox: BoundingBox {
      x1: 10.0,
      y1: 20.0,
      x2: 30.0,
      y2: 60.0,
    },
    image_size: ImageSize {
      width: 100,
      height: 200,
    },
  };

  assert_eq!(format_yolo_label(&plan), "0 0.200000 0.200000 0.200000 0.200000\n");
}
