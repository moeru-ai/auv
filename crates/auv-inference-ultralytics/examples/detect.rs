use auv_inference_common::{DetectionOptions, ModelId, render_annotated_image};
use auv_inference_ultralytics::{InferenceDevice, UltralyticsDetector, UltralyticsModelConfig};
use image::{ImageFormat, ImageReader};
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
  annotated_out: Option<PathBuf>,
  confidence: f32,
  iou: f32,
  max_detections: usize,
  input_size: u32,
  device: InferenceDevice,
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
    let mut annotated_out = None;
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
        "--annotated-out" => annotated_out = Some(PathBuf::from(next_value(&mut args, &flag)?)),
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
      annotated_out,
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
  if let Some(path) = &args.annotated_out {
    require_png_path(path)?;
  }

  let detector = UltralyticsDetector::load(UltralyticsModelConfig {
    model_id: model_id_from_path(&args.model),
    model_path: args.model,
    input_size: Some(args.input_size),
    options: DetectionOptions {
      confidence_threshold: args.confidence,
      iou_threshold: args.iou,
      max_detections: args.max_detections,
    },
    device: args.device,
    class_names_override: load_class_names(args.classes.as_deref())?,
  })?;

  let detections = detector.detect_path(&args.image)?;
  write_json(&args.json_out, &detections)?;

  if let Some(path) = &args.annotated_out {
    let image = ImageReader::open(&args.image)?.decode()?.to_rgb8();
    let annotated = render_annotated_image(&image, &detections.detections);
    annotated.save_with_format(path, ImageFormat::Png)?;
  }

  println!("detections: {}", detections.detections.len());
  println!("json: {}", args.json_out.display());
  if let Some(path) = &args.annotated_out {
    println!("annotated: {}", path.display());
  }

  Ok(())
}

fn load_class_names(path: Option<&Path>) -> ExampleResult<Option<Vec<String>>> {
  let Some(path) = path else {
    return Ok(None);
  };
  let names = std::fs::read_to_string(path)?
    .lines()
    .map(str::trim)
    .filter(|line| !line.is_empty())
    .map(str::to_string)
    .collect();
  Ok(Some(names))
}

fn write_json(path: &Path, detections: &auv_inference_common::DetectionSet) -> ExampleResult<()> {
  let file = File::create(path)?;
  let mut writer = BufWriter::new(file);
  serde_json::to_writer_pretty(&mut writer, detections)?;
  writer.write_all(b"\n")?;
  // BufWriter::drop silently swallows flush errors, which would let this CLI
  // exit 0 with truncated JSON on the rare write-on-drop failure. Flush
  // explicitly so the error surfaces.
  writer.flush()?;
  Ok(())
}

fn model_id_from_path(path: &Path) -> ModelId {
  let id = path
    .file_stem()
    .or_else(|| path.file_name())
    .and_then(|value| value.to_str())
    .unwrap_or("ultralytics-model")
    .to_string();
  ModelId(id)
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> ExampleResult<String> {
  args
    .next()
    .ok_or_else(|| invalid(format!("{flag} requires a value")))
}

fn parse_value<T>(flag: &str, value: String) -> ExampleResult<T>
where
  T: FromStr,
  T::Err: std::fmt::Display,
{
  value
    .parse()
    .map_err(|err| invalid(format!("invalid value for {flag}: {err}")))
}

fn required<T>(value: Option<T>, flag: &str) -> ExampleResult<T> {
  value.ok_or_else(|| invalid(format!("{flag} is required")))
}

fn require_png_path(path: &Path) -> ExampleResult<()> {
  if path
    .extension()
    .and_then(|extension| extension.to_str())
    .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
  {
    return Ok(());
  }

  Err(invalid(format!(
    "--annotated-out must point to a .png file: {}",
    path.display()
  )))
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

  #[test]
  fn annotated_output_requires_png_path() {
    assert!(require_png_path(Path::new("detections.png")).is_ok());
    assert!(require_png_path(Path::new("detections.PNG")).is_ok());
    assert!(require_png_path(Path::new("detections.jpg")).is_err());
    assert!(require_png_path(Path::new("detections")).is_err());
  }
}
