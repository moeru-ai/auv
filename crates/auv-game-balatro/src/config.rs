use std::path::{Path, PathBuf};

use auv_inference_ultralytics::InferenceDevice;
use hf_hub::{HFClientSync, HFError};
use thiserror::Error;

const HF_OWNER: &str = "proj-airi";
const ENTITIES_MODEL_REPO: &str = "games-balatro-2024-yolo-entities-detection";
const ENTITIES_DATASET_REPO: &str = "games-balatro-2024-entities-detection";
const UI_MODEL_REPO: &str = "games-balatro-2024-yolo-ui-detection";
const UI_DATASET_REPO: &str = "games-balatro-2024-ui-detection";
const CARD_CORNER_MODEL_REPO: &str = "games-balatro-2024-card-corner-classifier";
const ONNX_MODEL_FILE: &str = "onnx/model.onnx";
const CLASSES_FILE: &str = "data/train/yolo/classes.txt";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BalatroModelAsset {
  Local(PathBuf),
  HuggingFace {
    repo_kind: HuggingFaceRepoKind,
    owner: &'static str,
    repo: &'static str,
    filename: &'static str,
  },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HuggingFaceRepoKind {
  Model,
  Dataset,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BalatroModelConfig {
  pub entities_model: BalatroModelAsset,
  pub entities_classes: BalatroModelAsset,
  pub ui_model: BalatroModelAsset,
  pub ui_classes: BalatroModelAsset,
  pub card_corner_model: BalatroModelAsset,
  pub device: InferenceDevice,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedBalatroModelConfig {
  pub entities_model: PathBuf,
  pub entities_classes: PathBuf,
  pub ui_model: PathBuf,
  pub ui_classes: PathBuf,
  pub device: InferenceDevice,
}

#[derive(Debug, Error)]
pub enum BalatroModelConfigError {
  #[error("failed to initialize Hugging Face client: {0}")]
  HuggingFaceClient(#[source] HFError),
  #[error("failed to resolve Hugging Face {repo_kind:?} asset {owner}/{repo}:{filename}: {source}")]
  HuggingFaceAsset {
    repo_kind: HuggingFaceRepoKind,
    owner: &'static str,
    repo: &'static str,
    filename: &'static str,
    #[source]
    source: HFError,
  },
}

impl BalatroModelConfig {
  pub fn from_observe_args(args: &crate::cli::ObserveArgs) -> Self {
    let defaults = Self::default();
    Self {
      entities_model: args
        .entities_model
        .clone()
        .map(BalatroModelAsset::Local)
        .unwrap_or(defaults.entities_model),
      entities_classes: args
        .entities_classes
        .clone()
        .map(BalatroModelAsset::Local)
        .unwrap_or(defaults.entities_classes),
      ui_model: args
        .ui_model
        .clone()
        .map(BalatroModelAsset::Local)
        .unwrap_or(defaults.ui_model),
      ui_classes: args
        .ui_classes
        .clone()
        .map(BalatroModelAsset::Local)
        .unwrap_or(defaults.ui_classes),
      card_corner_model: args
        .card_corner_model
        .clone()
        .map(BalatroModelAsset::Local)
        .unwrap_or(defaults.card_corner_model),
      device: args.device.clone(),
    }
  }

  pub fn resolve(&self) -> Result<ResolvedBalatroModelConfig, BalatroModelConfigError> {
    let mut client = None;
    Ok(ResolvedBalatroModelConfig {
      entities_model: self.entities_model.resolve_with_client(&mut client)?,
      entities_classes: self.entities_classes.resolve_with_client(&mut client)?,
      ui_model: self.ui_model.resolve_with_client(&mut client)?,
      ui_classes: self.ui_classes.resolve_with_client(&mut client)?,
      device: self.device.clone(),
    })
  }
}

impl BalatroModelAsset {
  pub fn local(path: impl Into<PathBuf>) -> Self {
    Self::Local(path.into())
  }

  pub const fn hugging_face_model(
    owner: &'static str,
    repo: &'static str,
    filename: &'static str,
  ) -> Self {
    Self::HuggingFace {
      repo_kind: HuggingFaceRepoKind::Model,
      owner,
      repo,
      filename,
    }
  }

  pub const fn hugging_face_dataset(
    owner: &'static str,
    repo: &'static str,
    filename: &'static str,
  ) -> Self {
    Self::HuggingFace {
      repo_kind: HuggingFaceRepoKind::Dataset,
      owner,
      repo,
      filename,
    }
  }

  pub fn resolve_path(&self) -> Result<PathBuf, BalatroModelConfigError> {
    self.resolve_with_client(&mut None)
  }

  fn resolve_with_client(
    &self,
    client: &mut Option<HFClientSync>,
  ) -> Result<PathBuf, BalatroModelConfigError> {
    match self {
      BalatroModelAsset::Local(path) => Ok(path.clone()),
      BalatroModelAsset::HuggingFace {
        repo_kind,
        owner,
        repo,
        filename,
      } => {
        let client = match client {
          Some(client) => client,
          None => {
            client.insert(HFClientSync::new().map_err(BalatroModelConfigError::HuggingFaceClient)?)
          }
        };
        match repo_kind {
          HuggingFaceRepoKind::Model => client
            .model(*owner, *repo)
            .download_file()
            .filename(*filename)
            .send(),
          HuggingFaceRepoKind::Dataset => client
            .dataset(*owner, *repo)
            .download_file()
            .filename(*filename)
            .send(),
        }
        .map_err(|source| BalatroModelConfigError::HuggingFaceAsset {
          repo_kind: *repo_kind,
          owner,
          repo,
          filename,
          source,
        })
      }
    }
  }
}

impl Default for BalatroModelConfig {
  fn default() -> Self {
    Self {
      entities_model: BalatroModelAsset::hugging_face_model(
        HF_OWNER,
        ENTITIES_MODEL_REPO,
        ONNX_MODEL_FILE,
      ),
      entities_classes: BalatroModelAsset::hugging_face_dataset(
        HF_OWNER,
        ENTITIES_DATASET_REPO,
        CLASSES_FILE,
      ),
      ui_model: BalatroModelAsset::hugging_face_model(HF_OWNER, UI_MODEL_REPO, ONNX_MODEL_FILE),
      ui_classes: BalatroModelAsset::hugging_face_dataset(HF_OWNER, UI_DATASET_REPO, CLASSES_FILE),
      card_corner_model: BalatroModelAsset::hugging_face_model(
        HF_OWNER,
        CARD_CORNER_MODEL_REPO,
        ONNX_MODEL_FILE,
      ),
      device: InferenceDevice::Cpu,
    }
  }
}

pub fn load_class_names(path: &Path) -> Result<Vec<String>, std::io::Error> {
  let contents = std::fs::read_to_string(path)?;
  Ok(
    contents
      .lines()
      .map(str::trim)
      .filter(|line| !line.is_empty())
      .map(str::to_owned)
      .collect(),
  )
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::cli::{Format, ObserveArgs};
  use auv_inference_ultralytics::InferenceDevice;
  use std::time::{SystemTime, UNIX_EPOCH};

  #[test]
  fn default_assets_use_hugging_face_sources() {
    let config = BalatroModelConfig::default();

    assert_eq!(
      &config.entities_model,
      &BalatroModelAsset::hugging_face_model(
        "proj-airi",
        "games-balatro-2024-yolo-entities-detection",
        "onnx/model.onnx"
      ),
    );
    assert_eq!(
      &config.entities_classes,
      &BalatroModelAsset::hugging_face_dataset(
        "proj-airi",
        "games-balatro-2024-entities-detection",
        "data/train/yolo/classes.txt"
      ),
    );
    assert_eq!(
      &config.ui_model,
      &BalatroModelAsset::hugging_face_model(
        "proj-airi",
        "games-balatro-2024-yolo-ui-detection",
        "onnx/model.onnx"
      ),
    );
    assert_eq!(
      &config.ui_classes,
      &BalatroModelAsset::hugging_face_dataset(
        "proj-airi",
        "games-balatro-2024-ui-detection",
        "data/train/yolo/classes.txt"
      ),
    );
    assert_eq!(
      &config.card_corner_model,
      &BalatroModelAsset::hugging_face_model(
        "proj-airi",
        "games-balatro-2024-card-corner-classifier",
        "onnx/model.onnx"
      ),
    );
    assert_eq!(config.device, InferenceDevice::Cpu);
  }

  #[test]
  fn default_assets_do_not_reference_owner_local_paths() {
    let config = BalatroModelConfig::default();

    let rendered = format!("{config:?}");

    assert!(
      !rendered.contains("/Users/") && !rendered.contains("/home/"),
      "default Balatro model config should be portable, got {rendered}"
    );
  }

  #[test]
  fn observe_args_override_paths_and_device() {
    let args = ObserveArgs {
      image: None,
      target: "Balatro".to_string(),
      json: false,
      format: Format::Text,
      json_out: None,
      no_cache: false,
      entities_model: Some(PathBuf::from("/tmp/entities.onnx")),
      entities_classes: Some(PathBuf::from("/tmp/entities.txt")),
      ui_model: Some(PathBuf::from("/tmp/ui.onnx")),
      ui_classes: Some(PathBuf::from("/tmp/ui.txt")),
      card_corner_model: Some(PathBuf::from("/tmp/card-corner.onnx")),
      device: InferenceDevice::Cuda(2),
    };

    let config = BalatroModelConfig::from_observe_args(&args);

    assert_eq!(
      config.entities_model,
      BalatroModelAsset::local("/tmp/entities.onnx")
    );
    assert_eq!(
      config.entities_classes,
      BalatroModelAsset::local("/tmp/entities.txt")
    );
    assert_eq!(config.ui_model, BalatroModelAsset::local("/tmp/ui.onnx"));
    assert_eq!(config.ui_classes, BalatroModelAsset::local("/tmp/ui.txt"));
    assert_eq!(
      config.card_corner_model,
      BalatroModelAsset::local("/tmp/card-corner.onnx")
    );
    assert_eq!(config.device, InferenceDevice::Cuda(2));
  }

  #[test]
  fn load_class_names_trims_and_skips_blank_lines() {
    let path = unique_temp_path("balatro-classes");
    std::fs::write(&path, "  card\n\nbutton  \n   \nblind\n").unwrap();

    let class_names = load_class_names(&path).unwrap();

    assert_eq!(class_names, vec!["card", "button", "blind"]);
    let _ = std::fs::remove_file(path);
  }

  fn unique_temp_path(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .unwrap()
      .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{}-{nanos}.txt", std::process::id()))
  }
}
