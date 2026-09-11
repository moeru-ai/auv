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
const CARD_IDENTITY_MODEL_REPO: &str = "games-balatro-2024-yolo-card-identity-detection-mod-ground-truth";
const CARD_ENHANCEMENT_MODEL_REPO: &str = "games-balatro-2024-yolo-card-enhancement-detection-mod-ground-truth";
const CARD_EDITION_MODEL_REPO: &str = "games-balatro-2024-yolo-card-edition-detection-mod-ground-truth";
const CARD_SEAL_MODEL_REPO: &str = "games-balatro-2024-yolo-card-seal-detection-mod-ground-truth";
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
  pub cards_model: Option<BalatroModelAsset>,
  pub card_identity_model: BalatroModelAsset,
  pub card_enhancement_model: BalatroModelAsset,
  pub card_edition_model: BalatroModelAsset,
  pub card_seal_model: BalatroModelAsset,
  pub ui_model: BalatroModelAsset,
  pub ui_classes: BalatroModelAsset,
  pub card_corner_model: BalatroModelAsset,
  pub device: InferenceDevice,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedBalatroModelConfig {
  pub entities_model: PathBuf,
  pub entities_classes: PathBuf,
  pub cards_model: Option<PathBuf>,
  pub card_identity_model: PathBuf,
  pub card_enhancement_model: PathBuf,
  pub card_edition_model: PathBuf,
  pub card_seal_model: PathBuf,
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
      entities_model: args.entities_model.clone().map(BalatroModelAsset::Local).unwrap_or(defaults.entities_model),
      entities_classes: args.entities_classes.clone().map(BalatroModelAsset::Local).unwrap_or(defaults.entities_classes),
      cards_model: args.cards_model.clone().map(BalatroModelAsset::Local).or(defaults.cards_model),
      card_identity_model: args.card_identity_model.clone().map(BalatroModelAsset::Local).unwrap_or(defaults.card_identity_model),
      card_enhancement_model: args.card_enhancement_model.clone().map(BalatroModelAsset::Local).unwrap_or(defaults.card_enhancement_model),
      card_edition_model: args.card_edition_model.clone().map(BalatroModelAsset::Local).unwrap_or(defaults.card_edition_model),
      card_seal_model: args.card_seal_model.clone().map(BalatroModelAsset::Local).unwrap_or(defaults.card_seal_model),
      ui_model: args.ui_model.clone().map(BalatroModelAsset::Local).unwrap_or(defaults.ui_model),
      ui_classes: args.ui_classes.clone().map(BalatroModelAsset::Local).unwrap_or(defaults.ui_classes),
      card_corner_model: args.card_corner_model.clone().map(BalatroModelAsset::Local).unwrap_or(defaults.card_corner_model),
      device: args.device.clone(),
    }
  }

  pub fn from_operation_args(args: &crate::cli::OperationControlArgs) -> Self {
    let defaults = Self::default();
    Self {
      cards_model: args.cards_model.clone().map(BalatroModelAsset::Local).or_else(|| defaults.cards_model.clone()),
      device: args.device.clone(),
      ..defaults
    }
  }

  pub fn resolve(&self) -> Result<ResolvedBalatroModelConfig, BalatroModelConfigError> {
    let mut client = None;
    Ok(ResolvedBalatroModelConfig {
      entities_model: self.entities_model.resolve_with_client(&mut client)?,
      entities_classes: self.entities_classes.resolve_with_client(&mut client)?,
      cards_model: self.cards_model.as_ref().map(|asset| asset.resolve_with_client(&mut client)).transpose()?,
      card_identity_model: self.card_identity_model.resolve_with_client(&mut client)?,
      card_enhancement_model: self.card_enhancement_model.resolve_with_client(&mut client)?,
      card_edition_model: self.card_edition_model.resolve_with_client(&mut client)?,
      card_seal_model: self.card_seal_model.resolve_with_client(&mut client)?,
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

  pub const fn hugging_face_model(owner: &'static str, repo: &'static str, filename: &'static str) -> Self {
    Self::HuggingFace {
      repo_kind: HuggingFaceRepoKind::Model,
      owner,
      repo,
      filename,
    }
  }

  pub const fn hugging_face_dataset(owner: &'static str, repo: &'static str, filename: &'static str) -> Self {
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

  fn resolve_with_client(&self, client: &mut Option<HFClientSync>) -> Result<PathBuf, BalatroModelConfigError> {
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
          None => client.insert(HFClientSync::new().map_err(BalatroModelConfigError::HuggingFaceClient)?),
        };
        match repo_kind {
          HuggingFaceRepoKind::Model => client.model(*owner, *repo).download_file().filename(*filename).send(),
          HuggingFaceRepoKind::Dataset => client.dataset(*owner, *repo).download_file().filename(*filename).send(),
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
      entities_model: BalatroModelAsset::hugging_face_model(HF_OWNER, ENTITIES_MODEL_REPO, ONNX_MODEL_FILE),
      entities_classes: BalatroModelAsset::hugging_face_dataset(HF_OWNER, ENTITIES_DATASET_REPO, CLASSES_FILE),
      // TODO(card-detector-publication): Keep the card-specialized model
      // opt-in until its game-asset/derived-weight rights are reviewed and the
      // private Hugging Face repository can become a usable default.
      cards_model: None,
      card_identity_model: BalatroModelAsset::hugging_face_model(HF_OWNER, CARD_IDENTITY_MODEL_REPO, ONNX_MODEL_FILE),
      card_enhancement_model: BalatroModelAsset::hugging_face_model(HF_OWNER, CARD_ENHANCEMENT_MODEL_REPO, ONNX_MODEL_FILE),
      card_edition_model: BalatroModelAsset::hugging_face_model(HF_OWNER, CARD_EDITION_MODEL_REPO, ONNX_MODEL_FILE),
      card_seal_model: BalatroModelAsset::hugging_face_model(HF_OWNER, CARD_SEAL_MODEL_REPO, ONNX_MODEL_FILE),
      ui_model: BalatroModelAsset::hugging_face_model(HF_OWNER, UI_MODEL_REPO, ONNX_MODEL_FILE),
      ui_classes: BalatroModelAsset::hugging_face_dataset(HF_OWNER, UI_DATASET_REPO, CLASSES_FILE),
      card_corner_model: BalatroModelAsset::hugging_face_model(HF_OWNER, CARD_CORNER_MODEL_REPO, ONNX_MODEL_FILE),
      device: InferenceDevice::Cpu,
    }
  }
}

pub fn load_class_names(path: &Path) -> Result<Vec<String>, std::io::Error> {
  let contents = std::fs::read_to_string(path)?;
  Ok(contents.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_owned).collect())
}
