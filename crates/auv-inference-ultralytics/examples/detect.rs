use auv_inference_common::ModelId;
use auv_inference_ultralytics::{InferenceDevice, UltralyticsModelConfig, UltralyticsSession};
use serde::Serialize;
use std::error::Error;
use std::fs::File;
use std::io::{BufWriter, Error as IoError, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

type ExampleResult<T> = Result<T, Box<dyn Error>>;

#[derive(Debug)]
struct Args {
  model: PathBuf,
  classes: Option<PathBuf>,
  image: PathBuf,
  json_out: PathBuf,
  confidence: f32,
  iou: f32,
  max_detections: usize,
  input_size: u32,
  device: InferenceDevice,
}

#[derive(Debug, Serialize)]
struct JsonPrediction {
  model_id: String,
  image_size: JsonImageSize,
  detections: Vec<JsonDetection>,
}

#[derive(Debug, Serialize)]
struct JsonImageSize {
  width: u32,
  height: u32,
}

#[derive(Debug, Serialize)]
struct JsonDetection {
  class_id: usize,
  label: String,
  confidence: f32,
  bbox: JsonBoundingBox,
}

#[derive(Debug, Serialize)]
struct JsonBoundingBox {
  x1: f32,
  y1: f32,
  x2: f32,
  y2: f32,
}

impl Args {
  fn parse_env() -> ExampleResult<Self> {
    Self::parse_from(std::env::args())
  }

  fn parse_from<I, S>(args: I) -> ExampleResult<Self>
  where
    I: IntoIterator<Item = S>,
    S: Into<String>,
  {
    let mut model = None;
    let mut classes = None;
    let mut image = None;
    let mut json_out = None;
    let mut confidence = 0.25;
    let mut iou = 0.45;
    let mut max_detections = 300;
    let mut input_size = 640;
    let mut device = InferenceDevice::Cpu;
    let mut args = args.into_iter().map(Into::into);
    let _program = args.next();

    while let Some(flag) = args.next() {
      match flag.as_str() {
        "--model" => model = Some(PathBuf::from(next_value(&mut args, &flag)?)),
        "--classes" => classes = Some(PathBuf::from(next_value(&mut args, &flag)?)),
        "--image" => image = Some(PathBuf::from(next_value(&mut args, &flag)?)),
        "--json-out" => json_out = Some(PathBuf::from(next_value(&mut args, &flag)?)),
        "--confidence" => confidence = parse_value(&flag, next_value(&mut args, &flag)?)?,
        "--iou" => iou = parse_value(&flag, next_value(&mut args, &flag)?)?,
        "--max-detections" => max_detections = parse_value(&flag, next_value(&mut args, &flag)?)?,
        "--input-size" => input_size = parse_value(&flag, next_value(&mut args, &flag)?)?,
        "--device" => device = parse_value(&flag, next_value(&mut args, &flag)?)?,
        _ => return Err(invalid(format!("unknown argument: {flag}"))),
      }
    }

    Ok(Self {
      model: required(model, "--model")?,
      classes,
      image: required(image, "--image")?,
      json_out: required(json_out, "--json-out")?,
      confidence,
      iou,
      max_detections,
      input_size,
      device,
    })
  }
}

fn main() -> ExampleResult<()> {
  run(Args::parse_env()?)
}

fn run(args: Args) -> ExampleResult<()> {
  let session = UltralyticsSession::load(UltralyticsModelConfig {
    model_id: model_id_from_path(&args.model),
    model_path: args.model,
    input_size: Some(args.input_size),
    confidence_threshold: args.confidence,
    iou_threshold: args.iou,
    max_detections: args.max_detections,
    device: args.device,
    class_names_override: load_class_names(args.classes.as_deref())?,
  })?;

  let prediction = session.predict_path(&args.image)?;
  let json_prediction = build_json_prediction(&prediction)?;
  write_json(&args.json_out, &json_prediction)?;

  println!("detections: {}", json_prediction.detections.len());
  println!("json: {}", args.json_out.display());

  Ok(())
}

fn build_json_prediction(prediction: &auv_inference_ultralytics::UltralyticsPrediction) -> ExampleResult<JsonPrediction> {
  let result = prediction.first_result()?;
  let detections = if let Some(boxes) = result.boxes() {
    let mut detections = Vec::with_capacity(boxes.len());
    for index in 0..boxes.len() {
      let [x1, y1, x2, y2] = boxes.xyxy(index)?;
      detections.push(JsonDetection {
        class_id: boxes.class_id(index)?,
        label: boxes.label(index)?,
        confidence: boxes.confidence(index)?,
        bbox: JsonBoundingBox { x1, y1, x2, y2 },
      });
    }
    detections
  } else {
    Vec::new()
  };

  Ok(JsonPrediction {
    model_id: prediction.model_id().0.clone(),
    image_size: JsonImageSize {
      width: result.image_width(),
      height: result.image_height(),
    },
    detections,
  })
}

fn load_class_names(path: Option<&Path>) -> ExampleResult<Option<Vec<String>>> {
  let Some(path) = path else {
    return Ok(None);
  };
  let names = std::fs::read_to_string(path)?.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_string).collect();
  Ok(Some(names))
}

fn write_json(path: &Path, prediction: &JsonPrediction) -> ExampleResult<()> {
  let file = File::create(path)?;
  let mut writer = BufWriter::new(file);
  serde_json::to_writer_pretty(&mut writer, prediction)?;
  writer.write_all(b"\n")?;
  writer.flush()?;
  Ok(())
}

fn model_id_from_path(path: &Path) -> ModelId {
  let id = path.file_stem().or_else(|| path.file_name()).and_then(|value| value.to_str()).unwrap_or("ultralytics-model").to_string();
  ModelId(id)
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> ExampleResult<String> {
  args.next().ok_or_else(|| invalid(format!("{flag} requires a value")))
}

fn parse_value<T>(flag: &str, value: String) -> ExampleResult<T>
where
  T: FromStr,
  T::Err: std::fmt::Display,
{
  value.parse().map_err(|err| invalid(format!("invalid value for {flag}: {err}")))
}

fn required<T>(value: Option<T>, flag: &str) -> ExampleResult<T> {
  value.ok_or_else(|| invalid(format!("{flag} is required")))
}

fn invalid(message: String) -> Box<dyn Error> {
  Box::new(IoError::new(ErrorKind::InvalidInput, message))
}

#[cfg(test)]
mod tests {
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
}
