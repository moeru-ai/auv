#[cfg(feature = "card-corner-onnx")]
use auv_game_balatro::card_corner::{CARD_CORNER_MODEL_ID, CardCornerClassifier, CardCornerClassifierConfig};
#[cfg(feature = "card-corner-onnx")]
use auv_game_balatro::config::BalatroModelAsset;
#[cfg(feature = "card-corner-onnx")]
use serde::Serialize;
#[cfg(feature = "card-corner-onnx")]
use std::path::PathBuf;

#[cfg(feature = "card-corner-onnx")]
#[derive(Debug, Serialize)]
struct SmokeOutput {
  model: &'static str,
  image: String,
  prediction: auv_game_balatro::card_corner::CardCornerPrediction,
}

#[cfg(feature = "card-corner-onnx")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
  let args = Args::parse()?;
  let image = image::open(&args.image)?.to_rgb8();
  let config = match args.model {
    Some(model) => CardCornerClassifierConfig::from_model_asset(&BalatroModelAsset::local(model))?,
    None => CardCornerClassifierConfig::default_model()?,
  };
  let classifier = CardCornerClassifier::load(config)?;
  let prediction = classifier.predict(&image)?;
  println!(
    "{}",
    serde_json::to_string_pretty(&SmokeOutput {
      model: CARD_CORNER_MODEL_ID,
      image: args.image.display().to_string(),
      prediction,
    })?
  );
  Ok(())
}

#[cfg(not(feature = "card-corner-onnx"))]
fn main() {
  eprintln!("card_corner_smoke requires the `card-corner-onnx` feature");
}

#[cfg(feature = "card-corner-onnx")]
struct Args {
  model: Option<PathBuf>,
  image: PathBuf,
}

#[cfg(feature = "card-corner-onnx")]
impl Args {
  fn parse() -> Result<Self, String> {
    let mut model = None;
    let mut image = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
      match arg.as_str() {
        "--model" => {
          model = Some(PathBuf::from(args.next().ok_or_else(|| "--model requires a path".to_string())?));
        }
        "--image" => {
          image = Some(PathBuf::from(args.next().ok_or_else(|| "--image requires a path".to_string())?));
        }
        _ => return Err(format!("unknown argument: {arg}")),
      }
    }
    Ok(Self {
      model,
      image: image.ok_or_else(|| "--image is required".to_string())?,
    })
  }
}
