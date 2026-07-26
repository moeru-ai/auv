use super::*;

#[test]
fn args_use_detection_defaults() {
  let args = Args::parse_from([
    "detect",
    "--model",
    "model.onnx",
    "--image",
    "image.jpg",
    "--json-out",
    "detections.json",
  ])
  .unwrap();

  assert_eq!(args.confidence, 0.25);
  assert_eq!(args.iou, 0.45);
  assert_eq!(args.max_detections, 300);
  assert_eq!(args.input_size, 640);
  assert_eq!(args.device, InferenceDevice::Cpu);
}
