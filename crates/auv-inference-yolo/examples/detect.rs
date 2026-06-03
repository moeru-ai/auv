use auv_inference_yolo::{
  DetectionOptions, ImageFrame, ModelId, YoloDetector, YoloFamily, YoloModelConfig,
  render_annotated_image,
};
use image::ImageReader;
use std::{
  env,
  error::Error,
  ffi::OsString,
  fs,
  path::{Path, PathBuf},
};

const INPUT_SIZE: u32 = 640;

#[derive(Debug)]
struct Args {
  model: PathBuf,
  classes: PathBuf,
  image: PathBuf,
  json_out: PathBuf,
  annotated_out: Option<PathBuf>,
  confidence: f32,
  iou: f32,
}

fn main() -> Result<(), Box<dyn Error>> {
  let args = Args::parse(env::args_os().skip(1))?;
  let class_names = load_class_names(&args.classes)?;
  let image = ImageReader::open(&args.image)?.decode()?.to_rgb8();
  let frame = ImageFrame::new(image);
  let detector = YoloDetector::load(YoloModelConfig {
    model_id: model_id(&args.model),
    model_path: args.model,
    class_names,
    input_size: INPUT_SIZE,
    family: YoloFamily::UltralyticsV8Like,
  })?;

  let result = detector.detect(
    &frame,
    DetectionOptions {
      confidence_threshold: args.confidence,
      iou_threshold: args.iou,
    },
  )?;
  fs::write(&args.json_out, serde_json::to_vec_pretty(&result)?)?;

  if let Some(path) = &args.annotated_out {
    let annotated = render_annotated_image(&frame.image, &result.detections);
    annotated.save(path)?;
  }

  println!(
    "wrote {} detections to {}",
    result.detections.len(),
    args.json_out.display()
  );

  Ok(())
}

impl Args {
  fn parse(mut values: impl Iterator<Item = OsString>) -> Result<Self, Box<dyn Error>> {
    let mut model = None;
    let mut classes = None;
    let mut image = None;
    let mut json_out = None;
    let mut annotated_out = None;
    let mut confidence = DetectionOptions::default().confidence_threshold;
    let mut iou = DetectionOptions::default().iou_threshold;

    while let Some(flag) = values.next() {
      let flag = flag
        .to_str()
        .ok_or_else(|| format!("argument flag is not valid UTF-8: {flag:?}"))?;
      match flag {
        "--model" => model = Some(next_path(&mut values, flag)?),
        "--classes" => classes = Some(next_path(&mut values, flag)?),
        "--image" => image = Some(next_path(&mut values, flag)?),
        "--json-out" => json_out = Some(next_path(&mut values, flag)?),
        "--annotated-out" => annotated_out = Some(next_path(&mut values, flag)?),
        "--confidence" => confidence = next_f32(&mut values, flag)?,
        "--iou" => iou = next_f32(&mut values, flag)?,
        "--help" | "-h" => return Err(usage().into()),
        unknown => return Err(format!("unknown argument: {unknown}\n\n{}", usage()).into()),
      }
    }

    Ok(Self {
      model: required_path(model, "--model")?,
      classes: required_path(classes, "--classes")?,
      image: required_path(image, "--image")?,
      json_out: required_path(json_out, "--json-out")?,
      annotated_out,
      confidence,
      iou,
    })
  }
}

fn next_path(
  values: &mut impl Iterator<Item = OsString>,
  flag: &str,
) -> Result<PathBuf, Box<dyn Error>> {
  Ok(PathBuf::from(next_value(values, flag)?))
}

fn next_f32(
  values: &mut impl Iterator<Item = OsString>,
  flag: &str,
) -> Result<f32, Box<dyn Error>> {
  let value = next_value(values, flag)?;
  let value = value
    .to_str()
    .ok_or_else(|| format!("{flag} value is not valid UTF-8: {value:?}"))?;
  Ok(value.parse()?)
}

fn next_value(
  values: &mut impl Iterator<Item = OsString>,
  flag: &str,
) -> Result<OsString, Box<dyn Error>> {
  values
    .next()
    .ok_or_else(|| format!("{flag} requires a value").into())
}

fn required_path(value: Option<PathBuf>, flag: &str) -> Result<PathBuf, Box<dyn Error>> {
  value.ok_or_else(|| format!("{flag} is required\n\n{}", usage()).into())
}

fn load_class_names(path: &Path) -> Result<Vec<String>, Box<dyn Error>> {
  let class_names = fs::read_to_string(path)?
    .lines()
    .filter_map(|line| {
      let label = line.trim();
      (!label.is_empty()).then(|| label.to_string())
    })
    .collect::<Vec<_>>();

  Ok(class_names)
}

fn model_id(path: &Path) -> ModelId {
  let id = path
    .file_stem()
    .and_then(|value| value.to_str())
    .unwrap_or("yolo-model");
  ModelId(id.to_string())
}

fn usage() -> &'static str {
  "usage: detect --model <model.onnx> --classes <classes.txt> --image <image> --json-out <detections.json> [--annotated-out <annotated.png>] [--confidence <0..1>] [--iou <0..1>]"
}
