use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use auv::AuvContext;
use auv_driver::WindowInput as _;
use auv_driver::capture::{Activation, Capture, CaptureOptions};
use auv_driver::geometry::{Point, RatioRect, Rect, WindowPoint};
use auv_driver::input::{ClickOptions, InputPolicy};
use auv_driver::selector::{App, Window};
use auv_driver::vision::TextRecognitionOptions;
use auv_inference_ultralytics::InferenceDevice;
use auv_task_object_detection::BoundingBox;
use clap::{Args, Parser, Subcommand, ValueEnum};
use image::{ImageError, RgbaImage};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::blind_action::{
  BlindSelectAttempt, BlindSelectConfirmation, BlindSelectConfirmationFailure, BlindSelectRequest, BlindSelectResult, BlindSkipConfirmation,
  BlindSkipConfirmationFailure, BlindSkipRequest, BlindSkipResult, emit_blind_select_completed, emit_blind_skip_completed,
  evaluate_blind_select_confirmation, evaluate_blind_skip_confirmation,
};
use crate::cards_action::{
  CardCommitAction, CardCommitConfirmation, CardCommitControl, CardCommitKind, CardCommitRequest, CardCommitResult, CardCommitState,
  CardCommitStop, CardsSelectRequest, CardsSelectResult, card_commit_confirmation_allows_resubmit,
  card_commit_confirmation_has_structural_change, emit_card_commit_completed, emit_cards_select_completed,
  evaluate_card_commit_confirmation, reject_fingerprint_only_confirmation,
};
use crate::cards_clear::{
  CardSelectionToggle, CardsClearIncompleteReason, CardsClearOutcome, CardsClearRequest, CardsClearResult, classify_cards_clear_outcome,
  emit_cards_clear_completed,
};
use crate::cash_out::{
  CashOutAttempt, CashOutConfirmation, CashOutConfirmationRequest, CashOutRequest, CashOutResult, emit_cash_out_completed,
  evaluate_cash_out_confirmation,
};
use crate::config::BalatroModelConfig;
use crate::consumable_use::{
  ConsumableUseAction, ConsumableUseConfirmation, ConsumableUseControl, ConsumableUseRequest, ConsumableUseResult, ConsumableUseState,
  ConsumableUseStop, emit_consumable_use_completed, evaluate_consumable_use_confirmation,
};
use crate::game_restart::{
  GameRestartClick, GameRestartOutcome, GameRestartRequest, GameRestartResult, GameRestartTarget, classify_game_restart_outcome,
  emit_game_restart_completed,
};
use crate::hand_selection::{HandSelectionResult, HandSelectionState, HandSelectionToggle, HandSelectionToggleKind};
use crate::model::{
  ActionPoint, BalatroPhase, BalatroState, ButtonTarget, CardSlot, ConsumableSlot, JokerSlot, ObjectZone, Reading, ReadingStatus, SlotId,
  StoreItem,
};
use crate::object_sell::{
  ObjectSellClick, ObjectSellConfirmation, ObjectSellIncompleteReason, ObjectSellOutcome, ObjectSellRequest, ObjectSellResult,
  SellableObject, emit_object_sell_completed, evaluate_object_sell_confirmation,
};
use crate::observation::{
  HoverReadRequest, ObservationError, apply_ui_numeric_reading, click_display_frame_point_via_api, hover_read_display_frame_points_via_api,
  is_numeric_ui_label, is_score_ui_label, is_single_ui_digit_label, move_display_frame_point_via_api, observe_image, observe_live_via_api,
};
pub use crate::output::OutputMode;
use crate::pack_choose::{
  ObservedPackState, PackChoice, PackChoiceId, PackChooseAction, PackChooseConfirmation, PackChooseControl, PackChooseRequest,
  PackChooseResult, PackChooseState, PackChooseStop, emit_pack_choose_completed, evaluate_pack_choose_confirmation,
};
use crate::pack_skip::{PackSkipConfirmation, PackSkipRequest, PackSkipResult, emit_pack_skip_completed, evaluate_pack_skip_confirmation};
use crate::store_buy::{
  StoreBuyClick, StoreBuyConfirmation, StoreBuyIncompleteReason, StoreBuyOutcome, StoreBuyRequest, StoreBuyResult, emit_store_buy_completed,
  evaluate_store_buy_confirmation,
};
use crate::store_next_round::{
  StoreNextRoundAttempt, StoreNextRoundConfirmation, StoreNextRoundConfirmationRequest, StoreNextRoundRequest, StoreNextRoundResult,
  StoreNextRoundTarget, emit_store_next_round_completed, evaluate_store_next_round_confirmation,
};

const DECK_ATLAS_LOVE_PATH: &str = "resources/textures/2x/8BitDeck.png";
const DECK_ATLAS_CACHE_FILE: &str = "8BitDeck.png";
const SETUP_MANIFEST_FILE: &str = "setup.json";
const SETUP_MANIFEST_SCHEMA_VERSION: &str = "auv.game.balatro.setup.v0";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum Format {
  #[default]
  Text,
  Json,
}

impl fmt::Display for Format {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Text => formatter.write_str("text"),
      Self::Json => formatter.write_str("json"),
    }
  }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum VerifyModeArg {
  #[default]
  Targeted,
  Weak,
  ActivationOnly,
}

impl fmt::Display for VerifyModeArg {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Targeted => formatter.write_str("targeted"),
      Self::Weak => formatter.write_str("weak"),
      Self::ActivationOnly => formatter.write_str("activation-only"),
    }
  }
}

#[derive(Clone, Debug, Eq, Parser, PartialEq)]
#[command(name = "auv-game-balatro")]
pub struct CliArgs {
  #[command(subcommand)]
  pub command: Command,
}

#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
pub enum Command {
  Game(GameArgs),
  /// Annotate one fixed frame and emit detection/click diagnostics.
  Diagnostics(DiagnosticsArgs),
  Objective(ObjectiveArgs),
  Scores(ScoresArgs),
  Rounds(RoundsArgs),
  Cards(CardsArgs),
  Jokers(JokersArgs),
  Consumables(ConsumablesArgs),
  Store(StoreArgs),
  Pack(PackArgs),
  Blinds(BlindsArgs),
  Setup(SetupArgs),
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct DiagnosticsArgs {
  #[command(flatten)]
  pub observe: ObserveArgs,
  /// Expected selected hand slots, for example `hand:0,hand:1`.
  #[arg(long, value_name = "HAND_SLOTS")]
  pub requested_slots: Option<String>,
  #[arg(long, value_name = "PATH")]
  pub annotated_out: PathBuf,
  #[arg(long, value_name = "PATH")]
  pub report_out: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct SetupArgs {
  #[arg(long, value_name = "PATH")]
  pub love: Option<PathBuf>,
  #[arg(long, value_name = "PATH")]
  pub app: Option<PathBuf>,
  #[arg(long, value_name = "PATH")]
  pub cache_dir: Option<PathBuf>,
  #[arg(long)]
  pub check: bool,
  #[arg(long)]
  pub force: bool,
  #[arg(long)]
  pub json: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SetupReport {
  pub schema_version: String,
  pub status: SetupStatus,
  pub cache_dir: PathBuf,
  pub deck_atlas_path: PathBuf,
  pub manifest_path: PathBuf,
  pub source_love_path: Option<PathBuf>,
  pub deck_atlas_sha256: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStatus {
  Ready,
  Reused,
  Extracted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct SetupManifest {
  schema_version: String,
  source_love_path: PathBuf,
  deck_atlas_path: PathBuf,
  deck_atlas_sha256: String,
  extracted_at_ms: u128,
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct ObserveArgs {
  #[arg(long, value_name = "PATH")]
  pub image: Option<PathBuf>,
  #[arg(long, default_value = "Balatro")]
  pub target: String,
  #[arg(long)]
  pub json: bool,
  #[arg(long, default_value_t)]
  pub format: Format,
  #[arg(long, value_name = "PATH")]
  pub json_out: Option<PathBuf>,
  #[arg(long)]
  pub no_cache: bool,
  #[arg(long, value_name = "PATH")]
  pub entities_model: Option<PathBuf>,
  #[arg(long, value_name = "PATH")]
  pub entities_classes: Option<PathBuf>,
  /// Optional card-specialized detector applied only to the normalized hand region.
  #[arg(long, value_name = "PATH")]
  pub cards_model: Option<PathBuf>,
  /// Hand-region rank/suit detector. Defaults to the published Mod ground-truth model.
  #[arg(long, value_name = "PATH")]
  pub card_identity_model: Option<PathBuf>,
  /// Hand-region enhancement detector. Defaults to the published Mod ground-truth model.
  #[arg(long, value_name = "PATH")]
  pub card_enhancement_model: Option<PathBuf>,
  /// Hand-region edition detector. Defaults to the published Mod ground-truth model.
  #[arg(long, value_name = "PATH")]
  pub card_edition_model: Option<PathBuf>,
  /// Hand-region seal detector. Defaults to the published Mod ground-truth model.
  #[arg(long, value_name = "PATH")]
  pub card_seal_model: Option<PathBuf>,
  #[arg(long, value_name = "PATH")]
  pub ui_model: Option<PathBuf>,
  #[arg(long, value_name = "PATH")]
  pub ui_classes: Option<PathBuf>,
  #[arg(long, value_name = "PATH")]
  pub card_corner_model: Option<PathBuf>,
  #[arg(long, default_value = "cpu", value_parser = clap::value_parser!(InferenceDevice))]
  pub device: InferenceDevice,
}

impl ObserveArgs {
  pub fn output_mode(&self) -> OutputMode {
    if let Some(path) = &self.json_out {
      return OutputMode::JsonFile(path.clone());
    }
    if self.json || self.format == Format::Json {
      return OutputMode::Json;
    }
    OutputMode::Human
  }
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct OperationControlArgs {
  #[arg(long, default_value = "Balatro")]
  pub target: String,
  #[arg(long)]
  pub verify: bool,
  #[arg(long, default_value_t)]
  pub verify_mode: VerifyModeArg,
  #[arg(long)]
  pub timeout_ms: Option<u64>,
  #[arg(long, alias = "detailed")]
  pub details: bool,
  /// Optional card-specialized detector used by every observation in this operation.
  #[arg(long, value_name = "PATH")]
  pub cards_model: Option<PathBuf>,
  /// Inference device used by every observation in this operation.
  #[arg(long, default_value = "cpu", value_parser = clap::value_parser!(InferenceDevice))]
  pub device: InferenceDevice,
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct SlotOperationArgs {
  #[arg(long)]
  pub slot: String,
  #[command(flatten)]
  pub control: OperationControlArgs,
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct TargetSlotOperationArgs {
  #[arg(long)]
  pub slot: String,
  #[arg(long, value_delimiter = ',', value_name = "TARGETS")]
  pub targets: Vec<String>,
  #[command(flatten)]
  pub control: OperationControlArgs,
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct MultiSlotOperationArgs {
  #[arg(long, value_name = "SLOTS")]
  pub slots: String,
  #[command(flatten)]
  pub control: OperationControlArgs,
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct GameArgs {
  #[command(subcommand)]
  pub command: GameCommand,
}

#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
pub enum GameCommand {
  State(ObserveArgs),
  CashOut(OperationControlArgs),
  Restart(OperationControlArgs),
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct ObjectiveArgs {
  #[command(flatten)]
  pub observe: ObserveArgs,
  #[arg(long)]
  pub include_scores: bool,
  #[arg(long)]
  pub include_rounds: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct ScoresArgs {
  #[command(subcommand)]
  pub command: ScoresCommand,
}

#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
pub enum ScoresCommand {
  Get(ObserveArgs),
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct RoundsArgs {
  #[command(subcommand)]
  pub command: RoundsCommand,
}

#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
pub enum RoundsCommand {
  Get(ObserveArgs),
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct SlotObserveArgs {
  #[arg(long)]
  pub slot: String,
  #[arg(long, value_name = "PATH")]
  pub frame_out: Option<PathBuf>,
  #[command(flatten)]
  pub observe: ObserveArgs,
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct CardsArgs {
  #[command(subcommand)]
  pub command: CardsCommand,
}

#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
pub enum CardsCommand {
  Ls(ObserveArgs),
  Hand(ObserveArgs),
  Read(SlotObserveArgs),
  Clear(OperationControlArgs),
  Select(MultiSlotOperationArgs),
  Play(MultiSlotOperationArgs),
  Discard(MultiSlotOperationArgs),
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct JokersArgs {
  #[command(subcommand)]
  pub command: JokersCommand,
}

#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
pub enum JokersCommand {
  Ls(ObserveArgs),
  Read(SlotObserveArgs),
  Sell(SlotOperationArgs),
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct ConsumablesArgs {
  #[command(subcommand)]
  pub command: ConsumablesCommand,
}

#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
pub enum ConsumablesCommand {
  Ls(ObserveArgs),
  Read(SlotObserveArgs),
  Sell(SlotOperationArgs),
  Use(TargetSlotOperationArgs),
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct StoreArgs {
  #[command(subcommand)]
  pub command: StoreCommand,
}

#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
pub enum StoreCommand {
  Status(ObserveArgs),
  Ls(ObserveArgs),
  Read(SlotObserveArgs),
  Buy(SlotOperationArgs),
  Reroll(OperationControlArgs),
  NextRound(OperationControlArgs),
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct PackArgs {
  #[command(subcommand)]
  pub command: PackCommand,
}

#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
pub enum PackCommand {
  Read(ObserveArgs),
  Choose(TargetSlotOperationArgs),
  Skip(OperationControlArgs),
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct BlindsArgs {
  #[command(subcommand)]
  pub command: BlindsCommand,
}

#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
pub enum BlindsCommand {
  Ls(ObserveArgs),
  Select(SlotOperationArgs),
  Skip(OperationControlArgs),
}

#[derive(Debug, Error)]
pub enum CliError {
  #[error("Balatro command `{command}` is deferred: {reason}")]
  Deferred {
    command: &'static str,
    reason: &'static str,
  },
  #[error("observation command requires --image until live capture dispatch lands for this surface")]
  MissingImage,
  #[error("observation failed: {0}")]
  Observation(#[from] ObservationError),
  #[error("output failed: {0}")]
  Output(#[from] crate::output::OutputError),
  #[error("driver error: {0}")]
  Driver(#[from] auv_driver::error::DriverError),
  #[error("image error: {0}")]
  Image(#[from] ImageError),
  #[error("io error: {0}")]
  Io(#[from] std::io::Error),
  #[error("json error: {0}")]
  Json(#[from] serde_json::Error),
  #[error("{0}")]
  Message(String),
}

#[cfg(target_os = "macos")]
fn open_macos_session() -> Result<auv_driver::LocalDriverSession, CliError> {
  Ok(auv_driver::open_local()?)
}

pub fn run_from_env() -> Result<(), CliError> {
  let args = CliArgs::parse();
  #[cfg(feature = "tracing")]
  {
    run_with_inherited_tracing(args)
  }
  #[cfg(not(feature = "tracing"))]
  {
    run(args)
  }
}

#[cfg(feature = "tracing")]
fn run_with_inherited_tracing(args: CliArgs) -> Result<(), CliError> {
  use std::sync::Arc;

  let Some(store_root) = std::env::var_os(auv_tracing::STORE_ROOT_ENV) else {
    return run(args);
  };
  let context = AuvContext::from_env().map_err(|error| CliError::Message(format!("invalid inherited AUV context: {error}")))?;
  let Some(control_run_id) = context.run_id.as_deref() else {
    return run(args);
  };
  // TODO(resource-id-migration): Remove the legacy `run_` branch after old
  // control-plane stores no longer expose prefixed UUID Run identities.
  let run_uuid = uuid::Uuid::parse_str(control_run_id.strip_prefix("run_").unwrap_or(control_run_id))
    .map_err(|error| CliError::Message(format!("inherited Run ID {control_run_id:?} is invalid for tracing: {error}")))?;
  let run_id = run_uuid
    .to_string()
    .parse::<auv_tracing::RunId>()
    .map_err(|error| CliError::Message(format!("inherited Run ID {control_run_id:?} is invalid for tracing: {error}")))?;
  let store = Arc::new(
    auv_tracing::FileTracingStore::open(PathBuf::from(store_root))
      .map_err(|error| CliError::Message(format!("failed to open inherited tracing store: {error}")))?,
  );
  let dispatch = auv_tracing::configure()
    .tracing_store(store)
    .build()
    .map_err(|error| CliError::Message(format!("failed to configure Balatro tracing: {error}")))?;
  let result = auv_tracing::dispatcher::with_default(&dispatch, || {
    let root = auv_tracing::Context::root(run_id);
    root.in_scope(|| {
      auv_tracing::emit_event!(BalatroFrontendLifecycle { frontend: "plugin" });
      run(args)
    })
  });
  let flush = futures_executor::block_on(dispatch.flush());
  match (result, flush) {
    (Ok(()), Ok(())) => Ok(()),
    (Err(error), _) => Err(error),
    (Ok(()), Err(error)) => Err(CliError::Message(format!("failed to flush Balatro tracing: {error}"))),
  }
}

#[cfg(feature = "tracing")]
#[derive(Serialize)]
struct BalatroFrontendLifecycle {
  frontend: &'static str,
}

#[cfg(feature = "tracing")]
impl auv_tracing::EventPayload for BalatroFrontendLifecycle {
  const NAME: &'static str = "auv.frontend.lifecycle";
  const VERSION: u32 = 1;
}

pub fn run(args: CliArgs) -> Result<(), CliError> {
  match args.command {
    Command::Diagnostics(args) => write_frame_diagnostics(args),
    Command::Game(GameArgs {
      command: GameCommand::State(args),
    }) => write_observed_state(&args),
    Command::Game(GameArgs {
      command: GameCommand::CashOut(args),
    }) => click_game_cash_out(args),
    Command::Game(GameArgs {
      command: GameCommand::Restart(args),
    }) => click_game_restart(args),
    Command::Objective(args) => write_observed_state(&args.observe),
    Command::Scores(ScoresArgs {
      command: ScoresCommand::Get(args),
    }) => write_scores(&args),
    Command::Rounds(RoundsArgs {
      command: RoundsCommand::Get(args),
    }) => write_rounds(&args),
    Command::Cards(CardsArgs {
      command: CardsCommand::Ls(args) | CardsCommand::Hand(args),
    }) => write_observed_state(&args),
    Command::Cards(CardsArgs {
      command: CardsCommand::Read(args),
    }) => write_card_read(&args),
    Command::Cards(CardsArgs {
      command: CardsCommand::Clear(args),
    }) => click_cards_clear(args),
    Command::Cards(CardsArgs {
      command: CardsCommand::Select(args),
    }) => click_cards_select(args),
    Command::Cards(CardsArgs {
      command: CardsCommand::Play(args),
    }) => click_cards_commit(CardCommitKind::Play, args),
    Command::Cards(CardsArgs {
      command: CardsCommand::Discard(args),
    }) => click_cards_commit(CardCommitKind::Discard, args),
    Command::Jokers(JokersArgs {
      command: JokersCommand::Ls(args),
    }) => write_jokers(&args),
    Command::Jokers(JokersArgs {
      command: JokersCommand::Read(args),
    }) => write_object_read(&args, ObjectReadZone::Joker),
    Command::Jokers(JokersArgs {
      command: JokersCommand::Sell(args),
    }) => click_joker_sell(args),
    Command::Consumables(ConsumablesArgs {
      command: ConsumablesCommand::Ls(args),
    }) => write_observed_state(&args),
    Command::Consumables(ConsumablesArgs {
      command: ConsumablesCommand::Read(args),
    }) => write_object_read(&args, ObjectReadZone::Consumable),
    Command::Consumables(ConsumablesArgs {
      command: ConsumablesCommand::Sell(args),
    }) => click_consumable_sell(args),
    Command::Consumables(ConsumablesArgs {
      command: ConsumablesCommand::Use(args),
    }) => click_consumable_use(args),
    Command::Store(StoreArgs {
      command: StoreCommand::Status(args),
    }) => write_store_status(&args),
    Command::Store(StoreArgs {
      command: StoreCommand::Ls(args),
    }) => write_store_items(&args),
    Command::Store(StoreArgs {
      command: StoreCommand::Read(args),
    }) => write_object_read(&args, ObjectReadZone::Store),
    Command::Store(StoreArgs {
      command: StoreCommand::Buy(args),
    }) => click_store_buy(args),
    Command::Store(StoreArgs {
      command: StoreCommand::Reroll(args),
    }) => click_store_reroll(args),
    Command::Store(StoreArgs {
      command: StoreCommand::NextRound(args),
    }) => click_store_next_round(args),
    Command::Pack(PackArgs {
      command: PackCommand::Read(args),
    }) => write_pack_read(&args),
    Command::Pack(PackArgs {
      command: PackCommand::Choose(args),
    }) => click_pack_choose(args),
    Command::Pack(PackArgs {
      command: PackCommand::Skip(args),
    }) => click_pack_skip(args),
    Command::Blinds(BlindsArgs {
      command: BlindsCommand::Ls(args),
    }) => write_blind_buttons(&args),
    Command::Blinds(BlindsArgs {
      command: BlindsCommand::Select(args),
    }) => click_blind_select(args),
    Command::Blinds(BlindsArgs {
      command: BlindsCommand::Skip(args),
    }) => click_blind_skip(args),
    Command::Setup(args) => run_setup(args),
  }
}

fn run_setup(args: SetupArgs) -> Result<(), CliError> {
  let report = setup_balatro_assets(&args)?;
  if args.json {
    println!("{}", serde_json::to_string_pretty(&report)?);
  } else {
    println!("Balatro setup {:?}: deck atlas {}", report.status, report.deck_atlas_path.display());
  }
  Ok(())
}

fn setup_balatro_assets(args: &SetupArgs) -> Result<SetupReport, CliError> {
  let cache_dir = setup_cache_dir(args.cache_dir.as_deref())?;
  let deck_atlas_path = cache_dir.join(DECK_ATLAS_CACHE_FILE);
  let manifest_path = cache_dir.join(SETUP_MANIFEST_FILE);

  if args.check {
    let deck_atlas_sha256 = if deck_atlas_path.exists() {
      validate_deck_atlas_path(&deck_atlas_path)?;
      Some(sha256_file(&deck_atlas_path)?)
    } else {
      return Err(CliError::Message(format!(
        "Balatro setup cache is missing {}; run `auv-game-balatro setup` first",
        deck_atlas_path.display()
      )));
    };
    return Ok(SetupReport {
      schema_version: SETUP_MANIFEST_SCHEMA_VERSION.to_string(),
      status: SetupStatus::Ready,
      cache_dir,
      deck_atlas_path,
      manifest_path,
      source_love_path: None,
      deck_atlas_sha256,
    });
  }

  if deck_atlas_path.exists() && !args.force {
    validate_deck_atlas_path(&deck_atlas_path)?;
    return Ok(SetupReport {
      schema_version: SETUP_MANIFEST_SCHEMA_VERSION.to_string(),
      status: SetupStatus::Reused,
      cache_dir,
      deck_atlas_path: deck_atlas_path.clone(),
      manifest_path,
      source_love_path: None,
      deck_atlas_sha256: Some(sha256_file(&deck_atlas_path)?),
    });
  }

  let love_path = resolve_setup_love_path(args)?;
  let atlas_bytes = extract_deck_atlas_from_love(&love_path)?;
  image::load_from_memory(&atlas_bytes)?;
  fs::create_dir_all(&cache_dir)?;
  fs::write(&deck_atlas_path, &atlas_bytes)?;
  let deck_atlas_sha256 = sha256_bytes(&atlas_bytes);
  let manifest = SetupManifest {
    schema_version: SETUP_MANIFEST_SCHEMA_VERSION.to_string(),
    source_love_path: love_path.clone(),
    deck_atlas_path: deck_atlas_path.clone(),
    deck_atlas_sha256: deck_atlas_sha256.clone(),
    extracted_at_ms: now_millis(),
  };
  fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)? + "\n")?;

  Ok(SetupReport {
    schema_version: SETUP_MANIFEST_SCHEMA_VERSION.to_string(),
    status: SetupStatus::Extracted,
    cache_dir,
    deck_atlas_path,
    manifest_path,
    source_love_path: Some(love_path),
    deck_atlas_sha256: Some(deck_atlas_sha256),
  })
}

fn setup_cache_dir(explicit: Option<&Path>) -> Result<PathBuf, CliError> {
  if let Some(path) = explicit {
    return Ok(path.to_path_buf());
  }
  if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
    return Ok(home.join(".cache").join("auv").join("game-balatro"));
  }
  Err(CliError::Message("could not resolve Balatro setup cache directory; pass --cache-dir".to_string()))
}

fn resolve_setup_love_path(args: &SetupArgs) -> Result<PathBuf, CliError> {
  if let Some(path) = args.love.as_deref() {
    return require_love_path(path);
  }
  if let Some(app) = args.app.as_deref() {
    return require_love_path(&love_path_from_app(app));
  }
  if let Some(path) = discover_steam_love_path() {
    return require_love_path(&path);
  }
  Err(CliError::Message("could not find Balatro.love; pass --love <path> or --app <Balatro.app>".to_string()))
}

fn require_love_path(path: &Path) -> Result<PathBuf, CliError> {
  if path.exists() {
    Ok(path.to_path_buf())
  } else {
    Err(CliError::Message(format!("Balatro.love does not exist: {}", path.display())))
  }
}

fn love_path_from_app(app: &Path) -> PathBuf {
  app.join("Contents").join("Resources").join("Balatro.love")
}

fn discover_steam_love_path() -> Option<PathBuf> {
  let home = std::env::var_os("HOME").map(PathBuf::from)?;
  let path = home
    .join("Library")
    .join("Application Support")
    .join("Steam")
    .join("steamapps")
    .join("common")
    .join("Balatro")
    .join("Balatro.app")
    .join("Contents")
    .join("Resources")
    .join("Balatro.love");
  path.exists().then_some(path)
}

fn extract_deck_atlas_from_love(love_path: &Path) -> Result<Vec<u8>, CliError> {
  let output = ProcessCommand::new("unzip").arg("-p").arg(love_path).arg(DECK_ATLAS_LOVE_PATH).output()?;
  if !output.status.success() {
    return Err(CliError::Message(format!("failed to extract {DECK_ATLAS_LOVE_PATH} from {}", love_path.display())));
  }
  Ok(output.stdout)
}

fn validate_deck_atlas_path(path: &Path) -> Result<(), CliError> {
  image::open(path)?;
  Ok(())
}

fn sha256_file(path: &Path) -> Result<String, CliError> {
  Ok(sha256_bytes(&fs::read(path)?))
}

fn sha256_bytes(bytes: &[u8]) -> String {
  let mut hasher = Sha256::new();
  hasher.update(bytes);
  format!("{:x}", hasher.finalize())
}

fn now_millis() -> u128 {
  SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_millis()).unwrap_or_default()
}

fn click_store_reroll(_args: OperationControlArgs) -> Result<(), CliError> {
  deferred("store.reroll", "store reroll input is implemented after store buy")
}

#[cfg(target_os = "macos")]
fn click_joker_sell(args: SlotOperationArgs) -> Result<(), CliError> {
  let slot_index = parse_joker_slot_index(&args.slot)?;
  let result = object_sell(ObjectSellRequest {
    target: args.control.target,
    slot: SlotId::new(ObjectZone::Joker, slot_index),
    confirm_sale: args.control.verify && args.control.verify_mode != VerifyModeArg::ActivationOnly,
    timeout_ms: args.control.timeout_ms.unwrap_or(1000),
  })?;
  write_object_sell_output("jokers.sell", &result)
}

#[cfg(not(target_os = "macos"))]
fn click_joker_sell(args: SlotOperationArgs) -> Result<(), CliError> {
  parse_joker_slot_index(&args.slot)?;
  Err(CliError::Message("jokers sell live operation is only available on macOS".to_string()))
}

#[cfg(target_os = "macos")]
fn click_consumable_sell(args: SlotOperationArgs) -> Result<(), CliError> {
  let slot_index = parse_consumable_slot_index(&args.slot)?;
  let result = object_sell(ObjectSellRequest {
    target: args.control.target,
    slot: SlotId::new(ObjectZone::Consumable, slot_index),
    confirm_sale: args.control.verify && args.control.verify_mode != VerifyModeArg::ActivationOnly,
    timeout_ms: args.control.timeout_ms.unwrap_or(1000),
  })?;
  write_object_sell_output("consumables.sell", &result)
}

#[cfg(not(target_os = "macos"))]
fn click_consumable_sell(args: SlotOperationArgs) -> Result<(), CliError> {
  parse_consumable_slot_index(&args.slot)?;
  Err(CliError::Message("consumables sell live operation is only available on macOS".to_string()))
}

#[cfg(target_os = "macos")]
pub fn object_sell(request: ObjectSellRequest) -> Result<ObjectSellResult, CliError> {
  if !matches!(request.slot.zone, ObjectZone::Joker | ObjectZone::Consumable) {
    return Err(CliError::Message(format!("object sell requires a joker or consumable slot, got {}", request.slot)));
  }
  let session = open_macos_session()?;
  let window = session.window().resolve(Window::main_visible().owned_by(App::name(request.target.clone())))?;
  let before_image = capture_window_to_temp(&session, &window, "object-sell-before")?;
  // TODO(balatro-object-sell-artifacts): emit captures through auv-tracing
  // once the shared in-memory capture encoder is owner-approved.
  let before = observe_image(&before_image, &BalatroModelConfig::default(), true);
  let _ = fs::remove_file(before_image);
  let before = before?;
  let (object, object_point) = match request.slot.zone {
    ObjectZone::Joker => {
      let joker = select_joker(&before, request.slot.index)?.clone();
      let point = window_point_from_joker(&before, &window, &joker);
      (SellableObject::Joker { joker }, point)
    }
    ObjectZone::Consumable => {
      let consumable = select_consumable(&before, request.slot.index)?.clone();
      let point = window_point_from_consumable(&before, &window, &consumable);
      (SellableObject::Consumable { consumable }, point)
    }
    _ => unreachable!("slot zone validated above"),
  };
  let selection = ObjectSellClick {
    window_point: WindowPoint::new(object_point.x, object_point.y),
    delivery: click_game_point(&session, &window, object_point)?,
  };

  std::thread::sleep(Duration::from_millis(500));
  let selected = match capture_window_to_temp(&session, &window, "object-sell-selected") {
    Ok(image) => {
      let selected = observe_image(&image, &BalatroModelConfig::default(), true).map_err(|error| error.to_string());
      let _ = fs::remove_file(image);
      selected
    }
    Err(error) => Err(error.to_string()),
  };
  let selected = match selected {
    Ok(selected) => selected,
    Err(message) => {
      let result = ObjectSellResult {
        target: request.target,
        slot: request.slot,
        object,
        outcome: ObjectSellOutcome::SelectionOnly {
          selection,
          reason: ObjectSellIncompleteReason::StateReadFailed { message },
        },
      };
      emit_object_sell_completed(&result);
      return Ok(result);
    }
  };
  let sell_button = match find_button(&selected, "button_sell") {
    Ok(button) => button.clone(),
    Err(_) => {
      let result = ObjectSellResult {
        target: request.target,
        slot: request.slot,
        object,
        outcome: ObjectSellOutcome::SelectionOnly {
          selection,
          reason: ObjectSellIncompleteReason::SellControlNotFound,
        },
      };
      emit_object_sell_completed(&result);
      return Ok(result);
    }
  };
  let sell_point = window_point_from_button(&selected, &window, &sell_button);
  let submission = ObjectSellClick {
    window_point: WindowPoint::new(sell_point.x, sell_point.y),
    delivery: click_game_point(&session, &window, sell_point)?,
  };

  let confirmation = if request.confirm_sale {
    match capture_observable_window(&session, &window, "object-sell-after", request.timeout_ms, 500) {
      Ok((image, after)) => {
        let confirmation = evaluate_object_sell_confirmation(request.slot.zone, &before, after.as_ref().map_err(|error| error.to_string()));
        let _ = fs::remove_file(image);
        confirmation
      }
      Err(error) => evaluate_object_sell_confirmation(request.slot.zone, &before, Err(error.to_string())),
    }
  } else {
    ObjectSellConfirmation::NotRequested
  };

  let result = ObjectSellResult {
    target: request.target,
    slot: request.slot,
    object,
    outcome: ObjectSellOutcome::Submitted {
      selection,
      sell_button,
      submission,
      confirmation,
    },
  };
  emit_object_sell_completed(&result);
  Ok(result)
}

#[cfg(not(target_os = "macos"))]
pub fn object_sell(_request: ObjectSellRequest) -> Result<ObjectSellResult, CliError> {
  Err(CliError::Message("object sell live operation is only available on macOS".to_string()))
}

#[derive(Debug, Serialize)]
struct ObjectSellCliOutput<'a> {
  operation: &'static str,
  target: &'a str,
  slot: SlotId,
  object: &'a SellableObject,
  outcome: &'a ObjectSellOutcome,
}

fn write_object_sell_output(operation: &'static str, result: &ObjectSellResult) -> Result<(), CliError> {
  write_output(
    OutputMode::Json,
    &ObjectSellCliOutput {
      operation,
      target: &result.target,
      slot: result.slot,
      object: &result.object,
      outcome: &result.outcome,
    },
  )
}

fn deferred(command: &'static str, reason: &'static str) -> Result<(), CliError> {
  Err(CliError::Deferred { command, reason })
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct CardReadResult {
  slot: SlotId,
  bbox: BoundingBox,
  confidence: f32,
  reading: CardReadValue,
  evidence: CardReadEvidence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ObjectReadZone {
  Store,
  Joker,
  Consumable,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct ObjectReadResult {
  slot: SlotId,
  kind: String,
  bbox: BoundingBox,
  confidence: f32,
  reading: ObjectReadValue,
  evidence: ObjectReadEvidence,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct ObjectReadValue {
  status: &'static str,
  raw_text: Option<String>,
  confidence: Option<f32>,
}

impl ObjectReadValue {
  fn unread() -> Self {
    Self {
      status: "unread",
      raw_text: None,
      confidence: None,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct ObjectReadEvidence {
  frame: String,
  source: String,
  hover_required: bool,
  #[serde(skip_serializing_if = "Option::is_none")]
  hover_frame: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  hover_ocr_region: Option<RatioRect>,
  #[serde(skip_serializing_if = "Option::is_none")]
  hover_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct PackReadChoice {
  #[serde(flatten)]
  choice: PackChoice,
  hint: String,
  hover_required: bool,
  #[serde(skip_serializing_if = "Option::is_none")]
  hover_text: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  hover_frame: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  hover_ocr_region: Option<RatioRect>,
  #[serde(skip_serializing_if = "Option::is_none")]
  hover_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct PackReadOutput {
  phase: BalatroPhase,
  choices: Vec<PackReadChoice>,
  skip_button: Option<ButtonTarget>,
  frame: crate::model::FrameRef,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct CardReadValue {
  status: &'static str,
  raw_text: Option<String>,
  normalized_text: Option<String>,
  rank: Option<String>,
  rank_candidate: Option<String>,
  rank_candidate_confidence: Option<f32>,
  suit: Option<String>,
  suit_symbol: Option<String>,
  short_code: Option<String>,
  confidence: Option<f32>,
  valid: bool,
}

impl CardReadValue {
  #[cfg(not(target_os = "macos"))]
  fn unread() -> Self {
    Self {
      status: "unread",
      raw_text: None,
      normalized_text: None,
      rank: None,
      rank_candidate: None,
      rank_candidate_confidence: None,
      suit: None,
      suit_symbol: None,
      short_code: None,
      confidence: None,
      valid: false,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct CardReadEvidence {
  frame: String,
  ocr_region: RatioRect,
  corner_crop: Option<PathBuf>,
  source: String,
}

fn write_observed_state(args: &ObserveArgs) -> Result<(), CliError> {
  let state = observe_from_args(args)?;
  // TODO(balatro-state-human-output): the default renderer still prints Rust
  // Debug output. Add an app-owned table only after its stable summary columns
  // are owner-approved; see `2026-08-05-linux-live-gameplay-evidence.md`.
  write_output(args.output_mode(), &state)
}

fn write_scores(args: &ObserveArgs) -> Result<(), CliError> {
  let state = observe_from_args(args)?;
  write_output(args.output_mode(), &state.scores)
}

fn write_rounds(args: &ObserveArgs) -> Result<(), CliError> {
  let state = observe_from_args(args)?;
  write_output(args.output_mode(), &state.rounds)
}

fn write_jokers(args: &ObserveArgs) -> Result<(), CliError> {
  let mut state = observe_from_args(args)?;
  if args.image.is_none() && std::env::var_os("AUV_CONTEXT").is_some() {
    hover_read_jokers_via_api(&mut state)?;
  } else {
    // TODO(balatro-local-joker-auto-read): batch local hover OCR after the
    // app-owned operation can retain one native session across list output;
    // the current owner-approved slice is the routed Linux gameplay path.
  }
  write_output(args.output_mode(), &state.jokers)
}

fn write_card_read(args: &SlotObserveArgs) -> Result<(), CliError> {
  let reads = read_cards_from_args(args)?;
  write_output(args.observe.output_mode(), &reads)
}

fn write_object_read(args: &SlotObserveArgs, zone: ObjectReadZone) -> Result<(), CliError> {
  if args.observe.image.is_none() {
    let read = read_object_live(args, zone)?;
    return write_output(args.observe.output_mode(), &read);
  }

  let state = observe_from_args(&args.observe)?;
  let read = object_read_from_state(&state, &args.slot, zone)?;
  write_output(args.observe.output_mode(), &read)
}

fn write_store_status(args: &ObserveArgs) -> Result<(), CliError> {
  let state = observe_from_args(args)?;
  write_output(args.output_mode(), &state.store)
}

fn write_store_items(args: &ObserveArgs) -> Result<(), CliError> {
  let state = observe_from_args(args)?;
  write_output(args.output_mode(), &state.store.items)
}

fn write_blind_buttons(args: &ObserveArgs) -> Result<(), CliError> {
  let state = observe_from_args(args)?;
  let buttons = blind_buttons(&state);
  write_output(args.output_mode(), &buttons)
}

fn write_pack_read(args: &ObserveArgs) -> Result<(), CliError> {
  #[cfg(target_os = "macos")]
  if args.image.is_none() {
    let output = read_pack_live(args)?;
    return write_output(args.output_mode(), &output);
  }

  let state = observe_from_args(args)?;
  let choices = active_pack_choices(&state);
  write_output(
    args.output_mode(),
    &json!({
      "phase": state.phase,
      "choices": choices,
      "skip_button": best_button(&state.buttons, "button_card_pack_skip"),
      "frame": state.frame,
    }),
  )
}

#[cfg(target_os = "macos")]
fn click_game_cash_out(args: OperationControlArgs) -> Result<(), CliError> {
  let config = BalatroModelConfig::from_operation_args(&args);
  let details = args.details;
  let confirmation = if !args.verify || args.verify_mode == VerifyModeArg::ActivationOnly {
    CashOutConfirmationRequest::None
  } else if args.verify_mode == VerifyModeArg::Targeted {
    CashOutConfirmationRequest::Targeted
  } else {
    CashOutConfirmationRequest::Weak
  };
  let request = CashOutRequest {
    target: args.target,
    confirmation,
    timeout_ms: args.timeout_ms.unwrap_or(1200),
  };
  let result = if std::env::var_os("AUV_CONTEXT").is_some() {
    cash_out_via_api(request, config)?
  } else {
    cash_out(request)?
  };
  write_cash_out_output(details, &result)
}

#[cfg(not(target_os = "macos"))]
fn click_game_cash_out(args: OperationControlArgs) -> Result<(), CliError> {
  if std::env::var_os("AUV_CONTEXT").is_none() {
    return Err(CliError::Message("game cash-out requires macOS locally or an inherited AUV device context".to_string()));
  }
  let config = BalatroModelConfig::from_operation_args(&args);
  let confirmation = if !args.verify || args.verify_mode == VerifyModeArg::ActivationOnly {
    CashOutConfirmationRequest::None
  } else if args.verify_mode == VerifyModeArg::Targeted {
    CashOutConfirmationRequest::Targeted
  } else {
    CashOutConfirmationRequest::Weak
  };
  let details = args.details;
  let result = cash_out_via_api(
    CashOutRequest {
      target: args.target,
      confirmation,
      timeout_ms: args.timeout_ms.unwrap_or(1200),
    },
    config,
  )?;
  write_cash_out_output(details, &result)
}

#[cfg(target_os = "macos")]
pub fn cash_out(request: CashOutRequest) -> Result<CashOutResult, CliError> {
  let session = open_macos_session()?;
  let window = session.window().resolve(Window::main_visible().owned_by(App::name(request.target.clone())))?;
  let before_image = capture_window_to_temp(&session, &window, "game-cash-out-before")?;
  // TODO(balatro-cash-out-artifacts): emit the before/after captures through
  // auv-tracing once the shared in-memory capture encoder is owner-approved.
  let before = observe_image(&before_image, &BalatroModelConfig::default(), true);
  let _ = fs::remove_file(&before_image);
  let before = before?;
  let selected_button = find_button(&before, "button_cash_out")?.clone();
  let point = window_point_from_button(&before, &window, &selected_button);
  let delivery = click_game_point(&session, &window, point)?;

  let confirmation = if request.confirmation == CashOutConfirmationRequest::None {
    CashOutConfirmation::NotRequested
  } else {
    let (after_image, after) = capture_observable_window(&session, &window, "game-cash-out-after", request.timeout_ms, 500)?;
    let confirmation = evaluate_cash_out_confirmation(request.confirmation, &before, after.as_ref().map_err(|error| error.to_string()));
    let _ = fs::remove_file(after_image);
    confirmation
  };

  let result = CashOutResult {
    target: request.target,
    attempts: vec![CashOutAttempt {
      selected_button,
      point: ActionPoint::window(window.reference.id.clone(), WindowPoint::new(point.x, point.y)),
      delivery,
    }],
    confirmation,
  };
  emit_cash_out_completed(&result);
  Ok(result)
}

#[cfg(not(target_os = "macos"))]
pub fn cash_out(_request: CashOutRequest) -> Result<CashOutResult, CliError> {
  Err(CliError::Message("game cash-out live operation is only available on macOS".to_string()))
}

fn cash_out_via_api(request: CashOutRequest, config: BalatroModelConfig) -> Result<CashOutResult, CliError> {
  let runtime = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .map_err(|error| CliError::Message(format!("failed to create daemon client runtime: {error}")))?;
  let before = runtime.block_on(observe_live_via_api(&request.target, &config, None, true))?;
  let mut current = before.clone();
  let mut attempts = Vec::new();
  let mut confirmation = CashOutConfirmation::NotRequested;
  let attempt_limit = if request.confirmation == CashOutConfirmationRequest::None {
    1
  } else {
    2
  };
  for attempt in 0..attempt_limit {
    let selected_button = find_button(&current, "button_cash_out")?.clone();
    let (screen_point, delivery) =
      runtime.block_on(click_display_frame_point_via_api(&current.frame, bbox_center_point(selected_button.bbox)))?;
    attempts.push(CashOutAttempt {
      selected_button,
      point: ActionPoint::screen(screen_point),
      delivery,
    });
    if request.confirmation == CashOutConfirmationRequest::None {
      break;
    }
    let settle_deadline = Instant::now() + Duration::from_millis((request.timeout_ms / attempt_limit as u64).max(350));
    loop {
      std::thread::sleep(Duration::from_millis(250));
      match runtime.block_on(observe_live_via_api(&request.target, &config, None, true)) {
        Ok(after) => {
          confirmation = evaluate_cash_out_confirmation(request.confirmation, &before, Ok(&after));
          current = after;
          if matches!(confirmation, CashOutConfirmation::Confirmed { .. }) || Instant::now() >= settle_deadline {
            break;
          }
        }
        Err(error) => {
          confirmation = evaluate_cash_out_confirmation(request.confirmation, &before, Err(error.to_string()));
          break;
        }
      }
    }
    if matches!(confirmation, CashOutConfirmation::Confirmed { .. }) {
      break;
    }
    if attempt + 1 < attempt_limit {
      std::thread::sleep(Duration::from_millis(250));
    }
  }

  let result = CashOutResult {
    target: request.target,
    attempts,
    confirmation,
  };
  emit_cash_out_completed(&result);
  Ok(result)
}

#[cfg(feature = "tracing")]
fn emit_input_delivery(delivery: &auv_driver::InputActionResult) {
  crate::run_read::emit_json_artifact(auv_driver::INPUT_ACTION_RESULT_PURPOSE, delivery);
}

#[cfg(not(feature = "tracing"))]
fn emit_input_delivery(_delivery: &auv_driver::InputActionResult) {}

#[derive(Debug, Serialize)]
struct CashOutCliOutput<'a> {
  operation: &'static str,
  target: &'a str,
  confirmation: &'a CashOutConfirmation,
  attempts: &'a [CashOutAttempt],
}

fn write_cash_out_output(_details: bool, result: &CashOutResult) -> Result<(), CliError> {
  write_output(
    OutputMode::Json,
    &CashOutCliOutput {
      operation: "game.cash_out",
      target: &result.target,
      confirmation: &result.confirmation,
      attempts: &result.attempts,
    },
  )
}

#[cfg(target_os = "macos")]
fn click_game_restart(args: OperationControlArgs) -> Result<(), CliError> {
  if std::env::var_os("AUV_CONTEXT").is_some() {
    let config = BalatroModelConfig::from_operation_args(&args);
    return write_game_restart_output(&game_restart_via_api(
      GameRestartRequest {
        target: args.target,
        confirm_started: args.verify && args.verify_mode != VerifyModeArg::ActivationOnly,
        timeout_ms: args.timeout_ms.unwrap_or(1800),
      },
      config,
    )?);
  }
  let result = game_restart(GameRestartRequest {
    target: args.target,
    confirm_started: args.verify && args.verify_mode != VerifyModeArg::ActivationOnly,
    timeout_ms: args.timeout_ms.unwrap_or(1800),
  })?;
  write_game_restart_output(&result)
}

#[cfg(target_os = "macos")]
pub fn game_restart(request: GameRestartRequest) -> Result<GameRestartResult, CliError> {
  let session = open_macos_session()?;
  let window = session.window().resolve(Window::main_visible().owned_by(App::name(request.target.clone())))?;
  let before_image = capture_window_to_temp(&session, &window, "game-restart-before")?;
  // TODO(balatro-game-restart-artifacts): emit captures through auv-tracing
  // once the shared in-memory capture encoder is owner-approved.
  let before = observe_image(&before_image, &BalatroModelConfig::default(), true).ok();
  let _ = fs::remove_file(before_image);
  let (first_target, first_point) =
    match before.as_ref().and_then(|state| restart_primary_button(&state.buttons).map(|button| (state, button))) {
      Some((state, button)) => (
        GameRestartTarget::DetectedButton {
          button: button.clone(),
        },
        window_point_from_button(state, &window, button),
      ),
      None => {
        // TODO(balatro-game-over-ui-v1): replace this layout target with a
        // detector-backed button once game-over controls are in the UI dataset.
        let y = if before.is_none() { 0.805 } else { 0.815 };
        (GameRestartTarget::GameOverLayout, normalized_window_point(&window, 0.62, y))
      }
    };
  let first_is_layout = matches!(first_target, GameRestartTarget::GameOverLayout);
  let mut clicks = Vec::new();
  deliver_game_restart_click(&session, &window, first_target, first_point, &mut clicks)?;
  if first_is_layout {
    std::thread::sleep(Duration::from_millis(300));
    deliver_game_restart_click(&session, &window, GameRestartTarget::GameOverLayout, first_point, &mut clicks)?;
  }

  std::thread::sleep(Duration::from_millis(900));
  let intermediate = match capture_window_to_temp(&session, &window, "game-restart-intermediate") {
    Ok(image) => {
      let state = observe_image(&image, &BalatroModelConfig::default(), true).map_err(|error| error.to_string());
      let _ = fs::remove_file(image);
      state
    }
    Err(error) => Err(error.to_string()),
  };
  match intermediate {
    Ok(intermediate) => {
      if let Some(button) = restart_primary_button(&intermediate.buttons) {
        let target = GameRestartTarget::DetectedButton {
          button: button.clone(),
        };
        let point = window_point_from_button(&intermediate, &window, button);
        deliver_game_restart_click(&session, &window, target, point, &mut clicks)?;
      }
    }
    Err(_) if first_is_layout => {
      // NOTICE: An unreadable intermediate frame after the game-over layout
      // click may be the older localized title screen.
      let point = normalized_window_point(&window, 0.31, 0.84);
      deliver_game_restart_click(&session, &window, GameRestartTarget::LocalizedTitleLayout, point, &mut clicks)?;
    }
    Err(_) => {}
  }

  let outcome = if request.confirm_started {
    let read_after = |label: &str| match capture_observable_window(&session, &window, label, request.timeout_ms, 700) {
      Ok((image, state)) => {
        let state = state.map_err(|error| error.to_string());
        let _ = fs::remove_file(image);
        state
      }
      Err(error) => Err(error.to_string()),
    };
    let mut after = read_after("game-restart-after");
    let late_button = after.as_ref().ok().and_then(|state| {
      (state.phase == BalatroPhase::MainMenu).then(|| restart_primary_button(&state.buttons).map(|button| (state, button))).flatten()
    });
    if let Some((state, button)) = late_button {
      let target = GameRestartTarget::DetectedButton {
        button: button.clone(),
      };
      let point = window_point_from_button(state, &window, button);
      deliver_game_restart_click(&session, &window, target, point, &mut clicks)?;
      after = read_after("game-restart-after-late-control");
    }
    classify_game_restart_outcome(after.as_ref().map_err(Clone::clone))
  } else {
    GameRestartOutcome::NotChecked
  };

  let result = GameRestartResult {
    target: request.target,
    clicks,
    outcome,
  };
  emit_game_restart_completed(&result);
  Ok(result)
}

#[cfg(not(target_os = "macos"))]
fn click_game_restart(args: OperationControlArgs) -> Result<(), CliError> {
  if std::env::var_os("AUV_CONTEXT").is_some() {
    let config = BalatroModelConfig::from_operation_args(&args);
    return write_game_restart_output(&game_restart_via_api(
      GameRestartRequest {
        target: args.target,
        confirm_started: args.verify && args.verify_mode != VerifyModeArg::ActivationOnly,
        timeout_ms: args.timeout_ms.unwrap_or(1800),
      },
      config,
    )?);
  }
  Err(CliError::Message("game restart live operation requires macOS locally or an inherited AUV device context".to_string()))
}

#[cfg(not(target_os = "macos"))]
pub fn game_restart(_request: GameRestartRequest) -> Result<GameRestartResult, CliError> {
  Err(CliError::Message("game restart live operation is only available on macOS".to_string()))
}

#[cfg(target_os = "macos")]
fn deliver_game_restart_click(
  session: &auv_driver_macos::MacosDriverSession,
  window: &auv_driver::window::Window,
  target: GameRestartTarget,
  point: Point,
  clicks: &mut Vec<GameRestartClick>,
) -> Result<(), CliError> {
  let delivery = click_game_point(session, window, point)?;
  clicks.push(GameRestartClick {
    target,
    point: ActionPoint::window(window.reference.id.clone(), WindowPoint::new(point.x, point.y)),
    delivery,
  });
  Ok(())
}

fn game_restart_via_api(request: GameRestartRequest, config: BalatroModelConfig) -> Result<GameRestartResult, CliError> {
  let runtime = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .map_err(|error| CliError::Message(format!("failed to create daemon client runtime: {error}")))?;
  let mut state = observe_restart_state_via_api(&runtime, &request.target, &config, RestartObservation::AnyControl)?;
  let mut clicks = Vec::new();

  for _ in 0..6 {
    let Some(button) = restart_primary_button(&state.buttons).cloned() else {
      state = observe_restart_state_via_api(&runtime, &request.target, &config, RestartObservation::AnyControl)?;
      continue;
    };
    let opens_new_run_modal = button.id == "button_main_menu_play";
    if button.id == "button_new_run_play" {
      // TODO(balatro-new-run-tab-detector): replace this layout point after the
      // New Run/Continue/Challenge tabs are represented in the UI dataset.
      // Selecting New Run is required because Continue may hold a stale run.
      let tab_point = Point::new(f64::from(state.frame.image_size.width) * 0.505, f64::from(state.frame.image_size.height) * 0.333);
      let (screen_point, delivery) = runtime.block_on(click_display_frame_point_via_api(&state.frame, tab_point))?;
      clicks.push(GameRestartClick {
        target: GameRestartTarget::NewRunTabLayout,
        point: ActionPoint::screen(screen_point),
        delivery,
      });
      // The modal is translucent, so a fresh detector frame can momentarily
      // prefer the occluded main-menu button. The modal geometry does not move
      // when changing tabs; retain this frame's detector-backed Play target.
      std::thread::sleep(Duration::from_millis(350));
    }
    let target = GameRestartTarget::DetectedButton {
      button: button.clone(),
    };
    let point = bbox_center_point(button.bbox);
    let (screen_point, delivery) = runtime.block_on(click_display_frame_point_via_api(&state.frame, point))?;
    clicks.push(GameRestartClick {
      target,
      point: ActionPoint::screen(screen_point),
      delivery,
    });
    std::thread::sleep(Duration::from_millis(900));
    if opens_new_run_modal {
      // TODO(balatro-new-run-modal-layout): replace both modal layout points
      // after tab and Play recognition are reliable on translucent frames.
      let tab_point = Point::new(f64::from(state.frame.image_size.width) * 0.505, f64::from(state.frame.image_size.height) * 0.333);
      let (screen_point, delivery) = runtime.block_on(click_display_frame_point_via_api(&state.frame, tab_point))?;
      clicks.push(GameRestartClick {
        target: GameRestartTarget::NewRunTabLayout,
        point: ActionPoint::screen(screen_point),
        delivery,
      });
      std::thread::sleep(Duration::from_millis(350));
      let play_point = Point::new(f64::from(state.frame.image_size.width) * 0.593, f64::from(state.frame.image_size.height) * 0.823);
      let (screen_point, delivery) = runtime.block_on(click_display_frame_point_via_api(&state.frame, play_point))?;
      clicks.push(GameRestartClick {
        target: GameRestartTarget::NewRunPlayLayout,
        point: ActionPoint::screen(screen_point),
        delivery,
      });
      std::thread::sleep(Duration::from_millis(900));
    }
    let expected = if opens_new_run_modal {
      RestartObservation::Started
    } else {
      RestartObservation::Started
    };
    state = observe_restart_state_via_api(&runtime, &request.target, &config, expected)?;
    if matches!(state.phase, BalatroPhase::BlindSelect | BalatroPhase::Playing | BalatroPhase::Store) {
      break;
    }
  }

  let outcome = if request.confirm_started {
    classify_game_restart_outcome(Ok(&state))
  } else {
    GameRestartOutcome::NotChecked
  };
  let result = GameRestartResult {
    target: request.target,
    clicks,
    outcome,
  };
  emit_game_restart_completed(&result);
  Ok(result)
}

#[derive(Clone, Copy)]
enum RestartObservation {
  AnyControl,
  Started,
}

fn observe_restart_state_via_api(
  runtime: &tokio::runtime::Runtime,
  target: &str,
  config: &BalatroModelConfig,
  expected: RestartObservation,
) -> Result<BalatroState, CliError> {
  let mut last = None;
  for attempt in 0..3 {
    let state = runtime.block_on(observe_live_via_api(target, config, None, true))?;
    let started = matches!(state.phase, BalatroPhase::BlindSelect | BalatroPhase::Playing | BalatroPhase::Store);
    let matches_expected = match expected {
      RestartObservation::AnyControl => restart_primary_button(&state.buttons).is_some() || started,
      RestartObservation::Started => started,
    };
    if matches_expected {
      return Ok(state);
    }
    last = Some(state);
    if attempt < 2 {
      std::thread::sleep(Duration::from_millis(250));
    }
  }
  Ok(last.expect("bounded restart observation loop always records a state"))
}

#[derive(Debug, Serialize)]
struct GameRestartCliOutput<'a> {
  operation: &'static str,
  target: &'a str,
  clicks: &'a [GameRestartClick],
  outcome: &'a GameRestartOutcome,
}

fn write_game_restart_output(result: &GameRestartResult) -> Result<(), CliError> {
  write_output(
    OutputMode::Json,
    &GameRestartCliOutput {
      operation: "game.restart",
      target: &result.target,
      clicks: &result.clicks,
      outcome: &result.outcome,
    },
  )
}

#[cfg(target_os = "macos")]
fn click_store_buy(args: SlotOperationArgs) -> Result<(), CliError> {
  let slot_index = parse_store_slot_index(&args.slot)?;
  let config = BalatroModelConfig::from_operation_args(&args.control);
  let request = StoreBuyRequest {
    target: args.control.target,
    slot: SlotId::new(ObjectZone::Store, slot_index),
    confirm_purchase: args.control.verify && args.control.verify_mode != VerifyModeArg::ActivationOnly,
    timeout_ms: args.control.timeout_ms.unwrap_or(1000),
  };
  let result = if std::env::var_os("AUV_CONTEXT").is_some() {
    store_buy_via_api(request, config)?
  } else {
    store_buy(request)?
  };
  write_store_buy_output(&result)
}

#[cfg(target_os = "macos")]
pub fn store_buy(request: StoreBuyRequest) -> Result<StoreBuyResult, CliError> {
  if request.slot.zone != ObjectZone::Store {
    return Err(CliError::Message(format!("store buy requires a store slot, got {}", request.slot)));
  }
  let session = open_macos_session()?;
  let window = session.window().resolve(Window::main_visible().owned_by(App::name(request.target.clone())))?;
  let before_image = capture_window_to_temp(&session, &window, "store-buy-before")?;
  // TODO(balatro-store-buy-artifacts): emit captures through auv-tracing once
  // the shared in-memory capture encoder is owner-approved.
  let before = observe_image(&before_image, &BalatroModelConfig::default(), true);
  let _ = fs::remove_file(before_image);
  let before = before?;
  let item = select_store_item(&before, request.slot.index)?.clone();
  let item_point = window_point_from_store_item(&before, &window, &item);
  let selection = StoreBuyClick {
    point: ActionPoint::window(window.reference.id.clone(), WindowPoint::new(item_point.x, item_point.y)),
    delivery: click_game_point(&session, &window, item_point)?,
  };

  std::thread::sleep(Duration::from_millis(500));
  let selected = match capture_window_to_temp(&session, &window, "store-buy-selected") {
    Ok(image) => {
      let selected = observe_image(&image, &BalatroModelConfig::default(), true).map_err(|error| error.to_string());
      let _ = fs::remove_file(image);
      selected
    }
    Err(error) => Err(error.to_string()),
  };
  let selected = match selected {
    Ok(selected) => selected,
    Err(message) => {
      let result = StoreBuyResult {
        target: request.target,
        slot: request.slot,
        item,
        outcome: StoreBuyOutcome::SelectionOnly {
          selection,
          reason: StoreBuyIncompleteReason::StateReadFailed { message },
        },
      };
      emit_store_buy_completed(&result);
      return Ok(result);
    }
  };
  let confirmation_button = match select_store_buy_confirm_button(&selected) {
    Ok(button) => button.clone(),
    Err(_) => {
      let result = StoreBuyResult {
        target: request.target,
        slot: request.slot,
        item,
        outcome: StoreBuyOutcome::SelectionOnly {
          selection,
          reason: StoreBuyIncompleteReason::PurchaseControlNotFound,
        },
      };
      emit_store_buy_completed(&result);
      return Ok(result);
    }
  };
  let confirm_point = window_point_from_button(&selected, &window, &confirmation_button);
  let submission = StoreBuyClick {
    point: ActionPoint::window(window.reference.id.clone(), WindowPoint::new(confirm_point.x, confirm_point.y)),
    delivery: click_game_point(&session, &window, confirm_point)?,
  };

  let confirmation = if request.confirm_purchase {
    match capture_observable_window(&session, &window, "store-buy-after", request.timeout_ms, 500) {
      Ok((image, after)) => {
        let confirmation = evaluate_store_buy_confirmation(&before, after.as_ref().map_err(|error| error.to_string()));
        let _ = fs::remove_file(image);
        confirmation
      }
      Err(error) => evaluate_store_buy_confirmation(&before, Err(error.to_string())),
    }
  } else {
    StoreBuyConfirmation::NotRequested
  };

  let result = StoreBuyResult {
    target: request.target,
    slot: request.slot,
    item,
    outcome: StoreBuyOutcome::Submitted {
      selection,
      confirmation_button,
      submission,
      confirmation,
    },
  };
  emit_store_buy_completed(&result);
  Ok(result)
}

#[cfg(not(target_os = "macos"))]
fn click_store_buy(args: SlotOperationArgs) -> Result<(), CliError> {
  if std::env::var_os("AUV_CONTEXT").is_none() {
    return Err(CliError::Message("store buy requires macOS locally or an inherited AUV device context".to_string()));
  }
  let slot_index = parse_store_slot_index(&args.slot)?;
  let config = BalatroModelConfig::from_operation_args(&args.control);
  let result = store_buy_via_api(
    StoreBuyRequest {
      target: args.control.target,
      slot: SlotId::new(ObjectZone::Store, slot_index),
      confirm_purchase: args.control.verify && args.control.verify_mode != VerifyModeArg::ActivationOnly,
      timeout_ms: args.control.timeout_ms.unwrap_or(1000),
    },
    config,
  )?;
  write_store_buy_output(&result)
}

#[cfg(not(target_os = "macos"))]
pub fn store_buy(_request: StoreBuyRequest) -> Result<StoreBuyResult, CliError> {
  Err(CliError::Message("store buy live operation is only available on macOS".to_string()))
}

fn store_buy_via_api(request: StoreBuyRequest, config: BalatroModelConfig) -> Result<StoreBuyResult, CliError> {
  if request.slot.zone != ObjectZone::Store {
    return Err(CliError::Message(format!("store buy requires a store slot, got {}", request.slot)));
  }
  let runtime = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .map_err(|error| CliError::Message(format!("failed to create daemon client runtime: {error}")))?;
  let probe = runtime.block_on(observe_live_via_api(&request.target, &config, None, true))?;
  // A preceding read may leave a tooltip covering store detections. Move to a
  // display corner outside the observed game viewport and obtain a fresh,
  // stable frame before resolving the caller's slot index.
  runtime.block_on(move_display_frame_point_via_api(&probe.frame, Point::new(1.0, 1.0)))?;
  std::thread::sleep(Duration::from_millis(400));
  let before = runtime.block_on(observe_live_via_api(&request.target, &config, None, true))?;
  let item = select_store_item(&before, request.slot.index)?.clone();
  let (screen_point, delivery) = runtime.block_on(click_display_frame_point_via_api(&before.frame, bbox_center_point(item.bbox)))?;
  let selection = StoreBuyClick {
    point: ActionPoint::screen(screen_point),
    delivery,
  };

  std::thread::sleep(Duration::from_millis(500));
  let selected = match runtime.block_on(observe_live_via_api(&request.target, &config, None, true)) {
    Ok(selected) => selected,
    Err(error) => {
      let result = StoreBuyResult {
        target: request.target,
        slot: request.slot,
        item,
        outcome: StoreBuyOutcome::SelectionOnly {
          selection,
          reason: StoreBuyIncompleteReason::StateReadFailed {
            message: error.to_string(),
          },
        },
      };
      emit_store_buy_completed(&result);
      return Ok(result);
    }
  };
  let confirmation_button = match select_store_buy_confirm_button(&selected) {
    Ok(button) => button.clone(),
    Err(_) => {
      let result = StoreBuyResult {
        target: request.target,
        slot: request.slot,
        item,
        outcome: StoreBuyOutcome::SelectionOnly {
          selection,
          reason: StoreBuyIncompleteReason::PurchaseControlNotFound,
        },
      };
      emit_store_buy_completed(&result);
      return Ok(result);
    }
  };
  let (screen_point, delivery) =
    runtime.block_on(click_display_frame_point_via_api(&selected.frame, bbox_center_point(confirmation_button.bbox)))?;
  let submission = StoreBuyClick {
    point: ActionPoint::screen(screen_point),
    delivery,
  };
  let confirmation = if request.confirm_purchase {
    std::thread::sleep(Duration::from_millis(request.timeout_ms.max(500)));
    evaluate_store_buy_confirmation(
      &before,
      runtime.block_on(observe_live_via_api(&request.target, &config, None, true)).as_ref().map_err(ToString::to_string),
    )
  } else {
    StoreBuyConfirmation::NotRequested
  };
  let result = StoreBuyResult {
    target: request.target,
    slot: request.slot,
    item,
    outcome: StoreBuyOutcome::Submitted {
      selection,
      confirmation_button,
      submission,
      confirmation,
    },
  };
  emit_store_buy_completed(&result);
  Ok(result)
}

#[derive(Debug, Serialize)]
struct StoreBuyCliOutput<'a> {
  operation: &'static str,
  target: &'a str,
  slot: SlotId,
  item: &'a StoreItem,
  outcome: &'a StoreBuyOutcome,
}

fn write_store_buy_output(result: &StoreBuyResult) -> Result<(), CliError> {
  write_output(
    OutputMode::Json,
    &StoreBuyCliOutput {
      operation: "store.buy",
      target: &result.target,
      slot: result.slot,
      item: &result.item,
      outcome: &result.outcome,
    },
  )
}

fn observe_from_args(args: &ObserveArgs) -> Result<BalatroState, CliError> {
  let config = BalatroModelConfig::from_observe_args(args);
  let inherited_context = std::env::var_os("AUV_CONTEXT").is_some();
  let endpoint = if inherited_context {
    None
  } else {
    auv::discovery::resolve(None).map_err(|error| CliError::Message(error.to_string()))?
  };
  if let Some(image) = args.image.as_deref() {
    if inherited_context || endpoint.is_some() {
      let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CliError::Message(format!("failed to create daemon client runtime: {error}")))?;
      let mut state = runtime.block_on(crate::observation::observe_image_via_api(image, &config, endpoint.clone(), args.no_cache))?;
      enrich_ui_numeric_readings_from_image(&mut state, image);
      return Ok(state);
    }
    return observe_image_with_ui_readings(image, &config, args.no_cache);
  }
  if inherited_context || endpoint.is_some() {
    let runtime = tokio::runtime::Builder::new_current_thread()
      .enable_all()
      .build()
      .map_err(|error| CliError::Message(format!("failed to create daemon client runtime: {error}")))?;
    return runtime.block_on(observe_live_via_api(&args.target, &config, endpoint, args.no_cache)).map_err(Into::into);
  }
  // TODO(balatro-daemon-actions): live card/object reads and action
  // verification retain their command-local driver sessions. Move those paths
  // only after an owner-approved typed action/verification contract exists.
  observe_live_target(&args.target, &config, args.no_cache)
}

#[derive(Clone, Debug, Serialize)]
struct HandSlotDiagnostic {
  slot: SlotId,
  bbox: BoundingBox,
  confidence: f32,
  selected: bool,
  requested: bool,
  click_frame_point: Point,
  click_inside_bbox: bool,
  visual_fingerprint: Option<String>,
  reading: Option<CardReadValue>,
}

#[derive(Debug, Serialize)]
struct FrameDiagnosticReport<'a> {
  schema_version: &'static str,
  frame: &'a crate::model::FrameRef,
  phase: BalatroPhase,
  scores: &'a crate::model::ScoreState,
  rounds: &'a crate::model::RoundState,
  consumables: &'a [ConsumableSlot],
  requested_slots: Vec<SlotId>,
  selection_matches_request: Option<bool>,
  hand: Vec<HandSlotDiagnostic>,
  raw_entities: &'a [crate::model::ObjectEvidence],
  raw_ui: &'a [crate::model::ObjectEvidence],
  state_diagnostics: &'a [crate::model::BalatroDiagnostic],
  warnings: Vec<String>,
}

fn write_frame_diagnostics(args: DiagnosticsArgs) -> Result<(), CliError> {
  let image_path = args
    .observe
    .image
    .as_deref()
    .ok_or_else(|| CliError::Message("diagnostics requires --image PATH so the annotated output is reproducible".to_string()))?;
  let state = observe_from_args(&args.observe)?;
  let requested = args.requested_slots.as_deref().map(parse_hand_slot_indices).transpose()?.unwrap_or_default();
  let interactions = hand_card_interactions(&state);
  let mut read_indices = requested.clone();
  read_indices.extend(interactions.iter().filter(|interaction| interaction.selected).map(|interaction| interaction.slot.index));
  read_indices.sort_unstable();
  read_indices.dedup();
  if read_indices.is_empty() {
    read_indices.extend(interactions.iter().map(|interaction| interaction.slot.index));
  }
  let reads = diagnostic_card_reads(image_path, &state, &read_indices)?;
  let hand = interactions
    .iter()
    .map(|interaction| HandSlotDiagnostic {
      slot: interaction.slot,
      bbox: interaction.bbox,
      confidence: interaction.confidence,
      selected: interaction.selected,
      requested: requested.contains(&interaction.slot.index),
      click_frame_point: interaction.click_frame_point,
      click_inside_bbox: point_inside_bbox(interaction.click_frame_point, interaction.bbox),
      visual_fingerprint: interaction.visual_fingerprint.clone(),
      reading: reads.iter().find(|read| read.slot == interaction.slot).map(|read| read.reading.clone()),
    })
    .collect::<Vec<_>>();
  let selection_matches_request = (!requested.is_empty()).then(|| {
    let mut selected = hand.iter().filter(|slot| slot.selected).map(|slot| slot.slot.index).collect::<Vec<_>>();
    let mut requested = requested.clone();
    selected.sort_unstable();
    requested.sort_unstable();
    selected == requested
  });
  let mut warnings = Vec::new();
  if selection_matches_request == Some(false) {
    warnings.push("observed raised-card selection does not match --requested-slots".to_string());
  }
  if hand.iter().any(|slot| !slot.click_inside_bbox) {
    warnings.push("at least one computed hand click lies outside its detected bbox".to_string());
  }
  if hand.iter().any(|slot| slot.reading.as_ref().is_some_and(|reading| !reading.valid)) {
    warnings.push("at least one requested or selected card could not be read as rank+suit".to_string());
  }
  let report = FrameDiagnosticReport {
    schema_version: "auv.game.balatro.frame_diagnostic.v1",
    frame: &state.frame,
    phase: state.phase,
    scores: &state.scores,
    rounds: &state.rounds,
    consumables: &state.consumables,
    requested_slots: requested.into_iter().map(|index| SlotId::new(ObjectZone::Hand, index)).collect(),
    selection_matches_request,
    hand,
    raw_entities: &state.raw_entities,
    raw_ui: &state.raw_ui,
    state_diagnostics: &state.diagnostics,
    warnings,
  };

  let mut annotated = image::open(image_path)?.to_rgba8();
  draw_frame_diagnostics(&mut annotated, &state, &report.hand);
  ensure_parent_directory(&args.annotated_out)?;
  annotated.save(&args.annotated_out)?;
  ensure_parent_directory(&args.report_out)?;
  fs::write(&args.report_out, serde_json::to_vec_pretty(&report)?)?;
  println!(
    "{}",
    serde_json::to_string_pretty(&json!({
      "annotated": args.annotated_out,
      "report": args.report_out,
      "phase": state.phase,
      "selection_matches_request": report.selection_matches_request,
      "warnings": report.warnings,
    }))?
  );
  Ok(())
}

fn ensure_parent_directory(path: &Path) -> Result<(), CliError> {
  if let Some(parent) = path.parent()
    && !parent.as_os_str().is_empty()
  {
    fs::create_dir_all(parent)?;
  }
  Ok(())
}

fn point_inside_bbox(point: Point, bbox: BoundingBox) -> bool {
  point.x >= f64::from(bbox.x1) && point.x <= f64::from(bbox.x2) && point.y >= f64::from(bbox.y1) && point.y <= f64::from(bbox.y2)
}

fn draw_frame_diagnostics(image: &mut RgbaImage, state: &BalatroState, hand: &[HandSlotDiagnostic]) {
  for evidence in &state.raw_entities {
    draw_bbox(image, evidence.detection.bbox, image::Rgba([40, 220, 90, 255]), 2);
  }
  for evidence in &state.raw_ui {
    draw_bbox(image, evidence.detection.bbox, image::Rgba([40, 180, 255, 255]), 2);
  }
  for slot in hand {
    let color = if slot.selected {
      image::Rgba([255, 60, 210, 255])
    } else {
      image::Rgba([255, 210, 40, 255])
    };
    draw_bbox(image, slot.bbox, color, 4);
    draw_cross(image, slot.click_frame_point, image::Rgba([255, 40, 40, 255]));
  }
}

fn draw_bbox(image: &mut RgbaImage, bbox: BoundingBox, color: image::Rgba<u8>, thickness: u32) {
  let max_x = image.width().saturating_sub(1);
  let max_y = image.height().saturating_sub(1);
  let x1 = bbox.x1.max(0.0).floor() as u32;
  let y1 = bbox.y1.max(0.0).floor() as u32;
  let x2 = bbox.x2.max(0.0).ceil() as u32;
  let y2 = bbox.y2.max(0.0).ceil() as u32;
  let (x1, x2) = (x1.min(max_x), x2.min(max_x));
  let (y1, y2) = (y1.min(max_y), y2.min(max_y));
  for offset in 0..thickness {
    let left = x1.saturating_add(offset).min(x2);
    let right = x2.saturating_sub(offset).max(x1);
    let top = y1.saturating_add(offset).min(y2);
    let bottom = y2.saturating_sub(offset).max(y1);
    for x in left..=right {
      image.put_pixel(x, top, color);
      image.put_pixel(x, bottom, color);
    }
    for y in top..=bottom {
      image.put_pixel(left, y, color);
      image.put_pixel(right, y, color);
    }
  }
}

fn draw_cross(image: &mut RgbaImage, point: Point, color: image::Rgba<u8>) {
  let x = point.x.round() as i64;
  let y = point.y.round() as i64;
  for delta in -9..=9 {
    for (px, py) in [(x + delta, y), (x, y + delta)] {
      if px >= 0 && py >= 0 && px < i64::from(image.width()) && py < i64::from(image.height()) {
        image.put_pixel(px as u32, py as u32, color);
      }
    }
  }
}

#[cfg(target_os = "macos")]
fn diagnostic_card_reads(image: &Path, state: &BalatroState, indices: &[u32]) -> Result<Vec<CardReadResult>, CliError> {
  let session = open_macos_session()?;
  let capture = capture_from_image(image)?;
  let templates = load_deck_rank_templates();
  indices
    .iter()
    .map(|index| {
      let card = select_hand_card(state, *index)?;
      read_card_from_capture(&session, &capture, image, state, card, templates.as_deref())
    })
    .collect()
}

#[cfg(not(target_os = "macos"))]
fn diagnostic_card_reads(image: &Path, state: &BalatroState, indices: &[u32]) -> Result<Vec<CardReadResult>, CliError> {
  indices
    .iter()
    .map(|index| {
      let card = select_hand_card(state, *index)?;
      Ok(CardReadResult {
        slot: card.slot,
        bbox: card.bbox,
        confidence: card.confidence,
        reading: CardReadValue::unread(),
        evidence: CardReadEvidence {
          frame: image.display().to_string(),
          ocr_region: ocr_region_for_card(state, card),
          corner_crop: None,
          source: "image_without_ocr_non_macos".to_string(),
        },
      })
    })
    .collect()
}

fn observe_image_with_ui_readings(image: &Path, config: &BalatroModelConfig, no_cache: bool) -> Result<BalatroState, CliError> {
  let mut state = observe_image(image, config, no_cache)?;
  enrich_ui_numeric_readings_from_image(&mut state, image);
  Ok(state)
}

fn read_cards_from_args(args: &SlotObserveArgs) -> Result<Vec<CardReadResult>, CliError> {
  let requested = parse_card_read_slots(&args.slot)?;
  let config = BalatroModelConfig::from_observe_args(&args.observe);
  if let Some(image) = args.observe.image.as_deref() {
    return read_cards_from_image(image, &config, args.observe.no_cache, &requested);
  }
  read_cards_live(&args.observe.target, &config, args.observe.no_cache, &requested, args.frame_out.as_deref())
}

#[cfg(target_os = "macos")]
fn read_object_live(args: &SlotObserveArgs, zone: ObjectReadZone) -> Result<ObjectReadResult, CliError> {
  if std::env::var_os("AUV_CONTEXT").is_some() {
    return read_object_live_via_api(args, zone);
  }
  let config = BalatroModelConfig::from_observe_args(&args.observe);
  let session = open_macos_session()?;
  let window = session.window().resolve(Window::main_visible().owned_by(App::name(args.observe.target.clone())))?;
  let capture = capture_window(&session, &window)?;
  let frame = match args.frame_out.as_deref() {
    Some(path) => save_capture_to_path(&capture, path)?,
    None => save_capture_to_temp(&capture, "object-read")?,
  };
  let state = observe_image_with_ui_readings(&frame, &config, args.observe.no_cache)?;
  let mut read = object_read_from_state(&state, &args.slot, zone)?;
  let original_mouse = auv_driver_macos::native::pointer::current_mouse_logical_point().ok();

  if let Err(error) = hover_read_object(&session, &window, &state, &mut read) {
    read.evidence.hover_error = Some(error.to_string());
  }

  if let Some((x, y)) = original_mouse {
    let _ = auv_driver_macos::native::pointer::move_point(x, y, 0);
  }

  Ok(read)
}

#[cfg(not(target_os = "macos"))]
fn read_object_live(args: &SlotObserveArgs, zone: ObjectReadZone) -> Result<ObjectReadResult, CliError> {
  read_object_live_via_api(args, zone)
}

fn read_object_live_via_api(args: &SlotObserveArgs, zone: ObjectReadZone) -> Result<ObjectReadResult, CliError> {
  let state = observe_from_args(&args.observe)?;
  let mut read = object_read_from_state(&state, &args.slot, zone)?;
  let runtime = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .map_err(|error| CliError::Message(format!("failed to create daemon client runtime: {error}")))?;
  let request = HoverReadRequest {
    point: bbox_center_point(read.bbox),
    region: object_hover_ocr_region(),
    custom_words: object_ocr_words().into_iter().map(str::to_string).collect(),
  };
  match runtime.block_on(hover_read_display_frame_points_via_api(&state.frame, &[request])) {
    Ok(mut observations) => {
      let observation = observations.pop().expect("one hover request returns one observation");
      apply_hover_read_observation(&mut read, observation.recognition, observation.frame_source);
    }
    Err(error) => {
      read.evidence.source = "remote_hover_ocr_failed".to_string();
      read.evidence.hover_error = Some(error.to_string());
    }
  }
  Ok(read)
}

fn hover_read_jokers_via_api(state: &mut BalatroState) -> Result<(), CliError> {
  let pending = state
    .jokers
    .iter()
    .enumerate()
    .filter(|(_, joker)| joker.cache.needs_reading || joker.reading.text.is_none())
    .map(|(index, joker)| {
      (
        index,
        HoverReadRequest {
          point: bbox_center_point(joker.bbox),
          region: object_hover_ocr_region(),
          custom_words: object_ocr_words().into_iter().map(str::to_string).collect(),
        },
      )
    })
    .collect::<Vec<_>>();
  if pending.is_empty() {
    return Ok(());
  }
  let requests = pending.iter().map(|(_, request)| request.clone()).collect::<Vec<_>>();
  let runtime = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .map_err(|error| CliError::Message(format!("failed to create daemon client runtime: {error}")))?;
  let observations = runtime.block_on(hover_read_display_frame_points_via_api(&state.frame, &requests))?;
  for ((index, _), observation) in pending.into_iter().zip(observations) {
    if let Some((text, confidence)) = tooltip_recognition(&observation.recognition, state.jokers[index].bbox) {
      let joker = &mut state.jokers[index];
      joker.reading = Reading {
        status: ReadingStatus::Read,
        text: Some(text),
        confidence,
      };
      joker.cache.needs_reading = false;
      joker.cache.changed_since_last_read = false;
    }
  }
  Ok(())
}

fn apply_hover_read_observation(read: &mut ObjectReadResult, recognition: auv_driver::TextRecognition, frame_source: String) {
  if let Some((text, confidence)) = tooltip_recognition(&recognition, read.bbox) {
    read.reading = ObjectReadValue {
      status: "read",
      raw_text: Some(text),
      confidence,
    };
    read.evidence.hover_required = false;
  }
  read.evidence.source = "remote_hover_ocr".to_string();
  read.evidence.hover_frame = Some(frame_source);
  read.evidence.hover_ocr_region = Some(object_hover_ocr_region());
}

fn tooltip_recognition(recognition: &auv_driver::TextRecognition, bbox: BoundingBox) -> Option<(String, Option<f32>)> {
  let width = f64::from((bbox.x2 - bbox.x1).max(1.0));
  let height = f64::from((bbox.y2 - bbox.y1).max(1.0));
  let x1 = f64::from(bbox.x1) - width * 1.8;
  let y1 = f64::from(bbox.y1) - height * 0.7;
  let x2 = f64::from(bbox.x2) + width * 1.8;
  let y2 = f64::from(bbox.y2) + height * 0.7;
  let regions = recognition
    .regions
    .iter()
    .filter(|region| {
      let bounds = region.bounds;
      bounds.origin.x + bounds.size.width >= x1
        && bounds.origin.x <= x2
        && bounds.origin.y + bounds.size.height >= y1
        && bounds.origin.y <= y2
    })
    .collect::<Vec<_>>();
  let text = regions.iter().map(|region| region.text.trim()).filter(|text| !text.is_empty()).collect::<Vec<_>>().join("\n");
  let text = non_empty_trimmed_text(&text).or_else(|| non_empty_trimmed_text(&recognition.text))?;
  let confidences = regions.iter().filter_map(|region| region.confidence).collect::<Vec<_>>();
  let confidence = (!confidences.is_empty()).then(|| confidences.iter().sum::<f32>() / confidences.len() as f32);
  Some((text, confidence))
}

#[cfg(target_os = "macos")]
fn read_cards_from_image(
  image: &Path,
  config: &BalatroModelConfig,
  no_cache: bool,
  requested: &Option<Vec<u32>>,
) -> Result<Vec<CardReadResult>, CliError> {
  let session = open_macos_session()?;
  let capture = capture_from_image(image)?;
  let state = observe_image_with_ui_readings(image, config, no_cache)?;
  let cards = select_cards_for_read(&state, requested)?;
  let rank_templates = load_deck_rank_templates();
  cards.into_iter().map(|card| read_card_from_capture(&session, &capture, image, &state, card, rank_templates.as_deref())).collect()
}

#[cfg(not(target_os = "macos"))]
fn read_cards_from_image(
  image: &Path,
  config: &BalatroModelConfig,
  no_cache: bool,
  requested: &Option<Vec<u32>>,
) -> Result<Vec<CardReadResult>, CliError> {
  let state = observe_image_with_ui_readings(image, config, no_cache)?;
  let cards = select_cards_for_read(&state, requested)?;
  cards
    .into_iter()
    .map(|card| {
      let region = ocr_region_for_card(&state, card);
      Ok(CardReadResult {
        slot: card.slot,
        bbox: card.bbox,
        confidence: card.confidence,
        reading: CardReadValue::unread(),
        evidence: CardReadEvidence {
          frame: state.frame.source.clone(),
          ocr_region: region,
          corner_crop: None,
          source: "image_without_ocr_non_macos".to_string(),
        },
      })
    })
    .collect()
}

fn write_output<T>(mode: OutputMode, value: &T) -> Result<(), CliError>
where
  T: Serialize + std::fmt::Debug,
{
  match mode {
    OutputMode::Human => {
      println!("{value:#?}");
      Ok(())
    }
    OutputMode::Json => {
      println!("{}", serde_json::to_string_pretty(value)?);
      Ok(())
    }
    OutputMode::JsonFile(path) => {
      crate::output::write_json_file(&path, value)?;
      Ok(())
    }
  }
}

#[cfg(target_os = "macos")]
fn click_consumable_use(args: TargetSlotOperationArgs) -> Result<(), CliError> {
  let slot_index = parse_consumable_slot_index(&args.slot)?;
  let target_indices = parse_hand_target_indices(&args.targets)?;
  let details = args.control.details;
  let result = consumable_use(ConsumableUseRequest {
    target: args.control.target,
    slot: SlotId::new(ObjectZone::Consumable, slot_index),
    hand_targets: target_indices.into_iter().map(|index| SlotId::new(ObjectZone::Hand, index)).collect(),
    confirm_use: args.control.verify && args.control.verify_mode != VerifyModeArg::ActivationOnly,
    timeout_ms: args.control.timeout_ms.unwrap_or(1200),
  })?;
  write_consumable_use_output(details, &result)
}

#[cfg(target_os = "macos")]
pub fn consumable_use(request: ConsumableUseRequest) -> Result<ConsumableUseResult, CliError> {
  if request.slot.zone != ObjectZone::Consumable {
    return Err(CliError::Message(format!("consumable use requires a consumable slot, got {}", request.slot)));
  }
  if let Some(target) = request.hand_targets.iter().find(|target| target.zone != ObjectZone::Hand) {
    return Err(CliError::Message(format!("consumable use hand target must use the hand zone, got {target}")));
  }

  let session = open_macos_session()?;
  let window = session.window().resolve(Window::main_visible().owned_by(App::name(request.target.clone())))?;
  let before_image = capture_window_to_temp(&session, &window, "consumable-use-before")?;
  let before = observe_image(&before_image, &BalatroModelConfig::default(), true);
  let _ = fs::remove_file(&before_image);
  let before = before?;
  let consumable = select_consumable(&before, request.slot.index)?.clone();
  let mut actions = Vec::new();
  let mut current = before.clone();

  if !request.hand_targets.is_empty() {
    let indices = request.hand_targets.iter().map(|target| target.index).collect::<Vec<_>>();
    let (selection, selected_state) =
      click_hand_targets(&session, &window, "consumable-use-targets", &before, &indices, Some(request.timeout_ms))?.into_parts();
    let targets_ready = selection.is_matched();
    actions.push(ConsumableUseAction::SelectHandTargets { selection });
    if !targets_ready {
      let result = ConsumableUseResult {
        target: request.target,
        slot: request.slot,
        consumable,
        actions,
        state: ConsumableUseState::Stopped {
          reason: ConsumableUseStop::HandTargetsNotReady,
        },
      };
      emit_consumable_use_completed(&result);
      return Ok(result);
    }
    let Some(selected_state) = selected_state else {
      unreachable!("a matched hand selection always retains its observed state");
    };
    current = selected_state;
  }

  let consumable_point = window_point_from_consumable(&current, &window, &consumable);
  let selection_delivery = match click_game_point(&session, &window, consumable_point) {
    Ok(delivery) => delivery,
    Err(error) => {
      let result = ConsumableUseResult {
        target: request.target,
        slot: request.slot,
        consumable,
        actions,
        state: ConsumableUseState::Stopped {
          reason: ConsumableUseStop::ConsumableSelectionFailed {
            message: error.to_string(),
          },
        },
      };
      emit_consumable_use_completed(&result);
      return Ok(result);
    }
  };
  actions.push(ConsumableUseAction::SelectConsumable {
    window_point: WindowPoint::new(consumable_point.x, consumable_point.y),
    delivery: selection_delivery,
  });

  std::thread::sleep(Duration::from_millis(500));
  let selected = match read_hand_selection_state(&session, &window, "consumable-use-selected", request.timeout_ms, 0) {
    Ok(state) => state,
    Err(message) => {
      let result = ConsumableUseResult {
        target: request.target,
        slot: request.slot,
        consumable,
        actions,
        state: ConsumableUseState::Stopped {
          reason: ConsumableUseStop::SelectedStateReadFailed { message },
        },
      };
      emit_consumable_use_completed(&result);
      return Ok(result);
    }
  };
  let (use_control, use_frame_point) = match resolve_consumable_use_target(&selected, request.slot.index) {
    Ok(target) => target,
    Err(error) => {
      let result = ConsumableUseResult {
        target: request.target,
        slot: request.slot,
        consumable,
        actions,
        state: ConsumableUseState::Stopped {
          reason: ConsumableUseStop::UseControlNotFound {
            message: error.to_string(),
          },
        },
      };
      emit_consumable_use_completed(&result);
      return Ok(result);
    }
  };
  let use_point = window_point_from_frame_point(&selected, &window, use_frame_point);
  let submission_delivery = match click_game_point(&session, &window, use_point) {
    Ok(delivery) => delivery,
    Err(error) => {
      let result = ConsumableUseResult {
        target: request.target,
        slot: request.slot,
        consumable,
        actions,
        state: ConsumableUseState::Stopped {
          reason: ConsumableUseStop::UseSubmissionFailed {
            message: error.to_string(),
          },
        },
      };
      emit_consumable_use_completed(&result);
      return Ok(result);
    }
  };
  actions.push(ConsumableUseAction::SubmitUse {
    control: use_control,
    window_point: WindowPoint::new(use_point.x, use_point.y),
    delivery: submission_delivery,
  });

  let confirmation = if request.confirm_use {
    let after = match capture_observable_window(&session, &window, "consumable-use-after", request.timeout_ms, request.timeout_ms.min(500)) {
      Ok((image, after)) => {
        let _ = fs::remove_file(image);
        after.map_err(|error| error.to_string())
      }
      Err(error) => Err(error.to_string()),
    };
    evaluate_consumable_use_confirmation(&before, after.as_ref().map_err(Clone::clone))
  } else {
    ConsumableUseConfirmation::NotRequested
  };
  let result = ConsumableUseResult {
    target: request.target,
    slot: request.slot,
    consumable,
    actions,
    state: ConsumableUseState::Submitted { confirmation },
  };
  emit_consumable_use_completed(&result);
  Ok(result)
}

#[cfg(not(target_os = "macos"))]
fn click_consumable_use(args: TargetSlotOperationArgs) -> Result<(), CliError> {
  parse_consumable_slot_index(&args.slot)?;
  parse_hand_target_indices(&args.targets)?;
  Err(CliError::Message("consumables use live operation is only available on macOS".to_string()))
}

#[cfg(not(target_os = "macos"))]
pub fn consumable_use(_request: ConsumableUseRequest) -> Result<ConsumableUseResult, CliError> {
  Err(CliError::Message("consumables use live operation is only available on macOS".to_string()))
}

#[derive(Debug, Serialize)]
struct ConsumableUseCliOutput<'a> {
  operation: &'static str,
  target: &'a str,
  slot: SlotId,
  actions: &'a [ConsumableUseAction],
  state: &'a ConsumableUseState,
  #[serde(skip_serializing_if = "Option::is_none")]
  consumable: Option<&'a ConsumableSlot>,
}

fn write_consumable_use_output(details: bool, result: &ConsumableUseResult) -> Result<(), CliError> {
  write_output(
    OutputMode::Json,
    &ConsumableUseCliOutput {
      operation: "consumables.use",
      target: &result.target,
      slot: result.slot,
      actions: &result.actions,
      state: &result.state,
      consumable: details.then_some(&result.consumable),
    },
  )
}

#[cfg(target_os = "macos")]
fn click_pack_skip(args: OperationControlArgs) -> Result<(), CliError> {
  let details = args.details;
  let result = pack_skip(PackSkipRequest {
    target: args.target,
    confirm_exit: args.verify && args.verify_mode != VerifyModeArg::ActivationOnly,
    timeout_ms: args.timeout_ms.unwrap_or(1200),
  })?;
  write_pack_skip_output(details, &result)
}

#[cfg(target_os = "macos")]
pub fn pack_skip(request: PackSkipRequest) -> Result<PackSkipResult, CliError> {
  let session = open_macos_session()?;
  let window = session.window().resolve(Window::main_visible().owned_by(App::name(request.target.clone())))?;
  let before_image = capture_window_to_temp(&session, &window, "pack-skip-before")?;
  // TODO(balatro-pack-skip-artifacts): emit the before/after captures through
  // auv-tracing once the shared in-memory capture encoder is owner-approved.
  let before = observe_image(&before_image, &BalatroModelConfig::default(), true);
  let _ = fs::remove_file(&before_image);
  let before = before?;
  let selected_button = find_button(&before, "button_card_pack_skip")?.clone();
  let point = window_point_from_button(&before, &window, &selected_button);
  let delivery = click_game_point(&session, &window, point)?;

  let confirmation = if request.confirm_exit {
    let (after_image, after) = capture_observable_window(&session, &window, "pack-skip-after", request.timeout_ms, 500)?;
    let confirmation = evaluate_pack_skip_confirmation(after.as_ref().map_err(|error| error.to_string()));
    let _ = fs::remove_file(after_image);
    confirmation
  } else {
    PackSkipConfirmation::NotRequested
  };

  let result = PackSkipResult {
    target: request.target,
    selected_button,
    window_point: WindowPoint::new(point.x, point.y),
    delivery,
    confirmation,
  };
  emit_pack_skip_completed(&result);
  Ok(result)
}

#[cfg(not(target_os = "macos"))]
fn click_pack_skip(_args: OperationControlArgs) -> Result<(), CliError> {
  Err(CliError::Message("pack skip live operation is only available on macOS".to_string()))
}

#[cfg(not(target_os = "macos"))]
pub fn pack_skip(_request: PackSkipRequest) -> Result<PackSkipResult, CliError> {
  Err(CliError::Message("pack skip live operation is only available on macOS".to_string()))
}

#[derive(Debug, Serialize)]
struct PackSkipCliOutput<'a> {
  operation: &'static str,
  target: &'a str,
  delivery: &'a auv_driver::InputActionResult,
  confirmation: &'a PackSkipConfirmation,
  #[serde(skip_serializing_if = "Option::is_none")]
  selected_button: Option<&'a ButtonTarget>,
  #[serde(skip_serializing_if = "Option::is_none")]
  window_point: Option<WindowPoint>,
}

fn write_pack_skip_output(details: bool, result: &PackSkipResult) -> Result<(), CliError> {
  write_output(
    OutputMode::Json,
    &PackSkipCliOutput {
      operation: "pack.skip",
      target: &result.target,
      delivery: &result.delivery,
      confirmation: &result.confirmation,
      selected_button: details.then_some(&result.selected_button),
      window_point: details.then_some(result.window_point),
    },
  )
}

#[cfg(target_os = "macos")]
fn click_pack_choose(args: TargetSlotOperationArgs) -> Result<(), CliError> {
  let slot_index = parse_pack_slot_index(&args.slot)?;
  let target_indices = parse_hand_target_indices(&args.targets)?;
  let details = args.control.details;
  let result = pack_choose(PackChooseRequest {
    target: args.control.target,
    choice: PackChoiceId::new(slot_index),
    hand_targets: target_indices.into_iter().map(|index| SlotId::new(ObjectZone::Hand, index)).collect(),
    confirm_applied: args.control.verify && args.control.verify_mode != VerifyModeArg::ActivationOnly,
    timeout_ms: args.control.timeout_ms.unwrap_or(1200),
  })?;
  write_pack_choose_output(details, &result)
}

#[cfg(target_os = "macos")]
pub fn pack_choose(request: PackChooseRequest) -> Result<PackChooseResult, CliError> {
  if let Some(target) = request.hand_targets.iter().find(|target| target.zone != ObjectZone::Hand) {
    return Err(CliError::Message(format!("pack choice hand target must use the hand zone, got {target}")));
  }

  let session = open_macos_session()?;
  let window = session.window().resolve(Window::main_visible().owned_by(App::name(request.target.clone())))?;
  let before_image = capture_window_to_temp(&session, &window, "pack-choose-before")?;
  let before = observe_image(&before_image, &BalatroModelConfig::default(), true);
  let _ = fs::remove_file(&before_image);
  let before = before?;
  let choices = active_pack_choices(&before);
  let choice = select_pack_choice(&choices, request.choice.index())?.clone();
  let before_choice_count = choices.len();
  let choice_was_already_selected = request.hand_targets.is_empty() && best_button(&before.buttons, "button_use").is_some();
  let mut actions = Vec::new();
  let mut selected = if choice_was_already_selected {
    before.clone()
  } else {
    let choice_point = window_point_from_frame_point(&before, &window, bbox_center_point(choice.bbox));
    let delivery = match click_game_point(&session, &window, choice_point) {
      Ok(delivery) => delivery,
      Err(error) => {
        let result = PackChooseResult {
          target: request.target,
          choice,
          choice_was_already_selected,
          actions,
          state: PackChooseState::Stopped {
            reason: PackChooseStop::ChoiceSelectionFailed {
              message: error.to_string(),
            },
          },
        };
        emit_pack_choose_completed(&result);
        return Ok(result);
      }
    };
    actions.push(PackChooseAction::SelectChoice {
      window_point: WindowPoint::new(choice_point.x, choice_point.y),
      delivery,
    });
    std::thread::sleep(Duration::from_millis(600));
    match read_hand_selection_state(&session, &window, "pack-choose-selected", request.timeout_ms, 0) {
      Ok(state) => state,
      Err(message) => {
        let result = PackChooseResult {
          target: request.target,
          choice,
          choice_was_already_selected,
          actions,
          state: PackChooseState::Stopped {
            reason: PackChooseStop::SelectedStateReadFailed { message },
          },
        };
        emit_pack_choose_completed(&result);
        return Ok(result);
      }
    }
  };

  if !request.hand_targets.is_empty() {
    let target_indices = request.hand_targets.iter().map(|target| target.index).collect::<Vec<_>>();
    let (selection, state) =
      click_hand_targets(&session, &window, "pack-choose-targets", &selected, &target_indices, Some(request.timeout_ms))?.into_parts();
    let targets_ready = selection.is_matched();
    actions.push(PackChooseAction::SelectHandTargets { selection });
    if !targets_ready {
      let result = PackChooseResult {
        target: request.target,
        choice,
        choice_was_already_selected,
        actions,
        state: PackChooseState::Stopped {
          reason: PackChooseStop::HandTargetsNotReady,
        },
      };
      emit_pack_choose_completed(&result);
      return Ok(result);
    }
    let Some(state) = state else {
      unreachable!("a matched hand selection always retains its observed state");
    };
    selected = state;
  }

  let (confirm_control, confirm_frame_point) = match resolve_pack_confirm_target(&selected, &choice) {
    Ok(target) => target,
    Err(error) => {
      let result = PackChooseResult {
        target: request.target,
        choice,
        choice_was_already_selected,
        actions,
        state: PackChooseState::Stopped {
          reason: PackChooseStop::ConfirmControlNotFound {
            message: error.to_string(),
          },
        },
      };
      emit_pack_choose_completed(&result);
      return Ok(result);
    }
  };
  let confirm_point = window_point_from_frame_point(&selected, &window, confirm_frame_point);
  let delivery = match click_game_point(&session, &window, confirm_point) {
    Ok(delivery) => delivery,
    Err(error) => {
      let result = PackChooseResult {
        target: request.target,
        choice,
        choice_was_already_selected,
        actions,
        state: PackChooseState::Stopped {
          reason: PackChooseStop::SubmissionFailed {
            message: error.to_string(),
          },
        },
      };
      emit_pack_choose_completed(&result);
      return Ok(result);
    }
  };
  actions.push(PackChooseAction::SubmitChoice {
    control: confirm_control,
    window_point: WindowPoint::new(confirm_point.x, confirm_point.y),
    delivery,
  });

  let confirmation = if request.confirm_applied {
    let after = match capture_observable_window(&session, &window, "pack-choose-after", request.timeout_ms, request.timeout_ms.min(500)) {
      Ok((image, after)) => {
        let _ = fs::remove_file(image);
        after
          .map(|state| ObservedPackState {
            choice_count: active_pack_choices(&state).len(),
            skip_control_present: best_button(&state.buttons, "button_card_pack_skip").is_some(),
          })
          .map_err(|error| error.to_string())
      }
      Err(error) => Err(error.to_string()),
    };
    evaluate_pack_choose_confirmation(before_choice_count, after)
  } else {
    PackChooseConfirmation::NotRequested
  };
  let result = PackChooseResult {
    target: request.target,
    choice,
    choice_was_already_selected,
    actions,
    state: PackChooseState::Submitted { confirmation },
  };
  emit_pack_choose_completed(&result);
  Ok(result)
}

#[cfg(not(target_os = "macos"))]
fn click_pack_choose(args: TargetSlotOperationArgs) -> Result<(), CliError> {
  parse_pack_slot_index(&args.slot)?;
  parse_hand_target_indices(&args.targets)?;
  Err(CliError::Message("pack choose live operation is only available on macOS".to_string()))
}

#[cfg(not(target_os = "macos"))]
pub fn pack_choose(_request: PackChooseRequest) -> Result<PackChooseResult, CliError> {
  Err(CliError::Message("pack choose live operation is only available on macOS".to_string()))
}

#[derive(Debug, Serialize)]
struct PackChooseCliOutput<'a> {
  operation: &'static str,
  target: &'a str,
  choice: PackChoiceId,
  choice_was_already_selected: bool,
  actions: &'a [PackChooseAction],
  state: &'a PackChooseState,
  #[serde(skip_serializing_if = "Option::is_none")]
  choice_details: Option<&'a PackChoice>,
}

fn write_pack_choose_output(details: bool, result: &PackChooseResult) -> Result<(), CliError> {
  write_output(
    OutputMode::Json,
    &PackChooseCliOutput {
      operation: "pack.choose",
      target: &result.target,
      choice: result.choice.id,
      choice_was_already_selected: result.choice_was_already_selected,
      actions: &result.actions,
      state: &result.state,
      choice_details: details.then_some(&result.choice),
    },
  )
}

#[cfg(target_os = "macos")]
fn click_store_next_round(args: OperationControlArgs) -> Result<(), CliError> {
  let config = BalatroModelConfig::from_operation_args(&args);
  let details = args.details;
  let confirmation = if !args.verify || args.verify_mode == VerifyModeArg::ActivationOnly {
    StoreNextRoundConfirmationRequest::None
  } else if args.verify_mode == VerifyModeArg::Targeted {
    StoreNextRoundConfirmationRequest::Targeted
  } else {
    StoreNextRoundConfirmationRequest::Weak
  };
  let request = StoreNextRoundRequest {
    target: args.target,
    confirmation,
    timeout_ms: args.timeout_ms.unwrap_or(1200),
  };
  let result = if std::env::var_os("AUV_CONTEXT").is_some() {
    store_next_round_via_api(request, config)?
  } else {
    store_next_round(request)?
  };
  write_store_next_round_output(details, &result)
}

#[cfg(target_os = "macos")]
pub fn store_next_round(request: StoreNextRoundRequest) -> Result<StoreNextRoundResult, CliError> {
  let session = open_macos_session()?;
  let window = session.window().resolve(Window::main_visible().owned_by(App::name(request.target.clone())))?;
  let before_image = capture_window_to_temp(&session, &window, "store-next-round-before")?;
  // TODO(balatro-store-next-round-artifacts): emit the before/after captures
  // through auv-tracing once the shared in-memory capture encoder is
  // owner-approved.
  let before = observe_image(&before_image, &BalatroModelConfig::default(), true);
  let _ = fs::remove_file(&before_image);
  let before = before?;
  let selected_target = resolve_store_next_round_target(&before)?;
  let point = window_point_from_frame_point(&before, &window, selected_target.frame_point());
  let delivery = click_game_point(&session, &window, point)?;

  let confirmation = if request.confirmation == StoreNextRoundConfirmationRequest::None {
    StoreNextRoundConfirmation::NotRequested
  } else {
    let (after_image, after) = capture_observable_window(&session, &window, "store-next-round-after", request.timeout_ms, 500)?;
    let confirmation =
      evaluate_store_next_round_confirmation(request.confirmation, &before, after.as_ref().map_err(|error| error.to_string()));
    let _ = fs::remove_file(after_image);
    confirmation
  };

  let result = StoreNextRoundResult {
    target: request.target,
    attempts: vec![StoreNextRoundAttempt {
      selected_target,
      point: ActionPoint::window(window.reference.id.clone(), WindowPoint::new(point.x, point.y)),
      delivery,
    }],
    confirmation,
  };
  emit_store_next_round_completed(&result);
  Ok(result)
}

#[cfg(not(target_os = "macos"))]
fn click_store_next_round(args: OperationControlArgs) -> Result<(), CliError> {
  if std::env::var_os("AUV_CONTEXT").is_none() {
    return Err(CliError::Message("store next-round requires macOS locally or an inherited AUV device context".to_string()));
  }
  let config = BalatroModelConfig::from_operation_args(&args);
  let details = args.details;
  let confirmation = if !args.verify || args.verify_mode == VerifyModeArg::ActivationOnly {
    StoreNextRoundConfirmationRequest::None
  } else if args.verify_mode == VerifyModeArg::Targeted {
    StoreNextRoundConfirmationRequest::Targeted
  } else {
    StoreNextRoundConfirmationRequest::Weak
  };
  let result = store_next_round_via_api(
    StoreNextRoundRequest {
      target: args.target,
      confirmation,
      timeout_ms: args.timeout_ms.unwrap_or(1200),
    },
    config,
  )?;
  write_store_next_round_output(details, &result)
}

#[cfg(not(target_os = "macos"))]
pub fn store_next_round(_request: StoreNextRoundRequest) -> Result<StoreNextRoundResult, CliError> {
  Err(CliError::Message("store next-round live operation is only available on macOS".to_string()))
}

fn store_next_round_via_api(request: StoreNextRoundRequest, config: BalatroModelConfig) -> Result<StoreNextRoundResult, CliError> {
  let runtime = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .map_err(|error| CliError::Message(format!("failed to create daemon client runtime: {error}")))?;
  let before = runtime.block_on(observe_live_via_api(&request.target, &config, None, true))?;
  let mut current = before.clone();
  let mut attempts = Vec::new();
  let mut confirmation = StoreNextRoundConfirmation::NotRequested;
  let attempt_limit = if request.confirmation == StoreNextRoundConfirmationRequest::None {
    1
  } else {
    2
  };
  for attempt in 0..attempt_limit {
    let selected_target = resolve_store_next_round_target(&current)?;
    let (screen_point, delivery) = runtime.block_on(click_display_frame_point_via_api(&current.frame, selected_target.frame_point()))?;
    attempts.push(StoreNextRoundAttempt {
      selected_target,
      point: ActionPoint::screen(screen_point),
      delivery,
    });
    if request.confirmation == StoreNextRoundConfirmationRequest::None {
      break;
    }
    let deadline = Instant::now() + Duration::from_millis((request.timeout_ms / attempt_limit as u64).max(350));
    loop {
      std::thread::sleep(Duration::from_millis(250));
      match runtime.block_on(observe_live_via_api(&request.target, &config, None, true)) {
        Ok(after) => {
          confirmation = evaluate_store_next_round_confirmation(request.confirmation, &before, Ok(&after));
          current = after;
          if matches!(confirmation, StoreNextRoundConfirmation::Confirmed { .. }) || Instant::now() >= deadline {
            break;
          }
        }
        Err(error) => {
          confirmation = evaluate_store_next_round_confirmation(request.confirmation, &before, Err(error.to_string()));
          break;
        }
      }
    }
    if matches!(confirmation, StoreNextRoundConfirmation::Confirmed { .. }) {
      break;
    }
    if attempt + 1 < attempt_limit {
      std::thread::sleep(Duration::from_millis(250));
    }
  }
  let result = StoreNextRoundResult {
    target: request.target,
    attempts,
    confirmation,
  };
  emit_store_next_round_completed(&result);
  Ok(result)
}

#[derive(Debug, Serialize)]
struct StoreNextRoundCliOutput<'a> {
  operation: &'static str,
  target: &'a str,
  confirmation: &'a StoreNextRoundConfirmation,
  attempts: &'a [StoreNextRoundAttempt],
}

fn write_store_next_round_output(_details: bool, result: &StoreNextRoundResult) -> Result<(), CliError> {
  write_output(
    OutputMode::Json,
    &StoreNextRoundCliOutput {
      operation: "store.next_round",
      target: &result.target,
      confirmation: &result.confirmation,
      attempts: &result.attempts,
    },
  )
}

#[cfg(target_os = "macos")]
fn click_cards_select(args: MultiSlotOperationArgs) -> Result<(), CliError> {
  let slot_indices = parse_hand_slot_indices(&args.slots)?;
  let result = cards_select(CardsSelectRequest {
    target: args.control.target,
    slots: slot_indices.into_iter().map(|index| SlotId::new(ObjectZone::Hand, index)).collect(),
    timeout_ms: args.control.timeout_ms.unwrap_or(1500),
  })?;
  write_cards_select_output(&result)
}

#[cfg(not(target_os = "macos"))]
fn click_cards_select(_args: MultiSlotOperationArgs) -> Result<(), CliError> {
  Err(CliError::Message("cards select live operation is only available on macOS".to_string()))
}

#[cfg(target_os = "macos")]
pub fn cards_select(request: CardsSelectRequest) -> Result<CardsSelectResult, CliError> {
  validate_hand_slots(&request.slots, "cards select")?;
  let session = open_macos_session()?;
  let window = session.window().resolve(Window::main_visible().owned_by(App::name(request.target.clone())))?;
  let (before_image, before) = capture_observable_window(&session, &window, "cards-select-before", request.timeout_ms, 0)?;
  let _ = fs::remove_file(before_image);
  let before = before?;
  let indices = request.slots.iter().map(|slot| slot.index).collect::<Vec<_>>();
  let (selection, _) = click_hand_targets(&session, &window, "cards-select", &before, &indices, Some(request.timeout_ms))?.into_parts();
  let result = CardsSelectResult {
    target: request.target,
    selection,
  };
  emit_cards_select_completed(&result);
  Ok(result)
}

#[cfg(not(target_os = "macos"))]
pub fn cards_select(_request: CardsSelectRequest) -> Result<CardsSelectResult, CliError> {
  Err(CliError::Message("cards select live operation is only available on macOS".to_string()))
}

#[derive(Debug, Serialize)]
struct CardsSelectCliOutput<'a> {
  operation: &'static str,
  target: &'a str,
  selection: &'a HandSelectionResult,
}

fn write_cards_select_output(result: &CardsSelectResult) -> Result<(), CliError> {
  write_output(
    OutputMode::Json,
    &CardsSelectCliOutput {
      operation: "cards.select",
      target: &result.target,
      selection: &result.selection,
    },
  )
}

#[cfg(target_os = "macos")]
fn click_cards_clear(args: OperationControlArgs) -> Result<(), CliError> {
  let result = cards_clear(CardsClearRequest {
    target: args.target,
    timeout_ms: args.timeout_ms.unwrap_or(1500),
  })?;
  write_cards_clear_output(&result)
}

#[cfg(target_os = "macos")]
pub fn cards_clear(request: CardsClearRequest) -> Result<CardsClearResult, CliError> {
  let session = open_macos_session()?;
  let window = session.window().resolve(Window::main_visible().owned_by(App::name(request.target.clone())))?;
  let (before_image, before_result) = capture_observable_window(&session, &window, "cards-clear-before", request.timeout_ms, 0)?;
  // TODO(balatro-cards-clear-artifacts): emit captures through auv-tracing
  // once the shared in-memory capture encoder is owner-approved.
  let _ = fs::remove_file(before_image);
  let before = before_result?;
  let before_interactions = hand_card_interactions(&before);
  let initially_selected =
    before_interactions.iter().filter(|interaction| interaction.selected).map(|interaction| interaction.slot).collect::<Vec<_>>();

  let mut click_state = before.clone();
  let mut toggles = Vec::new();
  for slot in &initially_selected {
    let card = select_hand_card(&click_state, slot.index)?;
    let point = window_point_from_hand_card(&click_state, &window, card);
    let delivery = click_game_point(&session, &window, point)?;
    toggles.push(CardSelectionToggle {
      slot: *slot,
      window_point: WindowPoint::new(point.x, point.y),
      delivery,
    });
    let state_result = match capture_observable_window(&session, &window, "cards-clear-after-toggle", request.timeout_ms, 120) {
      Ok((image, state_result)) => {
        let _ = fs::remove_file(image);
        state_result.map_err(|error| error.to_string())
      }
      Err(error) => Err(error.to_string()),
    };
    match state_result {
      Ok(state) => click_state = state,
      Err(message) => {
        let result = CardsClearResult {
          target: request.target,
          initially_selected,
          toggles,
          outcome: CardsClearOutcome::Incomplete {
            reason: CardsClearIncompleteReason::StateReadFailed { message },
          },
        };
        emit_cards_clear_completed(&result);
        return Ok(result);
      }
    }
  }

  let remaining = hand_card_interactions(&click_state)
    .into_iter()
    .filter(|interaction| interaction.selected)
    .map(|interaction| interaction.slot)
    .collect();
  let result = CardsClearResult {
    target: request.target,
    initially_selected,
    toggles,
    outcome: classify_cards_clear_outcome(remaining),
  };
  emit_cards_clear_completed(&result);
  Ok(result)
}

#[cfg(not(target_os = "macos"))]
fn click_cards_clear(_args: OperationControlArgs) -> Result<(), CliError> {
  Err(CliError::Message("cards clear live operation is only available on macOS".to_string()))
}

#[cfg(not(target_os = "macos"))]
pub fn cards_clear(_request: CardsClearRequest) -> Result<CardsClearResult, CliError> {
  Err(CliError::Message("cards clear live operation is only available on macOS".to_string()))
}

#[derive(Debug, Serialize)]
struct CardsClearCliOutput<'a> {
  operation: &'static str,
  target: &'a str,
  initially_selected: &'a [SlotId],
  toggles: &'a [CardSelectionToggle],
  outcome: &'a CardsClearOutcome,
}

fn write_cards_clear_output(result: &CardsClearResult) -> Result<(), CliError> {
  write_output(
    OutputMode::Json,
    &CardsClearCliOutput {
      operation: "cards.clear",
      target: &result.target,
      initially_selected: &result.initially_selected,
      toggles: &result.toggles,
      outcome: &result.outcome,
    },
  )
}

#[cfg(target_os = "macos")]
fn click_cards_commit(kind: CardCommitKind, args: MultiSlotOperationArgs) -> Result<(), CliError> {
  let slot_indices = parse_hand_slot_indices(&args.slots)?;
  let config = BalatroModelConfig::from_operation_args(&args.control);
  let request = CardCommitRequest {
    target: args.control.target,
    slots: slot_indices.into_iter().map(|index| SlotId::new(ObjectZone::Hand, index)).collect(),
    confirm_change: args.control.verify && args.control.verify_mode != VerifyModeArg::ActivationOnly,
    timeout_ms: args.control.timeout_ms.unwrap_or(1500),
  };
  let result = if std::env::var_os("AUV_CONTEXT").is_some() {
    card_commit_via_api(kind, request, config)?
  } else {
    match kind {
      CardCommitKind::Play => cards_play(request),
      CardCommitKind::Discard => cards_discard(request),
    }?
  };
  write_card_commit_output(&result)
}

#[cfg(not(target_os = "macos"))]
fn click_cards_commit(kind: CardCommitKind, args: MultiSlotOperationArgs) -> Result<(), CliError> {
  if std::env::var_os("AUV_CONTEXT").is_none() {
    return Err(CliError::Message(
      "cards play/discard live operations require macOS locally or an inherited AUV device context".to_string(),
    ));
  }
  let slot_indices = parse_hand_slot_indices(&args.slots)?;
  let config = BalatroModelConfig::from_operation_args(&args.control);
  let result = card_commit_via_api(
    kind,
    CardCommitRequest {
      target: args.control.target,
      slots: slot_indices.into_iter().map(|index| SlotId::new(ObjectZone::Hand, index)).collect(),
      confirm_change: args.control.verify && args.control.verify_mode != VerifyModeArg::ActivationOnly,
      timeout_ms: args.control.timeout_ms.unwrap_or(1500),
    },
    config,
  )?;
  write_card_commit_output(&result)
}

#[cfg(target_os = "macos")]
pub fn cards_play(request: CardCommitRequest) -> Result<CardCommitResult, CliError> {
  card_commit(CardCommitKind::Play, request)
}

#[cfg(not(target_os = "macos"))]
pub fn cards_play(_request: CardCommitRequest) -> Result<CardCommitResult, CliError> {
  Err(CliError::Message("cards play live operation is only available on macOS".to_string()))
}

#[cfg(target_os = "macos")]
pub fn cards_discard(request: CardCommitRequest) -> Result<CardCommitResult, CliError> {
  card_commit(CardCommitKind::Discard, request)
}

#[cfg(not(target_os = "macos"))]
pub fn cards_discard(_request: CardCommitRequest) -> Result<CardCommitResult, CliError> {
  Err(CliError::Message("cards discard live operation is only available on macOS".to_string()))
}

#[cfg(target_os = "macos")]
fn card_commit(kind: CardCommitKind, request: CardCommitRequest) -> Result<CardCommitResult, CliError> {
  validate_hand_slots(&request.slots, "card commit")?;
  let session = open_macos_session()?;
  let window = session.window().resolve(Window::main_visible().owned_by(App::name(request.target.clone())))?;
  let (before_image, before) = capture_observable_window(&session, &window, "card-commit-before", request.timeout_ms, 0)?;
  let _ = fs::remove_file(before_image);
  let before = before?;
  let indices = request.slots.iter().map(|slot| slot.index).collect::<Vec<_>>();
  select_hand_cards(&before, &indices)?;

  let (selection, selected) =
    click_hand_targets(&session, &window, "card-commit-targets", &before, &indices, Some(request.timeout_ms))?.into_parts();
  let targets_ready = selection.is_matched();
  let mut actions = vec![CardCommitAction::SelectHandTargets { selection }];
  if !targets_ready {
    let result = CardCommitResult {
      target: request.target,
      kind,
      requested_slots: request.slots,
      actions,
      state: CardCommitState::Stopped {
        reason: CardCommitStop::HandTargetsNotReady,
      },
    };
    emit_card_commit_completed(&result);
    return Ok(result);
  }
  let Some(selected) = selected else {
    unreachable!("a matched hand selection always retains its observed state");
  };

  let (control, frame_point) = match resolve_card_commit_target(&selected, kind) {
    Ok(target) => target,
    Err(error) => {
      let result = CardCommitResult {
        target: request.target,
        kind,
        requested_slots: request.slots,
        actions,
        state: CardCommitState::Stopped {
          reason: CardCommitStop::CommitControlNotFound {
            message: error.to_string(),
          },
        },
      };
      emit_card_commit_completed(&result);
      return Ok(result);
    }
  };
  let point = window_point_from_frame_point(&selected, &window, frame_point);
  let delivery = match click_game_point(&session, &window, point) {
    Ok(delivery) => delivery,
    Err(error) => {
      let result = CardCommitResult {
        target: request.target,
        kind,
        requested_slots: request.slots,
        actions,
        state: CardCommitState::Stopped {
          reason: CardCommitStop::SubmissionFailed {
            message: error.to_string(),
          },
        },
      };
      emit_card_commit_completed(&result);
      return Ok(result);
    }
  };
  actions.push(CardCommitAction::Submit {
    control,
    point: ActionPoint::window(window.reference.id.clone(), WindowPoint::new(point.x, point.y)),
    delivery,
  });

  let confirmation = if request.confirm_change {
    let after = match capture_observable_window(&session, &window, "card-commit-after", request.timeout_ms, request.timeout_ms.min(600)) {
      Ok((image, after)) => {
        let _ = fs::remove_file(image);
        after.map_err(|error| error.to_string())
      }
      Err(error) => Err(error.to_string()),
    };
    evaluate_card_commit_confirmation(&selected, after.as_ref().map_err(Clone::clone))
  } else {
    CardCommitConfirmation::NotRequested
  };
  let result = CardCommitResult {
    target: request.target,
    kind,
    requested_slots: request.slots,
    actions,
    state: CardCommitState::Submitted { confirmation },
  };
  emit_card_commit_completed(&result);
  Ok(result)
}

fn card_commit_via_api(kind: CardCommitKind, request: CardCommitRequest, config: BalatroModelConfig) -> Result<CardCommitResult, CliError> {
  validate_hand_slots(&request.slots, "card commit")?;
  let runtime = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .map_err(|error| CliError::Message(format!("failed to create daemon client runtime: {error}")))?;
  // TODO(balatro-remote-operation-session): observations still open several
  // short-lived API clients. Reuse one operation session after that lifecycle
  // contract is owner-approved; see
  // `2026-08-05-linux-live-gameplay-evidence.md`.
  let before = runtime.block_on(observe_live_via_api(&request.target, &config, None, true))?;
  let requested = request.slots.iter().map(|slot| slot.index).collect::<Vec<_>>();
  select_hand_cards(&before, &requested)?;
  let mut current = before.clone();
  let mut toggles = Vec::new();

  for attempt in 1..=2 {
    let clear = selected_slots_to_clear(&current, &requested);
    let select = requested_slots_to_select(&current, &requested);
    if clear.is_empty() && select.is_empty() {
      break;
    }
    for (kind, indices) in [
      (HandSelectionToggleKind::ClearUnexpected, clear),
      (HandSelectionToggleKind::SelectRequested, select),
    ] {
      for index in indices {
        let card = select_hand_card(&current, index)?;
        let frame_point = hand_card_click_frame_point(&current, card);
        let (screen_point, delivery) = runtime.block_on(click_display_frame_point_via_api(&current.frame, frame_point))?;
        toggles.push(HandSelectionToggle {
          kind,
          attempt,
          slot: SlotId::new(ObjectZone::Hand, index),
          point: ActionPoint::screen(screen_point),
          delivery,
        });
        std::thread::sleep(Duration::from_millis(220));
      }
    }
    std::thread::sleep(Duration::from_millis(650));
    current = runtime.block_on(observe_live_via_api(&request.target, &config, None, true))?;
  }

  let selected = selected_hand_slot_indices(&current, &hand_slot_indices(&current))
    .into_iter()
    .map(|index| SlotId::new(ObjectZone::Hand, index))
    .collect::<Vec<_>>();
  let matched = hand_selection_matches_requested(&current, &requested);
  let selection = HandSelectionResult {
    requested: request.slots.clone(),
    toggles,
    state: if matched {
      HandSelectionState::Matched { selected }
    } else {
      HandSelectionState::NotMatched { selected }
    },
  };
  let mut actions = vec![CardCommitAction::SelectHandTargets { selection }];
  if !matched {
    let result = CardCommitResult {
      target: request.target,
      kind,
      requested_slots: request.slots,
      actions,
      state: CardCommitState::Stopped {
        reason: CardCommitStop::HandTargetsNotReady,
      },
    };
    emit_card_commit_completed(&result);
    return Ok(result);
  }

  let selected_before_submit = current.clone();
  let mut submit_state = current;
  let mut confirmation = CardCommitConfirmation::NotRequested;
  let submit_attempts = if request.confirm_change { 2 } else { 1 };
  for attempt in 0..submit_attempts {
    let (control, frame_point) = match resolve_card_commit_target(&submit_state, kind) {
      Ok(target) => target,
      Err(error) if attempt == 0 => {
        let result = CardCommitResult {
          target: request.target,
          kind,
          requested_slots: request.slots,
          actions,
          state: CardCommitState::Stopped {
            reason: CardCommitStop::CommitControlNotFound {
              message: error.to_string(),
            },
          },
        };
        emit_card_commit_completed(&result);
        return Ok(result);
      }
      Err(_) => break,
    };
    let (screen_point, delivery) = runtime.block_on(click_display_frame_point_via_api(&submit_state.frame, frame_point))?;
    actions.push(CardCommitAction::Submit {
      control,
      point: ActionPoint::screen(screen_point),
      delivery,
    });
    if !request.confirm_change {
      break;
    }
    let settle_deadline = Instant::now() + Duration::from_millis(request.timeout_ms.min(1200));
    let mut observation_count = 0;
    loop {
      std::thread::sleep(Duration::from_millis(if observation_count == 0 { 750 } else { 500 }));
      observation_count += 1;
      match runtime.block_on(observe_live_via_api(&request.target, &config, None, true)) {
        Ok(after) => {
          confirmation = evaluate_card_commit_confirmation(&selected_before_submit, Ok(&after));
          submit_state = after;
          if card_commit_confirmation_has_structural_change(&confirmation) {
            break;
          }
          // A changed hand with no stable score/round OCR is normally a frame
          // captured during Balatro's scoring animation. Observe once more,
          // but never turn that partial success evidence into a second click.
          if observation_count >= 2 || (!matches!(confirmation, CardCommitConfirmation::Applied { .. }) && Instant::now() >= settle_deadline)
          {
            break;
          }
        }
        Err(error) => {
          confirmation = evaluate_card_commit_confirmation(&selected_before_submit, Err(error.to_string()));
          break;
        }
      }
    }
    if card_commit_confirmation_has_structural_change(&confirmation) {
      break;
    }
    if !card_commit_confirmation_allows_resubmit(&confirmation) {
      break;
    }
  }
  confirmation = reject_fingerprint_only_confirmation(confirmation);
  let result = CardCommitResult {
    target: request.target,
    kind,
    requested_slots: request.slots,
    actions,
    state: CardCommitState::Submitted { confirmation },
  };
  emit_card_commit_completed(&result);
  Ok(result)
}

#[derive(Debug, Serialize)]
struct CardCommitCliOutput<'a> {
  operation: &'static str,
  target: &'a str,
  requested_slots: &'a [SlotId],
  actions: &'a [CardCommitAction],
  state: &'a CardCommitState,
}

fn write_card_commit_output(result: &CardCommitResult) -> Result<(), CliError> {
  write_output(
    OutputMode::Json,
    &CardCommitCliOutput {
      operation: match result.kind {
        CardCommitKind::Play => "cards.play",
        CardCommitKind::Discard => "cards.discard",
      },
      target: &result.target,
      requested_slots: &result.requested_slots,
      actions: &result.actions,
      state: &result.state,
    },
  )
}

#[cfg(target_os = "macos")]
enum HandSelectionExecution {
  Observed {
    result: HandSelectionResult,
    state: BalatroState,
  },
  Incomplete {
    result: HandSelectionResult,
  },
}

#[cfg(target_os = "macos")]
impl HandSelectionExecution {
  fn into_parts(self) -> (HandSelectionResult, Option<BalatroState>) {
    match self {
      Self::Observed { result, state } => (result, Some(state)),
      Self::Incomplete { result } => (result, None),
    }
  }
}

#[cfg(target_os = "macos")]
fn click_hand_targets(
  session: &auv_driver_macos::MacosDriverSession,
  window: &auv_driver::window::Window,
  operation: &str,
  before: &BalatroState,
  slot_indices: &[u32],
  timeout_ms: Option<u64>,
) -> Result<HandSelectionExecution, CliError> {
  let _cards = select_hand_cards(before, slot_indices)?;
  let requested = slot_indices.iter().map(|index| SlotId::new(ObjectZone::Hand, *index)).collect::<Vec<_>>();
  let mut click_state = before.clone();
  let mut toggles = Vec::new();
  for slot_index in selected_slots_to_clear(&click_state, slot_indices) {
    let card = match select_hand_card(&click_state, slot_index) {
      Ok(card) => card,
      Err(error) => {
        return Ok(incomplete_hand_selection(requested, toggles, &click_state, error.to_string()));
      }
    };
    let point = window_point_from_hand_card(&click_state, window, card);
    let slot = card.slot;
    let delivery = match click_game_point(session, window, point) {
      Ok(delivery) => delivery,
      Err(error) => return Ok(incomplete_hand_selection(requested, toggles, &click_state, error.to_string())),
    };
    toggles.push(HandSelectionToggle {
      kind: HandSelectionToggleKind::ClearUnexpected,
      attempt: 1,
      slot,
      point: ActionPoint::window(window.reference.id.clone(), WindowPoint::new(point.x, point.y)),
      delivery,
    });
    std::thread::sleep(Duration::from_millis(160));
    match read_hand_selection_state(session, window, operation, timeout_ms.unwrap_or(1500), 120) {
      Ok(state) => click_state = state,
      Err(message) => return Ok(incomplete_hand_selection(requested, toggles, &click_state, message)),
    }
  }

  for slot_index in requested_slots_to_select(&click_state, slot_indices) {
    for attempt in 1..=2 {
      if hand_slot_is_selected(&click_state, slot_index) {
        break;
      }
      let card = match select_hand_card(&click_state, slot_index) {
        Ok(card) => card,
        Err(error) => {
          return Ok(incomplete_hand_selection(requested, toggles, &click_state, error.to_string()));
        }
      };
      let point = window_point_from_hand_card(&click_state, window, card);
      let slot = card.slot;
      let delivery = match click_game_point(session, window, point) {
        Ok(delivery) => delivery,
        Err(error) => return Ok(incomplete_hand_selection(requested, toggles, &click_state, error.to_string())),
      };
      toggles.push(HandSelectionToggle {
        kind: HandSelectionToggleKind::SelectRequested,
        attempt,
        slot,
        point: ActionPoint::window(window.reference.id.clone(), WindowPoint::new(point.x, point.y)),
        delivery,
      });
      std::thread::sleep(Duration::from_millis(180));
      match read_hand_selection_state(session, window, operation, timeout_ms.unwrap_or(1500), 120) {
        Ok(state) => click_state = state,
        Err(message) => return Ok(incomplete_hand_selection(requested, toggles, &click_state, message)),
      }
    }
  }

  std::thread::sleep(Duration::from_millis(250));
  let selected = match read_hand_selection_state(session, window, operation, timeout_ms.unwrap_or(1500), 0) {
    Ok(selected) => selected,
    Err(message) => return Ok(incomplete_hand_selection(requested, toggles, &click_state, message)),
  };
  let selected_slots = selected_hand_slot_indices(&selected, &hand_slot_indices(&selected))
    .into_iter()
    .map(|index| SlotId::new(ObjectZone::Hand, index))
    .collect();
  let state = if hand_selection_matches_requested(&selected, slot_indices) {
    HandSelectionState::Matched {
      selected: selected_slots,
    }
  } else {
    HandSelectionState::NotMatched {
      selected: selected_slots,
    }
  };
  Ok(HandSelectionExecution::Observed {
    result: HandSelectionResult {
      requested,
      toggles,
      state,
    },
    state: selected,
  })
}

#[cfg(target_os = "macos")]
fn read_hand_selection_state(
  session: &auv_driver_macos::MacosDriverSession,
  window: &auv_driver::window::Window,
  operation: &str,
  timeout_ms: u64,
  initial_delay_ms: u64,
) -> Result<BalatroState, String> {
  match capture_observable_window(session, window, operation, timeout_ms, initial_delay_ms) {
    Ok((image, state)) => {
      let state = state.map_err(|error| error.to_string());
      let _ = fs::remove_file(image);
      state
    }
    Err(error) => Err(error.to_string()),
  }
}

#[cfg(target_os = "macos")]
fn incomplete_hand_selection(
  requested: Vec<SlotId>,
  toggles: Vec<HandSelectionToggle>,
  last_state: &BalatroState,
  message: String,
) -> HandSelectionExecution {
  let last_selected = selected_hand_slot_indices(last_state, &hand_slot_indices(last_state))
    .into_iter()
    .map(|index| SlotId::new(ObjectZone::Hand, index))
    .collect();
  HandSelectionExecution::Incomplete {
    result: HandSelectionResult {
      requested,
      toggles,
      state: HandSelectionState::Incomplete {
        last_selected,
        message,
      },
    },
  }
}

#[cfg(target_os = "macos")]
fn click_blind_select(args: SlotOperationArgs) -> Result<(), CliError> {
  let slot_index = parse_blind_slot_index(&args.slot)?;
  let config = BalatroModelConfig::from_operation_args(&args.control);
  let request = BlindSelectRequest {
    target: args.control.target,
    slot: SlotId::new(ObjectZone::Blind, slot_index),
    confirm_started: args.control.verify && args.control.verify_mode != VerifyModeArg::ActivationOnly,
    timeout_ms: args.control.timeout_ms.unwrap_or(1200),
  };
  let result = if std::env::var_os("AUV_CONTEXT").is_some() {
    blind_select_via_api(request, config)?
  } else {
    blind_select(request)?
  };
  write_blind_select_output(&result)
}

#[cfg(not(target_os = "macos"))]
fn click_blind_select(args: SlotOperationArgs) -> Result<(), CliError> {
  if std::env::var_os("AUV_CONTEXT").is_none() {
    return Err(CliError::Message("blinds select live operation requires macOS locally or an inherited AUV device context".to_string()));
  }
  let slot_index = parse_blind_slot_index(&args.slot)?;
  let config = BalatroModelConfig::from_operation_args(&args.control);
  let result = blind_select_via_api(
    BlindSelectRequest {
      target: args.control.target,
      slot: SlotId::new(ObjectZone::Blind, slot_index),
      confirm_started: args.control.verify && args.control.verify_mode != VerifyModeArg::ActivationOnly,
      timeout_ms: args.control.timeout_ms.unwrap_or(1200),
    },
    config,
  )?;
  write_blind_select_output(&result)
}

#[cfg(target_os = "macos")]
fn click_blind_skip(args: OperationControlArgs) -> Result<(), CliError> {
  let result = blind_skip(BlindSkipRequest {
    target: args.target,
    confirm_exit: args.verify && args.verify_mode != VerifyModeArg::ActivationOnly,
    timeout_ms: args.timeout_ms.unwrap_or(1200),
  })?;
  write_blind_skip_output(&result)
}

#[cfg(not(target_os = "macos"))]
fn click_blind_skip(_args: OperationControlArgs) -> Result<(), CliError> {
  Err(CliError::Message("blinds skip live operation is only available on macOS".to_string()))
}

#[cfg(target_os = "macos")]
pub fn blind_select(request: BlindSelectRequest) -> Result<BlindSelectResult, CliError> {
  if request.slot.zone != ObjectZone::Blind {
    return Err(CliError::Message(format!("blind select requires a blind slot, got {}", request.slot)));
  }
  let session = open_macos_session()?;
  let window = session.window().resolve(Window::main_visible().owned_by(App::name(request.target.clone())))?;
  let before_image = capture_window_to_temp(&session, &window, "blind-select-before")?;
  // TODO(balatro-blind-action-artifacts): emit captures through auv-tracing
  // once the shared in-memory capture encoder is owner-approved.
  let before = observe_image(&before_image, &BalatroModelConfig::default(), true);
  let _ = fs::remove_file(before_image);
  let before = before?;
  let selected_button = select_button_for_slot(&before.buttons, "button_level_select", Some(request.slot.index))?.clone();
  let point = window_point_from_button(&before, &window, &selected_button);
  let delivery = click_game_point(&session, &window, point)?;

  let confirmation = if request.confirm_started {
    match capture_observable_window(&session, &window, "blind-select-after", request.timeout_ms, 500) {
      Ok((after_image, after)) => {
        let confirmation = evaluate_blind_select_confirmation(&before, after.as_ref().map_err(|error| error.to_string()));
        let _ = fs::remove_file(after_image);
        confirmation
      }
      Err(error) => BlindSelectConfirmation::NotStarted {
        before_phase: before.phase,
        after_phase: None,
        reason: BlindSelectConfirmationFailure::StateReadFailed {
          message: error.to_string(),
        },
      },
    }
  } else {
    BlindSelectConfirmation::NotRequested
  };

  let result = BlindSelectResult {
    target: request.target,
    slot: request.slot,
    attempts: vec![BlindSelectAttempt {
      selected_button,
      point: ActionPoint::window(window.reference.id.clone(), WindowPoint::new(point.x, point.y)),
      delivery,
    }],
    confirmation,
  };
  emit_blind_select_completed(&result);
  Ok(result)
}

fn blind_select_via_api(request: BlindSelectRequest, config: BalatroModelConfig) -> Result<BlindSelectResult, CliError> {
  if request.slot.zone != ObjectZone::Blind {
    return Err(CliError::Message(format!("blind select requires a blind slot, got {}", request.slot)));
  }
  let runtime = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .map_err(|error| CliError::Message(format!("failed to create daemon client runtime: {error}")))?;
  let before = runtime.block_on(observe_live_via_api(&request.target, &config, None, true))?;
  let mut current = before.clone();
  let mut attempts = Vec::new();
  let mut confirmation = BlindSelectConfirmation::NotRequested;
  let attempt_limit = if request.confirm_started { 2 } else { 1 };
  for attempt in 0..attempt_limit {
    let selected_button = select_button_for_slot(&current.buttons, "button_level_select", Some(request.slot.index))?.clone();
    let frame_point = bbox_center_point(selected_button.bbox);
    let (screen_point, delivery) = runtime.block_on(click_display_frame_point_via_api(&current.frame, frame_point))?;
    attempts.push(BlindSelectAttempt {
      selected_button,
      point: ActionPoint::screen(screen_point),
      delivery,
    });
    if !request.confirm_started {
      break;
    }
    std::thread::sleep(Duration::from_millis(650));
    match runtime.block_on(observe_live_via_api(&request.target, &config, None, true)) {
      Ok(after) => {
        confirmation = evaluate_blind_select_confirmation(&before, Ok(&after));
        current = after;
        if matches!(confirmation, BlindSelectConfirmation::Started { .. }) {
          break;
        }
        if attempt + 1 < attempt_limit {
          std::thread::sleep(Duration::from_millis(350));
        }
      }
      Err(error) => {
        confirmation = evaluate_blind_select_confirmation(&before, Err(error.to_string()));
        break;
      }
    }
  }
  let result = BlindSelectResult {
    target: request.target,
    slot: request.slot,
    attempts,
    confirmation,
  };
  emit_blind_select_completed(&result);
  Ok(result)
}

#[cfg(not(target_os = "macos"))]
pub fn blind_select(_request: BlindSelectRequest) -> Result<BlindSelectResult, CliError> {
  Err(CliError::Message("blinds select live operation is only available on macOS".to_string()))
}

#[cfg(target_os = "macos")]
pub fn blind_skip(request: BlindSkipRequest) -> Result<BlindSkipResult, CliError> {
  let session = open_macos_session()?;
  let window = session.window().resolve(Window::main_visible().owned_by(App::name(request.target.clone())))?;
  let before_image = capture_window_to_temp(&session, &window, "blind-skip-before")?;
  let before = observe_image(&before_image, &BalatroModelConfig::default(), true);
  let _ = fs::remove_file(before_image);
  let before = before?;
  let selected_button = select_button_for_slot(&before.buttons, "button_level_skip", None)?.clone();
  let point = window_point_from_button(&before, &window, &selected_button);
  let delivery = click_game_point(&session, &window, point)?;

  let confirmation = if request.confirm_exit {
    match capture_observable_window(&session, &window, "blind-skip-after", request.timeout_ms, 500) {
      Ok((after_image, after)) => {
        let confirmation = evaluate_blind_skip_confirmation(&before, after.as_ref().map_err(|error| error.to_string()));
        let _ = fs::remove_file(after_image);
        confirmation
      }
      Err(error) => BlindSkipConfirmation::NotExited {
        before_phase: before.phase,
        after_phase: None,
        reason: BlindSkipConfirmationFailure::StateReadFailed {
          message: error.to_string(),
        },
      },
    }
  } else {
    BlindSkipConfirmation::NotRequested
  };

  let result = BlindSkipResult {
    target: request.target,
    selected_button,
    window_point: WindowPoint::new(point.x, point.y),
    delivery,
    confirmation,
  };
  emit_blind_skip_completed(&result);
  Ok(result)
}

#[cfg(not(target_os = "macos"))]
pub fn blind_skip(_request: BlindSkipRequest) -> Result<BlindSkipResult, CliError> {
  Err(CliError::Message("blinds skip live operation is only available on macOS".to_string()))
}

#[derive(Debug, Serialize)]
struct BlindSelectCliOutput<'a> {
  operation: &'static str,
  target: &'a str,
  slot: SlotId,
  confirmation: &'a BlindSelectConfirmation,
  attempts: &'a [BlindSelectAttempt],
}

fn write_blind_select_output(result: &BlindSelectResult) -> Result<(), CliError> {
  write_output(
    OutputMode::Json,
    &BlindSelectCliOutput {
      operation: "blinds.select",
      target: &result.target,
      slot: result.slot,
      confirmation: &result.confirmation,
      attempts: &result.attempts,
    },
  )
}

#[derive(Debug, Serialize)]
struct BlindSkipCliOutput<'a> {
  operation: &'static str,
  target: &'a str,
  delivery: &'a auv_driver::InputActionResult,
  confirmation: &'a BlindSkipConfirmation,
  selected_button: &'a ButtonTarget,
  window_point: WindowPoint,
}

fn write_blind_skip_output(result: &BlindSkipResult) -> Result<(), CliError> {
  write_output(
    OutputMode::Json,
    &BlindSkipCliOutput {
      operation: "blinds.skip",
      target: &result.target,
      delivery: &result.delivery,
      confirmation: &result.confirmation,
      selected_button: &result.selected_button,
      window_point: result.window_point,
    },
  )
}

#[cfg(target_os = "macos")]
fn observe_live_target(target: &str, config: &BalatroModelConfig, no_cache: bool) -> Result<BalatroState, CliError> {
  let session = open_macos_session()?;
  let window = session.window().resolve(Window::main_visible().owned_by(App::name(target.to_string())))?;
  let image = capture_window_to_temp(&session, &window, "observe-live")?;
  observe_image_with_ui_readings(&image, config, no_cache)
}

#[cfg(target_os = "macos")]
fn read_cards_live(
  target: &str,
  config: &BalatroModelConfig,
  no_cache: bool,
  requested: &Option<Vec<u32>>,
  frame_out: Option<&Path>,
) -> Result<Vec<CardReadResult>, CliError> {
  let session = open_macos_session()?;
  let window = session.window().resolve(Window::main_visible().owned_by(App::name(target.to_string())))?;
  let capture = capture_window(&session, &window)?;
  let frame = match frame_out {
    Some(path) => save_capture_to_path(&capture, path)?,
    None => save_capture_to_temp(&capture, "cards-read")?,
  };
  let state = observe_image_with_ui_readings(&frame, config, no_cache)?;
  let cards = select_cards_for_read(&state, requested)?;
  let rank_templates = load_deck_rank_templates();
  let original_mouse = auv_driver_macos::native::pointer::current_mouse_logical_point().ok();
  let mut used_hover = false;
  let results = cards
    .into_iter()
    .map(|card| {
      let mut result = read_card_from_capture(&session, &capture, &frame, &state, card, rank_templates.as_deref())?;
      if should_hover_reread_card(&result.reading)
        && let Some(hover) = hover_reread_card(&session, &window, config, no_cache, &state, card, rank_templates.as_deref())?
      {
        used_hover = true;
        result = better_card_read(result, hover);
      }
      Ok(result)
    })
    .collect::<Result<Vec<_>, CliError>>();
  if used_hover && let Some((x, y)) = original_mouse {
    let _ = auv_driver_macos::native::pointer::move_point(x, y, 0);
  }
  results
}

#[cfg(target_os = "macos")]
fn read_pack_live(args: &ObserveArgs) -> Result<PackReadOutput, CliError> {
  let config = BalatroModelConfig::from_observe_args(args);
  let session = open_macos_session()?;
  let window = session.window().resolve(Window::main_visible().owned_by(App::name(args.target.clone())))?;
  let capture = capture_window(&session, &window)?;
  let frame = save_capture_to_temp(&capture, "pack-read")?;
  let state = observe_image_with_ui_readings(&frame, &config, args.no_cache)?;
  let mut choices = active_pack_choices(&state).into_iter().map(pack_read_choice).collect::<Vec<_>>();
  let original_mouse = auv_driver_macos::native::pointer::current_mouse_logical_point().ok();

  for choice in &mut choices {
    if !choice.hover_required {
      continue;
    }
    if let Err(error) = hover_read_pack_choice(&session, &window, &state, choice) {
      choice.hover_error = Some(error.to_string());
    }
  }

  if let Some((x, y)) = original_mouse {
    let _ = auv_driver_macos::native::pointer::move_point(x, y, 0);
  }

  Ok(PackReadOutput {
    phase: state.phase,
    choices,
    skip_button: best_button(&state.buttons, "button_card_pack_skip").cloned(),
    frame: state.frame,
  })
}

#[cfg(target_os = "macos")]
fn hover_read_pack_choice(
  session: &auv_driver_macos::MacosDriverSession,
  window: &auv_driver::window::Window,
  state: &BalatroState,
  choice: &mut PackReadChoice,
) -> Result<(), CliError> {
  let point = window_point_from_frame_point(state, window, bbox_center_point(choice.choice.bbox));
  let screen = session.window().to_screen_point(window, WindowPoint::new(point.x, point.y))?;
  let screen = screen.point();
  auv_driver_macos::native::pointer::move_point(screen.x, screen.y, 0).map_err(CliError::Message)?;
  std::thread::sleep(Duration::from_millis(450));

  let capture = capture_window(session, window)?;
  let frame = save_capture_to_temp(&capture, "pack-hover-read")?;
  let region = pack_choice_hover_ocr_region();
  let recognition = session.vision().recognize_text_in_capture_with_options(
    &capture,
    region,
    TextRecognitionOptions::default().with_recognition_languages(["zh-Hans", "en-US"]).with_custom_words(pack_ocr_words()),
  )?;
  choice.hover_text = non_empty_trimmed_text(&recognition.text);
  choice.hover_frame = Some(frame.display().to_string());
  choice.hover_ocr_region = Some(region);
  Ok(())
}

#[cfg(target_os = "macos")]
fn hover_read_object(
  session: &auv_driver_macos::MacosDriverSession,
  window: &auv_driver::window::Window,
  state: &BalatroState,
  read: &mut ObjectReadResult,
) -> Result<(), CliError> {
  let point = window_point_from_frame_point(state, window, bbox_center_point(read.bbox));
  let screen = session.window().to_screen_point(window, WindowPoint::new(point.x, point.y))?;
  let screen = screen.point();
  auv_driver_macos::native::pointer::move_point(screen.x, screen.y, 0).map_err(CliError::Message)?;
  std::thread::sleep(Duration::from_millis(450));

  let capture = capture_window(session, window)?;
  let frame = save_capture_to_temp(&capture, "object-hover-read")?;
  let region = object_hover_ocr_region();
  let recognition = session.vision().recognize_text_in_capture_with_options(
    &capture,
    region,
    TextRecognitionOptions::default().with_recognition_languages(["zh-Hans", "en-US"]).with_custom_words(object_ocr_words()),
  )?;

  if let Some(text) = non_empty_trimmed_text(&recognition.text) {
    read.reading = ObjectReadValue {
      status: "read",
      raw_text: Some(text),
      confidence: None,
    };
    read.evidence.hover_required = false;
  }
  read.evidence.source = "hover_ocr".to_string();
  read.evidence.hover_frame = Some(frame.display().to_string());
  read.evidence.hover_ocr_region = Some(region);
  Ok(())
}

#[cfg(target_os = "macos")]
fn click_game_point(
  session: &auv_driver_macos::MacosDriverSession,
  window: &auv_driver::window::Window,
  point: Point,
) -> Result<auv_driver::InputActionResult, CliError> {
  let delivery = session.window().click(
    window,
    WindowPoint::new(point.x, point.y),
    ClickOptions {
      policy: InputPolicy::ForegroundPreferred,
      ..ClickOptions::default()
    },
  )?;
  emit_input_delivery(&delivery);
  Ok(delivery)
}

#[cfg(target_os = "macos")]
fn read_card_from_capture(
  session: &auv_driver_macos::MacosDriverSession,
  capture: &Capture,
  frame: &Path,
  state: &BalatroState,
  card: &CardSlot,
  rank_templates: Option<&[RankTemplate]>,
) -> Result<CardReadResult, CliError> {
  let region = ocr_region_for_card(state, card);
  let corner_capture = card_corner_capture(capture, state, card);
  let recognition = session.vision().recognize_text_in_capture_with_options(
    &corner_capture,
    RatioRect::new(0.0, 0.0, 1.0, 1.0),
    TextRecognitionOptions::default().with_custom_words(card_ocr_words()).with_recognition_languages(["zh-Hans", "en-US"]),
  )?;
  let crop = save_capture_to_temp(&corner_capture, "card-corner")?;
  let suit = infer_suit_from_card_corner(capture, state, card);
  let mut source = "macos_vision_corner_ocr".to_string();
  let mut reading = parse_card_reading(&recognition.text, suit, None);
  if reading.rank.is_none()
    && let Some((rank, confidence)) = best_rank_candidate_from_deck_template(&corner_capture.image, rank_templates, suit)
  {
    reading.rank_candidate = Some(rank.clone());
    reading.rank_candidate_confidence = Some(confidence);
    // TODO(balatro-rank-template-calibration): Do not promote this candidate
    // into verified card content until a committed fixed-frame corpus proves
    // rank accuracy across scaled and perspective captures.
    source = format!("{source}+deck_template_rank_candidate");
  }
  Ok(CardReadResult {
    slot: card.slot,
    bbox: card.bbox,
    confidence: card.confidence,
    reading,
    evidence: CardReadEvidence {
      frame: frame.display().to_string(),
      ocr_region: region,
      corner_crop: Some(crop),
      source,
    },
  })
}

#[cfg(target_os = "macos")]
fn hover_reread_card(
  session: &auv_driver_macos::MacosDriverSession,
  window: &auv_driver::window::Window,
  config: &BalatroModelConfig,
  no_cache: bool,
  state: &BalatroState,
  card: &CardSlot,
  rank_templates: Option<&[RankTemplate]>,
) -> Result<Option<CardReadResult>, CliError> {
  let point = window_point_from_frame_point(state, window, bbox_center_point(card.bbox));
  let screen = session.window().to_screen_point(window, WindowPoint::new(point.x, point.y))?;
  let screen = screen.point();
  auv_driver_macos::native::pointer::move_point(screen.x, screen.y, 0).map_err(CliError::Message)?;
  std::thread::sleep(Duration::from_millis(350));

  let capture = capture_window(session, window)?;
  let frame = save_capture_to_temp(&capture, "cards-hover-read")?;
  let hover_state = observe_image_with_ui_readings(&frame, config, no_cache)?;
  let hover_card = match select_hand_card(&hover_state, card.slot.index) {
    Ok(card) => card,
    Err(_) => return Ok(None),
  };
  let mut result = read_card_from_capture(session, &capture, &frame, &hover_state, hover_card, rank_templates)?;
  result.evidence.source = format!("{}+hover_reread", result.evidence.source);
  Ok(Some(result))
}

fn better_card_read(original: CardReadResult, hover: CardReadResult) -> CardReadResult {
  if card_read_score(&hover.reading) > card_read_score(&original.reading) {
    hover
  } else {
    original
  }
}

fn card_read_score(reading: &CardReadValue) -> (u8, u8) {
  let completeness = match (reading.rank.is_some(), reading.suit.is_some(), reading.valid) {
    (_, _, true) => 3,
    (true, false, false) | (false, true, false) => 2,
    _ => 1,
  };
  let confidence = reading.confidence.map(|confidence| (confidence.clamp(0.0, 1.0) * 100.0).round() as u8).unwrap_or(0);
  (completeness, confidence)
}

#[cfg(target_os = "macos")]
fn capture_observable_window(
  session: &auv_driver_macos::MacosDriverSession,
  window: &auv_driver::window::Window,
  label: &str,
  timeout_ms: u64,
  initial_delay_ms: u64,
) -> Result<(PathBuf, Result<BalatroState, ObservationError>), CliError> {
  let timeout = Duration::from_millis(timeout_ms);
  let deadline = Instant::now() + timeout;
  let mut delay = Duration::from_millis(initial_delay_ms.min(timeout_ms));

  loop {
    if !delay.is_zero() {
      std::thread::sleep(delay);
    }

    let image = capture_window_to_temp(session, window, label)?;
    match observe_image(&image, &BalatroModelConfig::default(), true).map(|mut state| {
      enrich_ui_numeric_readings_from_image(&mut state, &image);
      state
    }) {
      Ok(state) => return Ok((image, Ok(state))),
      Err(error) if Instant::now() >= deadline => return Ok((image, Err(error))),
      Err(_) => {
        delay = Duration::from_millis(250);
      }
    }
  }
}

fn capture_from_image(image: &Path) -> Result<Capture, CliError> {
  let rgba = image::open(image)?.to_rgba8();
  let width = rgba.width();
  let height = rgba.height();
  Ok(Capture {
    image: rgba,
    bounds: Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
    scale_factor: 1.0,
    backend: "image-file".to_string(),
    fallback_reason: None,
  })
}

#[cfg(not(target_os = "macos"))]
fn observe_live_target(_target: &str, _config: &BalatroModelConfig, _no_cache: bool) -> Result<BalatroState, CliError> {
  Err(CliError::MissingImage)
}

#[cfg(not(target_os = "macos"))]
fn read_cards_live(
  _target: &str,
  _config: &BalatroModelConfig,
  _no_cache: bool,
  _requested: &Option<Vec<u32>>,
  _frame_out: Option<&Path>,
) -> Result<Vec<CardReadResult>, CliError> {
  Err(CliError::MissingImage)
}

#[cfg(target_os = "macos")]
fn capture_window(session: &auv_driver_macos::MacosDriverSession, window: &auv_driver::window::Window) -> Result<Capture, CliError> {
  match session.window().capture_with(
    window,
    CaptureOptions {
      activation: Activation::ActivateFirst {
        settle: Duration::from_millis(250),
      },
      ..CaptureOptions::default()
    },
  ) {
    Ok(capture) => Ok(capture),
    Err(error) => capture_window_via_display_region(session, window, error.to_string()),
  }
}

#[cfg(target_os = "macos")]
fn capture_window_via_display_region(
  session: &auv_driver_macos::MacosDriverSession,
  window: &auv_driver::window::Window,
  primary_error: String,
) -> Result<Capture, CliError> {
  // NOTICE: Balatro/love can make ScreenCaptureKit window capture time out
  // while display-region capture still works. Keep this fallback local to the
  // game surface until the shared capture contract can expose backend choices
  // explicitly.
  let mut region = window.frame;
  region.origin.x = region.origin.x.round();
  region.origin.y = region.origin.y.round();
  region.size.width = region.size.width.round();
  region.size.height = region.size.height.round();
  let mut capture = session
    .display()
    .capture_region(CaptureOptions {
      activation: Activation::KeepCurrent,
      region: Some(region),
      ..CaptureOptions::default()
    })
    .map_err(|fallback_error| {
      CliError::Message(format!("window capture failed ({primary_error}); display-region fallback also failed ({fallback_error})"))
    })?
    .capture;
  capture.backend = format!("{}:window-frame-fallback", capture.backend);
  capture.fallback_reason = Some(primary_error);
  Ok(capture)
}

#[cfg(target_os = "macos")]
fn save_capture_to_path(capture: &Capture, path: &Path) -> Result<PathBuf, CliError> {
  if let Some(parent) = path.parent()
    && !parent.as_os_str().is_empty()
  {
    fs::create_dir_all(parent)?;
  }
  capture.image.save(path)?;
  Ok(path.to_path_buf())
}

#[cfg(target_os = "macos")]
fn save_capture_to_temp(capture: &Capture, prefix: &str) -> Result<PathBuf, CliError> {
  let path = std::env::temp_dir().join(format!("auv-game-balatro-{prefix}-{}-{}.png", std::process::id(), unique_nanos()));
  capture.image.save(&path)?;
  Ok(path)
}

#[cfg(target_os = "macos")]
fn capture_window_to_temp(
  session: &auv_driver_macos::MacosDriverSession,
  window: &auv_driver::window::Window,
  prefix: &str,
) -> Result<PathBuf, CliError> {
  let capture = capture_window(session, window)?;
  save_capture_to_temp(&capture, prefix)
}

fn unique_nanos() -> u128 {
  SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_nanos()).unwrap_or_default()
}

fn find_button<'a>(state: &'a BalatroState, id: &str) -> Result<&'a ButtonTarget, CliError> {
  select_button_for_slot(&state.buttons, id, None)
}

fn resolve_card_commit_target(state: &BalatroState, kind: CardCommitKind) -> Result<(CardCommitControl, Point), CliError> {
  if let Ok(button) = find_button(state, kind.button_id()) {
    let button = button.clone();
    let point = bbox_center_point(button.bbox);
    return Ok((CardCommitControl::DetectedButton { button }, point));
  }

  if state.phase != BalatroPhase::Playing || state.hand.is_empty() {
    return Err(CliError::Message(format!("could not find {} without an observed playing hand", kind.button_id())));
  }
  let sort_rank = best_button(&state.buttons, "button_sort_hand_rank")
    .ok_or_else(|| CliError::Message(format!("could not find {} or button_sort_hand_rank", kind.button_id())))?
    .clone();
  let sort_suits = best_button(&state.buttons, "button_sort_hand_suits")
    .ok_or_else(|| CliError::Message(format!("could not find {} or button_sort_hand_suits", kind.button_id())))?
    .clone();
  let rank_center = bbox_center_point(sort_rank.bbox);
  let suits_center = bbox_center_point(sort_suits.bbox);
  let frame_width = f64::from(state.frame.image_size.width.max(1));
  let frame_height = f64::from(state.frame.image_size.height.max(1));
  if sort_rank.bbox.x1 >= sort_suits.bbox.x1
    || (rank_center.y - suits_center.y).abs() > frame_height * 0.04
    || rank_center.y < frame_height * 0.65
  {
    return Err(CliError::Message(format!(
      "refusing {} layout fallback because Sort Hand controls are not a plausible lower-screen pair",
      kind.button_id()
    )));
  }

  let group_left = f64::from(sort_rank.bbox.x1);
  let group_right = f64::from(sort_suits.bbox.x2);
  let group_width = group_right - group_left;
  if group_width <= frame_width * 0.03 || group_width >= frame_width * 0.20 {
    return Err(CliError::Message(format!("refusing {} layout fallback because Sort Hand control width is implausible", kind.button_id())));
  }
  // NOTICE: Balatro places Play Hand and Discard immediately beside the
  // two-button Sort Hand group. The 0.75 group-width offset is derived from a
  // live 2560x1440 Linux capture (Sort Hand x=1112..1283, Play center x≈984,
  // Discard center x≈1411). Keep the detected Sort Hand pair as the safety
  // anchor; remove this fallback once the UI model reliably emits both active
  // commit controls across live themes and scales.
  let x = match kind {
    CardCommitKind::Play => group_left - group_width * 0.75,
    CardCommitKind::Discard => group_right + group_width * 0.75,
  }
  .clamp(0.0, frame_width - 1.0);
  let y = ((rank_center.y + suits_center.y) / 2.0).clamp(0.0, frame_height - 1.0);
  Ok((
    CardCommitControl::PlayingHandLayout {
      sort_rank,
      sort_suits,
    },
    Point::new(x, y),
  ))
}

fn select_store_buy_confirm_button(state: &BalatroState) -> Result<&ButtonTarget, CliError> {
  best_button(&state.buttons, "button_purchase")
    .or_else(|| best_button(&state.buttons, "button_use"))
    .ok_or_else(|| CliError::Message("could not find button_purchase or button_use in selected store item frame".to_string()))
}

fn best_button<'a>(buttons: &'a [ButtonTarget], id: &str) -> Option<&'a ButtonTarget> {
  buttons
    .iter()
    .filter(|button| button.id == id)
    .max_by(|left, right| left.confidence.partial_cmp(&right.confidence).unwrap_or(std::cmp::Ordering::Equal))
}

fn restart_primary_button(buttons: &[ButtonTarget]) -> Option<&ButtonTarget> {
  best_button(buttons, "button_new_run")
    .or_else(|| best_button(buttons, "button_new_run_play"))
    .or_else(|| best_button(buttons, "button_main_menu_play"))
}

fn resolve_store_next_round_target(state: &BalatroState) -> Result<StoreNextRoundTarget, CliError> {
  if let Some(button) = best_button(&state.buttons, "button_store_next_round") {
    return Ok(StoreNextRoundTarget::DetectedButton {
      button: button.clone(),
    });
  }

  let has_store_evidence = has_store_layout_evidence(state);
  let purchase_visible = best_button(&state.buttons, "button_purchase").is_some();
  let pack_choices_visible = best_button(&state.buttons, "button_card_pack_skip").is_some() || !active_pack_choices(state).is_empty();
  if has_store_evidence && !purchase_visible && !pack_choices_visible {
    // Store phase classification or the derived store flag is the evidence
    // that keeps this fallback scoped to the live store screen; a visible
    // purchase button means a store item is selected, so this point is unsafe.
    // NOTICE: Fallback ratios come from a live 1646x963 store capture point
    // around (594,429), normalized to (0.361,0.446). Remove this once stable
    // YOLO/button target detection covers `button_store_next_round`.
    return Ok(StoreNextRoundTarget::StoreLayout {
      frame_point: Point::new(f64::from(state.frame.image_size.width) * 0.361, f64::from(state.frame.image_size.height) * 0.446),
    });
  }

  Err(CliError::Message("could not find button_store_next_round in observed Balatro frame".to_string()))
}

fn resolve_consumable_use_target(state: &BalatroState, slot_index: u32) -> Result<(ConsumableUseControl, Point), CliError> {
  if let Some(button) = best_button(&state.buttons, "button_use") {
    return Ok((
      ConsumableUseControl::DetectedButton {
        button: button.clone(),
      },
      bbox_center_point(button.bbox),
    ));
  }

  let consumable = select_consumable(state, slot_index)?;
  let width = consumable.bbox.width().max(1.0);
  let height = consumable.bbox.height().max(1.0);
  let x = (consumable.bbox.x2 + width * 0.32).min(state.frame.image_size.width as f32 - 1.0);
  let y = (consumable.bbox.y1 + height * 0.60).min(state.frame.image_size.height as f32 - 1.0);
  Ok((ConsumableUseControl::SelectedConsumableLayout, Point::new(f64::from(x), f64::from(y))))
}

fn has_store_layout_evidence(state: &BalatroState) -> bool {
  if state.phase == BalatroPhase::Store || state.store.is_store || !state.store.items.is_empty() {
    return true;
  }
  if has_empty_store_shell_evidence(state) {
    return true;
  }

  let width = state.frame.image_size.width.max(1) as f32;
  let height = state.frame.image_size.height.max(1) as f32;
  state.raw_entities.iter().any(|evidence| {
    let detection = &evidence.detection;
    let center_x = (detection.bbox.x1 + detection.bbox.x2) / 2.0;
    let center_y = (detection.bbox.y1 + detection.bbox.y2) / 2.0;
    matches!(detection.label.as_str(), "joker_card" | "tarot_card" | "planet_card" | "card_pack")
      && center_x > width * 0.42
      && center_x < width * 0.82
      && center_y > height * 0.35
      && center_y < height * 0.96
  })
}

fn has_empty_store_shell_evidence(state: &BalatroState) -> bool {
  state.phase == BalatroPhase::Unknown
    && state.hand.is_empty()
    && best_button(&state.buttons, "button_run_info").is_some()
    && best_button(&state.buttons, "button_options").is_some()
    && best_button(&state.buttons, "button_purchase").is_none()
    && best_button(&state.buttons, "button_card_pack_skip").is_none()
    && best_button(&state.buttons, "button_cash_out").is_none()
    && best_button(&state.buttons, "button_level_select").is_none()
    && best_button(&state.buttons, "button_level_skip").is_none()
}

fn resolve_pack_confirm_target(state: &BalatroState, choice: &PackChoice) -> Result<(PackChooseControl, Point), CliError> {
  if let Some(button) = best_button(&state.buttons, "button_use") {
    return Ok((
      PackChooseControl::DetectedButton {
        button: button.clone(),
      },
      bbox_center_point(button.bbox),
    ));
  }

  if best_button(&state.buttons, "button_card_pack_skip").is_none() && active_pack_choices(state).is_empty() {
    return Err(CliError::Message("could not resolve pack confirm target without active pack evidence".to_string()));
  }

  // NOTICE: The 0.82 height fallback comes from live active-pack captures where
  // Balatro places the confirm button below the selected choice. Remove this
  // fallback once `button_use` detection is stable for active pack selections.
  Ok((
    PackChooseControl::ActivePackLayout,
    Point::new(f64::from((choice.bbox.x1 + choice.bbox.x2) / 2.0), f64::from(state.frame.image_size.height) * 0.82),
  ))
}

fn blind_buttons(state: &BalatroState) -> Vec<&ButtonTarget> {
  let mut buttons =
    state.buttons.iter().filter(|button| matches!(button.id.as_str(), "button_level_select" | "button_level_skip")).collect::<Vec<_>>();
  buttons.sort_by(|left, right| left.bbox.x1.partial_cmp(&right.bbox.x1).unwrap_or(std::cmp::Ordering::Equal));
  buttons
}

fn select_button_for_slot<'a>(buttons: &'a [ButtonTarget], id: &str, slot_index: Option<u32>) -> Result<&'a ButtonTarget, CliError> {
  let mut matches = buttons.iter().filter(|button| button.id == id).collect::<Vec<_>>();

  if let Some(index) = slot_index {
    matches.sort_by(|left, right| left.bbox.x1.partial_cmp(&right.bbox.x1).unwrap_or(std::cmp::Ordering::Equal));
    matches.get(index as usize).copied().ok_or_else(|| CliError::Message(format!("could not find {id} at blind:{index}")))
  } else {
    matches
      .into_iter()
      .max_by(|left, right| left.confidence.partial_cmp(&right.confidence).unwrap_or(std::cmp::Ordering::Equal))
      .ok_or_else(|| CliError::Message(format!("could not find {id} in observed Balatro frame")))
  }
}

fn parse_blind_slot_index(slot: &str) -> Result<u32, CliError> {
  let Some(index) = slot.strip_prefix("blind:") else {
    return Err(CliError::Message(format!("blind select requires --slot blind:N, got {slot}")));
  };
  index.parse::<u32>().map_err(|_| CliError::Message(format!("blind slot index must be an integer, got {slot}")))
}

fn parse_prefixed_slot_index(slot: &str, prefix: &str) -> Result<u32, CliError> {
  let slot = slot.trim();
  let expected = format!("{prefix}:");
  let Some(index) = slot.strip_prefix(&expected) else {
    return Err(CliError::Message(format!("object operation requires --slot {prefix}:N, got {slot}")));
  };
  index.parse::<u32>().map_err(|_| CliError::Message(format!("{prefix} slot index must be an integer, got {slot}")))
}

fn parse_store_slot_index(slot: &str) -> Result<u32, CliError> {
  parse_prefixed_slot_index(slot, "store")
}

fn parse_joker_slot_index(slot: &str) -> Result<u32, CliError> {
  parse_prefixed_slot_index(slot, "joker")
}

fn parse_consumable_slot_index(slot: &str) -> Result<u32, CliError> {
  parse_prefixed_slot_index(slot, "consumable")
}

fn parse_pack_slot_index(slot: &str) -> Result<u32, CliError> {
  parse_prefixed_slot_index(slot, "pack")
}

fn parse_hand_slot_indices(slots: &str) -> Result<Vec<u32>, CliError> {
  slots
    .split(',')
    .map(|slot| {
      let slot = slot.trim();
      let Some(index) = slot.strip_prefix("hand:") else {
        return Err(CliError::Message(format!("card operation requires --slots hand:N[,hand:N...], got {slot}")));
      };
      index.parse::<u32>().map_err(|_| CliError::Message(format!("hand slot index must be an integer, got {slot}")))
    })
    .collect()
}

fn parse_hand_target_indices(targets: &[String]) -> Result<Vec<u32>, CliError> {
  targets
    .iter()
    .map(|target| {
      parse_prefixed_slot_index(target, "hand")
        .map_err(|_| CliError::Message(format!("targeted consumable operation requires --targets hand:N[,hand:N...], got {target}")))
    })
    .collect()
}

fn validate_hand_slots(slots: &[SlotId], operation: &str) -> Result<(), CliError> {
  if slots.is_empty() {
    return Err(CliError::Message(format!("{operation} requires at least one hand slot")));
  }
  if let Some(slot) = slots.iter().find(|slot| slot.zone != ObjectZone::Hand) {
    return Err(CliError::Message(format!("{operation} requires hand slots, got {slot}")));
  }
  if let Some(slot) = slots.iter().enumerate().find_map(|(index, slot)| slots[..index].contains(slot).then_some(slot)) {
    return Err(CliError::Message(format!("{operation} received duplicate slot {slot}")));
  }
  Ok(())
}

fn parse_card_read_slots(slot: &str) -> Result<Option<Vec<u32>>, CliError> {
  let slot = slot.trim();
  if matches!(slot, "all" | "hand:all") {
    return Ok(None);
  }
  parse_hand_slot_indices(slot).map(Some)
}

fn enrich_ui_numeric_readings_from_image(state: &mut BalatroState, image: &Path) {
  let Ok(image) = image::open(image).map(|image| image.to_rgba8()) else {
    return;
  };
  let crops = state
    .raw_ui
    .iter()
    .filter_map(|evidence| {
      let label = evidence.detection.label.as_str();
      if !matches!(state.phase, BalatroPhase::Playing | BalatroPhase::CashOut) && is_score_ui_label(label) {
        return None;
      }
      is_numeric_ui_label(label)
        .then(|| crop_detection_to_temp(&image, evidence.detection.bbox, label).map(|crop| (label.to_string(), crop)))?
    })
    .collect::<Vec<_>>();

  for (label, crop) in crops {
    if is_single_ui_digit_label(&label)
      && let Some(digit) = infer_single_ui_digit_from_crop(&crop)
      && is_allowed_single_ui_digit(&label, digit)
    {
      apply_ui_numeric_reading(&label, &digit.to_string(), &mut state.scores, &mut state.rounds);
      continue;
    }
    if use_score_digit_reader(&label)
      && let Some(text) = infer_ui_digit_text_from_crop_with_foreground(&crop, score_ui_digit_foreground(&label))
      && let Some(text) = ui_digit_text_for_label(&label, &text)
    {
      apply_ui_numeric_reading(&label, &text, &mut state.scores, &mut state.rounds);
      continue;
    }
    // TODO(balatro-first-party-ocr): cash and other non-glyph numeric fields
    // need a real OCR boundary. Deferred until AUV owns or selects a
    // first-party OCR tool instead of invoking owner-local Python sidecars.
  }
}

fn is_allowed_single_ui_digit(label: &str, digit: u8) -> bool {
  match label {
    "ui_data_discards_left" | "ui_data_hands_left" => digit <= 5,
    "ui_round_ante_current" | "ui_round_ante_left" => (1..=8).contains(&digit),
    "ui_round_round_current" | "ui_round_round_left" => digit <= 8,
    _ => true,
  }
}

fn ui_digit_text_for_label(label: &str, digits: &str) -> Option<String> {
  if digits.is_empty() || !digits.chars().all(|character| character.is_ascii_digit()) {
    return None;
  }
  match label {
    "ui_score_mult" => Some(format!("x{digits}")),
    "ui_score_round_score" => {
      let score_digits = digits.strip_prefix('0').unwrap_or(digits);
      Some(
        if score_digits.is_empty() {
          "0"
        } else {
          score_digits
        }
        .to_string(),
      )
    }
    "ui_score_chips" | "ui_score_current" | "ui_score_target_score" => Some(digits.to_string()),
    _ => None,
  }
}

fn score_ui_digit_foreground(label: &str) -> UiDigitForeground {
  match label {
    "ui_score_target_score" => UiDigitForeground::Colored,
    _ => UiDigitForeground::White,
  }
}

fn use_score_digit_reader(label: &str) -> bool {
  matches!(label, "ui_score_chips" | "ui_score_current" | "ui_score_mult" | "ui_score_round_score")
}

fn infer_single_ui_digit_from_crop(crop: &Path) -> Option<u8> {
  let image = image::open(crop).ok()?.to_rgba8();
  let reading = infer_ui_digit_text_from_image_with_foreground(&image, UiDigitForeground::Colored)?;
  let mut chars = reading.chars();
  let digit = chars.next()?.to_digit(10)? as u8;
  chars.next().is_none().then_some(digit)
}

fn infer_ui_digit_text_from_crop_with_foreground(crop: &Path, foreground: UiDigitForeground) -> Option<String> {
  let image = image::open(crop).ok()?.to_rgba8();
  infer_ui_digit_text_from_image_with_foreground(&image, foreground)
}

fn infer_ui_digit_text_from_image_with_foreground(image: &RgbaImage, foreground: UiDigitForeground) -> Option<String> {
  let mut digits = String::new();
  for points in ui_digit_glyph_segments(image, foreground)? {
    let mask = normalized_ui_digit_mask_from_points(points)?;
    let digit = infer_ui_digit_from_mask(&mask)?;
    digits.push(char::from(b'0' + digit));
  }
  (!digits.is_empty()).then_some(digits)
}

fn infer_ui_digit_from_mask(mask: &[bool; UI_DIGIT_MASK_CELLS]) -> Option<u8> {
  let mut candidates =
    UI_DIGIT_TEMPLATES.iter().map(|template| (template.digit, ui_digit_mask_distance(mask, template.rows))).collect::<Vec<_>>();
  candidates.sort_by(|left, right| left.1.partial_cmp(&right.1).unwrap_or(std::cmp::Ordering::Equal));
  let (digit, distance) = *candidates.first()?;
  let runner_up = candidates.get(1).map(|candidate| candidate.1).unwrap_or(1.0);
  // A wrong number is more harmful than an unread score. Require both an
  // absolute template fit and separation from the next-best digit; live
  // evidence showed Balatro's scaled `5` otherwise being reported as `3`.
  (distance <= 0.32 && runner_up - distance >= 0.04).then_some(digit)
}

fn normalized_ui_digit_mask_from_points(foreground: Vec<(u32, u32)>) -> Option<[bool; UI_DIGIT_MASK_CELLS]> {
  let min_x = foreground.iter().map(|(x, _)| *x).min()?;
  let min_y = foreground.iter().map(|(_, y)| *y).min()?;
  let max_x = foreground.iter().map(|(x, _)| *x).max()?;
  let max_y = foreground.iter().map(|(_, y)| *y).max()?;
  let width = (max_x - min_x + 1).max(1);
  let height = (max_y - min_y + 1).max(1);
  let foreground = foreground.into_iter().collect::<std::collections::HashSet<_>>();
  let mut mask = [false; UI_DIGIT_MASK_CELLS];
  for ty in 0..UI_DIGIT_MASK_H {
    for tx in 0..UI_DIGIT_MASK_W {
      let x_start = min_x + (tx as u32 * width / UI_DIGIT_MASK_W as u32);
      let x_end = min_x + ((tx as u32 + 1) * width / UI_DIGIT_MASK_W as u32).max(1);
      let y_start = min_y + (ty as u32 * height / UI_DIGIT_MASK_H as u32);
      let y_end = min_y + ((ty as u32 + 1) * height / UI_DIGIT_MASK_H as u32).max(1);
      let mut hits = 0_u32;
      let mut total = 0_u32;
      for y in y_start..=y_end.min(max_y) {
        for x in x_start..=x_end.min(max_x) {
          total += 1;
          if foreground.contains(&(x, y)) {
            hits += 1;
          }
        }
      }
      mask[ty * UI_DIGIT_MASK_W + tx] = total > 0 && hits as f32 / total as f32 >= 0.18;
    }
  }
  Some(mask)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UiDigitForeground {
  Colored,
  White,
}

fn ui_digit_glyph_segments(image: &RgbaImage, foreground: UiDigitForeground) -> Option<Vec<Vec<(u32, u32)>>> {
  let width = image.width();
  let height = image.height();
  let mut columns = vec![Vec::<(u32, u32)>::new(); width as usize];
  for y in 0..height {
    for x in 0..width {
      if is_ui_digit_pixel(image.get_pixel(x, y).0, foreground) {
        columns[x as usize].push((x, y));
      }
    }
  }

  let mut segments = Vec::new();
  let mut current = Vec::new();
  let mut empty_columns = 0_u32;
  for column in columns {
    if column.is_empty() {
      if !current.is_empty() {
        empty_columns += 1;
      }
      if empty_columns >= 3 && !current.is_empty() {
        segments.push(std::mem::take(&mut current));
        empty_columns = 0;
      }
      continue;
    }
    if empty_columns > 0 && empty_columns < 3 {
      empty_columns = 0;
    }
    current.extend(column);
  }
  if !current.is_empty() {
    segments.push(current);
  }

  let mut segments = segments.into_iter().filter(|segment| segment.len() >= 20).collect::<Vec<_>>();
  if let Some(max_height) = segments.iter().map(|segment| segment_height(segment)).max() {
    // Score crops can include commas, chip icons, or small UI fragments. The
    // digit templates are scale-invariant, so size filtering has to happen
    // before mask matching or those fragments may become plausible digits.
    let min_digit_height = (max_height as f32 * 0.72).ceil() as u32;
    segments.retain(|segment| segment_height(segment) >= min_digit_height);
  }
  (!segments.is_empty()).then_some(segments)
}

fn segment_height(segment: &[(u32, u32)]) -> u32 {
  let min_y = segment.iter().map(|(_, y)| *y).min().unwrap_or(0);
  let max_y = segment.iter().map(|(_, y)| *y).max().unwrap_or(min_y);
  max_y - min_y + 1
}

fn is_ui_digit_pixel([r, g, b, a]: [u8; 4], foreground: UiDigitForeground) -> bool {
  if a < 80 {
    return false;
  }
  let max = r.max(g).max(b);
  let min = r.min(g).min(b);
  if foreground == UiDigitForeground::White {
    return min > 145 && max > 180;
  }
  let green_label = g > r.saturating_add(18) && g > b.saturating_add(18);
  max > 75 && max - min > 35 && !green_label
}

fn ui_digit_mask_distance(mask: &[bool; UI_DIGIT_MASK_CELLS], rows: [&str; 7]) -> f32 {
  let mut different = 0;
  for (row_index, row) in rows.iter().enumerate() {
    for (column_index, character) in row.chars().enumerate() {
      let expected = character == '#';
      if mask[row_index * UI_DIGIT_MASK_W + column_index] != expected {
        different += 1;
      }
    }
  }
  different as f32 / UI_DIGIT_MASK_CELLS as f32
}

const UI_DIGIT_MASK_W: usize = 5;
const UI_DIGIT_MASK_H: usize = 7;
const UI_DIGIT_MASK_CELLS: usize = UI_DIGIT_MASK_W * UI_DIGIT_MASK_H;

struct UiDigitTemplate {
  digit: u8,
  rows: [&'static str; 7],
}

const UI_DIGIT_TEMPLATES: &[UiDigitTemplate] = &[
  UiDigitTemplate {
    digit: 0,
    rows: [
      ".###.", "#####", "##.##", "##.##", "##.##", "#####", ".###.",
    ],
  },
  UiDigitTemplate {
    digit: 1,
    rows: [
      "####.", "####.", ".###.", ".###.", ".###.", "#####", "#####",
    ],
  },
  UiDigitTemplate {
    digit: 2,
    rows: [
      "#####", "....#", "....#", "#####", "#....", "#....", "#####",
    ],
  },
  UiDigitTemplate {
    digit: 3,
    rows: [
      "#####", "#####", ".####", ".####", "...##", "#####", "#####",
    ],
  },
  UiDigitTemplate {
    digit: 4,
    rows: [
      "##.##", "##.##", "##.##", "#####", ".####", "...##", "...##",
    ],
  },
  UiDigitTemplate {
    digit: 5,
    rows: [
      "#####", "#####", "####.", "#####", "...##", "#####", "#####",
    ],
  },
  UiDigitTemplate {
    digit: 6,
    rows: [
      "#####", "#....", "#....", "#####", "#...#", "#...#", "#####",
    ],
  },
  UiDigitTemplate {
    digit: 7,
    rows: [
      "#####", "....#", "....#", "...#.", "..#..", ".#...", ".#...",
    ],
  },
  UiDigitTemplate {
    digit: 8,
    rows: [
      "#####", "#...#", "#...#", "#####", "#...#", "#...#", "#####",
    ],
  },
  UiDigitTemplate {
    digit: 9,
    rows: [
      "#####", "#...#", "#...#", "#####", "....#", "....#", "#####",
    ],
  },
];

fn crop_detection_to_temp(image: &RgbaImage, bbox: BoundingBox, label: &str) -> Option<PathBuf> {
  let image_w = image.width().max(1);
  let image_h = image.height().max(1);
  let pad_x = (bbox.width().max(1.0) * 0.08).ceil();
  let pad_y = (bbox.height().max(1.0) * 0.12).ceil();
  let x1 = (bbox.x1 - pad_x).floor().max(0.0) as u32;
  let y1 = (bbox.y1 - pad_y).floor().max(0.0) as u32;
  let x2 = (bbox.x2 + pad_x).ceil().min(image_w as f32) as u32;
  let y2 = (bbox.y2 + pad_y).ceil().min(image_h as f32) as u32;
  if x2 <= x1 || y2 <= y1 {
    return None;
  }
  let crop = image::imageops::crop_imm(image, x1, y1, x2 - x1, y2 - y1).to_image();
  let resized = image::imageops::resize(
    &crop,
    (x2 - x1).saturating_mul(4).max(1),
    (y2 - y1).saturating_mul(4).max(1),
    image::imageops::FilterType::Nearest,
  );
  let path = std::env::temp_dir().join(format!("auv-game-balatro-ui-{}-{}-{}.png", label, std::process::id(), unique_nanos()));
  resized.save(&path).ok()?;
  Some(path)
}

fn select_hand_cards<'a>(state: &'a BalatroState, slot_indices: &[u32]) -> Result<Vec<&'a CardSlot>, CliError> {
  slot_indices.iter().map(|index| select_hand_card(state, *index)).collect()
}

fn select_hand_card(state: &BalatroState, slot_index: u32) -> Result<&CardSlot, CliError> {
  state.hand.get(slot_index as usize).ok_or_else(|| CliError::Message(format!("could not find hand:{slot_index}")))
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct HandCardInteraction {
  slot: SlotId,
  bbox: BoundingBox,
  confidence: f32,
  selected: bool,
  click_frame_point: Point,
  visual_fingerprint: Option<String>,
}

fn hand_card_interactions(state: &BalatroState) -> Vec<HandCardInteraction> {
  state
    .hand
    .iter()
    .map(|card| HandCardInteraction {
      slot: card.slot,
      bbox: card.bbox,
      confidence: card.confidence,
      selected: hand_slot_is_selected(state, card.slot.index),
      click_frame_point: hand_card_click_frame_point(state, card),
      visual_fingerprint: card.cache.visual_fingerprint.clone(),
    })
    .collect()
}

fn selected_hand_slot_indices(state: &BalatroState, requested: &[u32]) -> Vec<u32> {
  hand_card_interactions(state)
    .into_iter()
    .filter(|interaction| requested.contains(&interaction.slot.index) && interaction.selected)
    .map(|interaction| interaction.slot.index)
    .collect()
}

fn hand_selection_matches_requested(state: &BalatroState, requested: &[u32]) -> bool {
  let mut selected = selected_hand_slot_indices(state, &hand_slot_indices(state));
  let mut requested = requested.to_vec();
  selected.sort_unstable();
  requested.sort_unstable();
  selected == requested
}

fn selected_slots_to_clear(state: &BalatroState, requested: &[u32]) -> Vec<u32> {
  selected_hand_slot_indices(state, &hand_slot_indices(state)).into_iter().filter(|slot_index| !requested.contains(slot_index)).collect()
}

fn requested_slots_to_select(state: &BalatroState, requested: &[u32]) -> Vec<u32> {
  requested.iter().copied().filter(|slot_index| !hand_slot_is_selected(state, *slot_index)).collect()
}

fn hand_slot_is_selected(state: &BalatroState, slot_index: u32) -> bool {
  let Some(baseline_y) = hand_selection_baseline_y(state) else {
    return false;
  };
  let Some(card) = state.hand.get(slot_index as usize) else {
    return false;
  };
  // Balatro indicates selected hand cards by raising them before the play or
  // discard button is pressed. Use the current hand's lower row as the baseline
  // so stale selections from an earlier failed command are visible and can be
  // cleared before a new play/discard operation.
  card.bbox.y1 <= baseline_y - 24.0
}

fn hand_selection_baseline_y(state: &BalatroState) -> Option<f32> {
  state.hand.iter().map(|card| card.bbox.y1).max_by(|left, right| left.total_cmp(right))
}

fn hand_slot_indices(state: &BalatroState) -> Vec<u32> {
  (0..state.hand.len() as u32).collect()
}

fn select_cards_for_read<'a>(state: &'a BalatroState, requested: &Option<Vec<u32>>) -> Result<Vec<&'a CardSlot>, CliError> {
  match requested {
    Some(indices) => select_hand_cards(state, indices),
    None => Ok(state.hand.iter().collect()),
  }
}

fn select_store_item(state: &BalatroState, index: u32) -> Result<&StoreItem, CliError> {
  state.store.items.get(index as usize).ok_or_else(|| CliError::Message(format!("could not find store:{index}")))
}

fn select_joker(state: &BalatroState, index: u32) -> Result<&JokerSlot, CliError> {
  state.jokers.get(index as usize).ok_or_else(|| CliError::Message(format!("could not find joker:{index}")))
}

fn select_consumable(state: &BalatroState, index: u32) -> Result<&ConsumableSlot, CliError> {
  state.consumables.get(index as usize).ok_or_else(|| CliError::Message(format!("could not find consumable:{index}")))
}

fn object_read_from_state(state: &BalatroState, slot: &str, zone: ObjectReadZone) -> Result<ObjectReadResult, CliError> {
  let (slot, kind, bbox, confidence) = match zone {
    ObjectReadZone::Store => {
      let index = parse_store_slot_index(slot)?;
      let item = select_store_item(state, index)?;
      (item.slot, object_kind_label(&item.kind)?, item.bbox, item.confidence)
    }
    ObjectReadZone::Joker => {
      let index = parse_joker_slot_index(slot)?;
      let joker = select_joker(state, index)?;
      (joker.slot, "joker".to_string(), joker.bbox, joker.confidence)
    }
    ObjectReadZone::Consumable => {
      let index = parse_consumable_slot_index(slot)?;
      let consumable = select_consumable(state, index)?;
      (consumable.slot, object_kind_label(&consumable.kind)?, consumable.bbox, consumable.confidence)
    }
  };

  Ok(ObjectReadResult {
    slot,
    kind,
    bbox,
    confidence,
    reading: ObjectReadValue::unread(),
    evidence: ObjectReadEvidence {
      frame: state.frame.source.clone(),
      source: "observation_without_hover_ocr".to_string(),
      hover_required: true,
      hover_frame: None,
      hover_ocr_region: None,
      hover_error: None,
    },
  })
}

fn object_kind_label<T>(kind: &T) -> Result<String, CliError>
where
  T: Serialize,
{
  serde_json::to_value(kind)?
    .as_str()
    .map(str::to_string)
    .ok_or_else(|| CliError::Message("object kind must serialize as a string".to_string()))
}

fn active_pack_choices(state: &BalatroState) -> Vec<PackChoice> {
  if best_button(&state.buttons, "button_card_pack_skip").is_none() {
    return Vec::new();
  }

  let height = state.frame.image_size.height.max(1) as f32;
  let width = state.frame.image_size.width.max(1) as f32;
  let mut choices = state
    .raw_entities
    .iter()
    .filter_map(|evidence| {
      let detection = &evidence.detection;
      let center_x = (detection.bbox.x1 + detection.bbox.x2) / 2.0;
      let center_y = (detection.bbox.y1 + detection.bbox.y2) / 2.0;
      let in_choice_area = center_x > width * 0.28 && center_x < width * 0.78 && center_y > height * 0.55 && center_y < height * 0.86;
      let is_choice = matches!(detection.label.as_str(), "joker_card" | "tarot_card" | "planet_card" | "spectral_card" | "poker_card_front")
        && in_choice_area;
      is_choice.then(|| PackChoice {
        id: PackChoiceId::new(0),
        detector_label: detection.label.clone(),
        bbox: detection.bbox,
        confidence: detection.confidence,
      })
    })
    .collect::<Vec<_>>();
  choices.sort_by(|left, right| left.bbox.x1.partial_cmp(&right.bbox.x1).unwrap_or(std::cmp::Ordering::Equal));
  for (index, choice) in choices.iter_mut().enumerate() {
    choice.id = PackChoiceId::new(index as u32);
  }
  choices
}

fn pack_read_choice(choice: PackChoice) -> PackReadChoice {
  PackReadChoice {
    hint: pack_choice_hint(&choice.detector_label).to_string(),
    choice,
    hover_required: true,
    hover_text: None,
    hover_frame: None,
    hover_ocr_region: None,
    hover_error: None,
  }
}

fn pack_choice_hint(label: &str) -> &'static str {
  match label {
    "poker_card_front" => {
      "active pack choice; detector label may be ambiguous in Standard/Buffoon packs, use hover OCR before strategic choice"
    }
    "joker_card" => "active joker pack choice; use hover OCR to read joker name/effect",
    "tarot_card" => "active tarot pack choice; use hover OCR to read tarot name/effect",
    "planet_card" => "active planet pack choice; use hover OCR to read planet name/hand upgrade",
    "spectral_card" => "active spectral pack choice; use hover OCR to read spectral name/effect",
    _ => "active pack choice; use hover OCR before strategic choice",
  }
}

fn pack_choice_hover_ocr_region() -> RatioRect {
  RatioRect::new(0.20, 0.02, 0.70, 0.72)
}

fn object_hover_ocr_region() -> RatioRect {
  RatioRect::new(0.16, 0.02, 0.72, 0.78)
}

fn pack_ocr_words() -> Vec<&'static str> {
  vec![
    "Joker",
    "Tarot",
    "Planet",
    "Spectral",
    "The Fool",
    "The Magician",
    "The High Priestess",
    "The Empress",
    "The Emperor",
    "The Hierophant",
    "The Lovers",
    "The Chariot",
    "Justice",
    "The Hermit",
    "The Wheel of Fortune",
    "Strength",
    "The Hanged Man",
    "Death",
    "Temperance",
    "The Devil",
    "The Tower",
    "The Star",
    "The Moon",
    "The Sun",
    "Judgement",
    "The World",
  ]
}

fn object_ocr_words() -> Vec<&'static str> {
  let mut words = pack_ocr_words();
  words.extend([
    "Joker",
    "Common",
    "Uncommon",
    "Rare",
    "Negative",
    "Foil",
    "Holographic",
    "Polychrome",
    "Mult",
    "Chips",
    "倍率",
    "筹码",
    "小丑牌",
    "塔罗牌",
    "星球牌",
    "优惠券",
    "普通",
    "罕见",
    "稀有",
  ]);
  words
}

fn non_empty_trimmed_text(text: &str) -> Option<String> {
  let text = text.trim();
  (!text.is_empty()).then(|| text.to_string())
}

fn select_pack_choice(choices: &[PackChoice], index: u32) -> Result<&PackChoice, CliError> {
  choices.get(index as usize).ok_or_else(|| CliError::Message(format!("could not find pack:{index}")))
}

fn ocr_region_for_card(state: &BalatroState, card: &CardSlot) -> RatioRect {
  let width = f64::from(state.frame.image_size.width).max(1.0);
  let height = f64::from(state.frame.image_size.height).max(1.0);
  let card_w = f64::from(card.bbox.width().max(1.0));
  let card_h = f64::from(card.bbox.height().max(1.0));
  RatioRect::new(f64::from(card.bbox.x1) / width, f64::from(card.bbox.y1) / height, (card_w * 0.38) / width, (card_h * 0.46) / height)
}

#[cfg(target_os = "macos")]
fn card_corner_capture(capture: &Capture, state: &BalatroState, card: &CardSlot) -> Capture {
  let (x, y, width, height) = card_corner_pixels(capture, state, card);
  let crop = image::imageops::crop_imm(&capture.image, x, y, width, height).to_image();
  let scale = 6;
  let resized = image::imageops::resize(&crop, width * scale, height * scale, image::imageops::FilterType::Nearest);
  Capture {
    image: resized,
    bounds: Rect::new(0.0, 0.0, f64::from(width * scale), f64::from(height * scale)),
    scale_factor: capture.scale_factor,
    backend: format!("{}:card-corner", capture.backend),
    fallback_reason: capture.fallback_reason.clone(),
  }
}

#[cfg(target_os = "macos")]
fn infer_suit_from_card_corner(capture: &Capture, state: &BalatroState, card: &CardSlot) -> Option<&'static str> {
  let (x, y, width, height) = card_corner_pixels(capture, state, card);
  let suit_x_end = x + ((width as f32 * 0.58).ceil() as u32).max(1);
  let suit_y_start = y + (height as f32 * 0.28).floor() as u32;
  let mut hearts = 0u32;
  let mut diamonds = 0u32;
  let mut clubs = 0u32;
  let mut spades = 0u32;
  // The full corner of face cards includes colored portrait pixels. Restrict
  // classification to the narrow glyph band directly below the rank so a
  // queen's artwork cannot outvote its heart/diamond suit symbol.
  for py in suit_y_start..(y + height) {
    for px in x..suit_x_end {
      let [r, g, b, a] = capture.image.get_pixel(px, py).0;
      let r16 = i16::from(r);
      let g16 = i16::from(g);
      let b16 = i16::from(b);
      if a < 120 {
        continue;
      }
      if r > 170 && g < 95 && b < 95 {
        hearts += 1;
      } else if r > 170 && g > 110 && b < 100 {
        diamonds += 1;
      } else if b > 135 && g > 90 && r < 130 {
        clubs += 1;
      } else if g > 55 && g16 > r16 + 18 && g16 > b16 + 8 && r < 110 && b < 115 {
        spades += 1;
      }
    }
  }
  [
    ("hearts", hearts),
    ("diamonds", diamonds),
    ("clubs", clubs),
    ("spades", spades),
  ]
  .into_iter()
  .max_by_key(|(_, count)| *count)
  .and_then(|(suit, count)| if count >= 8 { Some(suit) } else { None })
}

#[cfg(target_os = "macos")]
fn card_corner_pixels(capture: &Capture, state: &BalatroState, card: &CardSlot) -> (u32, u32, u32, u32) {
  let image_w = capture.image.width().max(1);
  let image_h = capture.image.height().max(1);
  let scale_x = image_w as f32 / state.frame.image_size.width.max(1) as f32;
  let scale_y = image_h as f32 / state.frame.image_size.height.max(1) as f32;
  let x = (card.bbox.x1.max(0.0) * scale_x).floor() as u32;
  let y = (card.bbox.y1.max(0.0) * scale_y).floor() as u32;
  let width = (card.bbox.width().max(1.0) * 0.38 * scale_x).ceil() as u32;
  let height = (card.bbox.height().max(1.0) * 0.46 * scale_y).ceil() as u32;
  let x = x.min(image_w.saturating_sub(1));
  let y = y.min(image_h.saturating_sub(1));
  let width = width.min(image_w - x).max(1);
  let height = height.min(image_h - y).max(1);
  (x, y, width, height)
}

#[cfg(target_os = "macos")]
#[derive(Clone, Debug)]
struct RankTemplate {
  rank: &'static str,
  suit: &'static str,
  mask: NormalizedMask,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Debug)]
struct NormalizedMask {
  pixels: Vec<bool>,
}

#[cfg(target_os = "macos")]
fn load_deck_rank_templates() -> Option<Vec<RankTemplate>> {
  let atlas = load_deck_atlas()?;
  let ranks = [
    "A", "2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K",
  ];
  let suits = ["hearts", "clubs", "diamonds", "spades"];
  let cell_w = atlas.width() / ranks.len() as u32;
  let cell_h = atlas.height() / suits.len() as u32;
  let mut templates = Vec::new();
  for (suit_index, suit) in suits.into_iter().enumerate() {
    for (rank_index, rank) in ranks.into_iter().enumerate() {
      let x = rank_index as u32 * cell_w;
      let y = suit_index as u32 * cell_h;
      let width = (cell_w as f32 * 0.26).ceil() as u32;
      let height = (cell_h as f32 * 0.36).ceil() as u32;
      let crop = image::imageops::crop_imm(&atlas, x, y, width, height).to_image();
      if let Some(mask) = top_glyph_mask(&crop) {
        templates.push(RankTemplate { rank, suit, mask });
      }
    }
  }
  (!templates.is_empty()).then_some(templates)
}

#[cfg(target_os = "macos")]
fn load_deck_atlas() -> Option<RgbaImage> {
  if let Ok(cache_dir) = setup_cache_dir(None) {
    if let Some(image) = load_deck_atlas_from_setup_cache(&cache_dir) {
      return Some(image);
    }
  }

  None
}

#[cfg(target_os = "macos")]
fn load_deck_atlas_from_setup_cache(cache_dir: &Path) -> Option<RgbaImage> {
  let deck_atlas_path = cache_dir.join(DECK_ATLAS_CACHE_FILE);
  if !deck_atlas_path.exists() {
    return None;
  }
  image::open(deck_atlas_path).ok().map(|image| image.to_rgba8())
}

#[cfg(target_os = "macos")]
fn best_rank_candidate_from_deck_template(
  corner: &RgbaImage,
  templates: Option<&[RankTemplate]>,
  suit: Option<&str>,
) -> Option<(String, f32)> {
  let x = (corner.width() as f32 * 0.12).floor() as u32;
  let y = (corner.height() as f32 * 0.12).floor() as u32;
  let width = (corner.width() as f32 * 0.48).ceil() as u32;
  let height = (corner.height() as f32 * 0.50).ceil() as u32;
  let observed_region =
    image::imageops::crop_imm(corner, x, y, width.min(corner.width() - x).max(1), height.min(corner.height() - y).max(1)).to_image();
  let observed = top_glyph_mask(&observed_region)?;
  templates?
    .iter()
    .filter(|template| suit.is_none_or(|suit| template.suit == suit))
    .map(|template| {
      let similarity = mask_similarity(&observed, &template.mask);
      (template.rank, similarity)
    })
    .max_by(|left, right| left.1.partial_cmp(&right.1).unwrap_or(std::cmp::Ordering::Equal))
    .map(|(rank, confidence)| (rank.to_string(), confidence.clamp(0.0, 1.0)))
}

#[cfg(target_os = "macos")]
fn top_glyph_mask(image: &RgbaImage) -> Option<NormalizedMask> {
  let active_rows = (0..image.height())
    .map(|y| (0..image.width()).filter(|x| is_card_glyph_pixel(image.get_pixel(*x, y).0)).count() >= 2)
    .collect::<Vec<_>>();
  let start = active_rows.iter().position(|active| *active)? as u32;
  let mut end = image.height();
  let mut blank_run = 0u32;
  for y in start..image.height() {
    if active_rows[y as usize] {
      blank_run = 0;
    } else {
      blank_run += 1;
      if blank_run >= 3 {
        end = y + 1 - blank_run;
        break;
      }
    }
  }
  if end <= start {
    return None;
  }
  let crop = image::imageops::crop_imm(image, 0, start, image.width(), end - start).to_image();
  normalized_foreground_mask(&crop)
}

#[cfg(target_os = "macos")]
fn normalized_foreground_mask(image: &RgbaImage) -> Option<NormalizedMask> {
  const MASK_W: usize = 24;
  const MASK_H: usize = 32;

  let mut min_x = image.width();
  let mut min_y = image.height();
  let mut max_x = 0;
  let mut max_y = 0;
  for y in 0..image.height() {
    for x in 0..image.width() {
      if is_card_glyph_pixel(image.get_pixel(x, y).0) {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
      }
    }
  }
  if min_x > max_x || min_y > max_y {
    return None;
  }

  let width = (max_x - min_x + 1).max(1);
  let height = (max_y - min_y + 1).max(1);
  let mut pixels = vec![false; MASK_W * MASK_H];
  for ty in 0..MASK_H {
    for tx in 0..MASK_W {
      let sx = min_x + ((tx as f32 + 0.5) / MASK_W as f32 * width as f32) as u32;
      let sy = min_y + ((ty as f32 + 0.5) / MASK_H as f32 * height as f32) as u32;
      pixels[ty * MASK_W + tx] = is_card_glyph_pixel(image.get_pixel(sx.min(image.width() - 1), sy.min(image.height() - 1)).0);
    }
  }
  Some(NormalizedMask { pixels })
}

#[cfg(target_os = "macos")]
fn is_card_glyph_pixel([r, g, b, a]: [u8; 4]) -> bool {
  if a < 80 {
    return false;
  }
  let max = r.max(g).max(b);
  let min = r.min(g).min(b);
  max > 35 && max - min > 18 && !(r > 210 && g > 210 && b > 210)
}

#[cfg(target_os = "macos")]
fn mask_similarity(left: &NormalizedMask, right: &NormalizedMask) -> f32 {
  let intersection = left.pixels.iter().zip(&right.pixels).filter(|(left, right)| **left && **right).count();
  let foreground = left.pixels.iter().filter(|pixel| **pixel).count() + right.pixels.iter().filter(|pixel| **pixel).count();
  if foreground == 0 {
    return 0.0;
  }
  2.0 * intersection as f32 / foreground as f32
}

fn parse_card_reading(raw_text: &str, suit: Option<&str>, confidence: Option<f32>) -> CardReadValue {
  let normalized = normalize_card_text(raw_text);
  let rank = extract_rank(&normalized);
  let suit = suit.map(str::to_string).or_else(|| detect_suit(&normalized));
  let suit_symbol = suit.as_deref().and_then(suit_symbol).map(str::to_string);
  let short_code = rank.as_ref().zip(suit.as_deref()).map(|(rank, suit)| {
    format!(
      "{rank}{}",
      match suit {
        "hearts" => "H",
        "diamonds" => "D",
        "clubs" => "C",
        "spades" => "S",
        _ => "?",
      }
    )
  });
  let valid = rank.is_some() && suit.is_some();
  CardReadValue {
    status: if valid { "read" } else { "partial" },
    raw_text: (!raw_text.trim().is_empty()).then(|| raw_text.to_string()),
    normalized_text: (!normalized.is_empty()).then_some(normalized),
    rank,
    rank_candidate: None,
    rank_candidate_confidence: None,
    suit,
    suit_symbol,
    short_code,
    confidence,
    valid,
  }
}

fn should_hover_reread_card(reading: &CardReadValue) -> bool {
  !reading.valid || reading.confidence.is_some_and(|confidence| confidence < 0.85)
}

fn normalize_card_text(text: &str) -> String {
  let mut normalized = text
    .trim()
    .replace('：', ":")
    .replace('，', ",")
    .replace("红挑", "红桃")
    .replace("黑挑", "黑桃")
    .replace("方申", "方片")
    .replace("梅华", "梅花");
  normalized.retain(|ch| !ch.is_whitespace());
  normalized.to_uppercase()
}

fn extract_rank(text: &str) -> Option<String> {
  for rank in [
    "10", "A", "K", "Q", "J", "T", "9", "8", "7", "6", "5", "4", "3", "2",
  ] {
    if text.contains(rank) {
      return Some(if rank == "T" { "10" } else { rank }.to_string());
    }
  }
  None
}

fn detect_suit(text: &str) -> Option<String> {
  [
    ("diamonds", ["方片", "方块", "DIAMOND", "♦"].as_slice()),
    ("hearts", ["红桃", "红心", "HEART", "♥"].as_slice()),
    ("spades", ["黑桃", "SPADE", "♠"].as_slice()),
    ("clubs", ["梅花", "CLUB", "♣"].as_slice()),
  ]
  .into_iter()
  .find_map(|(suit, patterns)| patterns.iter().any(|pattern| text.contains(pattern)).then(|| suit.to_string()))
}

fn suit_symbol(suit: &str) -> Option<&'static str> {
  match suit {
    "hearts" => Some("♥"),
    "diamonds" => Some("♦"),
    "clubs" => Some("♣"),
    "spades" => Some("♠"),
    _ => None,
  }
}

fn card_ocr_words() -> [&'static str; 21] {
  [
    "A", "K", "Q", "J", "10", "9", "8", "7", "6", "5", "4", "3", "2", "红桃", "方片", "方块", "黑桃", "梅花", "Hearts", "Diamonds", "Spades",
  ]
}

fn window_point_from_button(state: &BalatroState, window: &auv_driver::window::Window, button: &ButtonTarget) -> Point {
  window_point_from_frame_point(state, window, bbox_center_point(button.bbox))
}

fn window_point_from_store_item(state: &BalatroState, window: &auv_driver::window::Window, item: &StoreItem) -> Point {
  window_point_from_frame_point(state, window, bbox_center_point(item.bbox))
}

fn window_point_from_joker(state: &BalatroState, window: &auv_driver::window::Window, joker: &JokerSlot) -> Point {
  window_point_from_frame_point(state, window, bbox_center_point(joker.bbox))
}

fn window_point_from_consumable(state: &BalatroState, window: &auv_driver::window::Window, consumable: &ConsumableSlot) -> Point {
  window_point_from_frame_point(state, window, bbox_center_point(consumable.bbox))
}

fn bbox_center_point(bbox: BoundingBox) -> Point {
  Point::new(f64::from((bbox.x1 + bbox.x2) / 2.0), f64::from((bbox.y1 + bbox.y2) / 2.0))
}

fn window_point_from_frame_point(state: &BalatroState, window: &auv_driver::window::Window, point: Point) -> Point {
  let width = f64::from(state.frame.image_size.width).max(1.0);
  let height = f64::from(state.frame.image_size.height).max(1.0);
  Point::new(point.x / width * window.frame.size.width, point.y / height * window.frame.size.height)
}

fn normalized_window_point(window: &auv_driver::window::Window, x: f64, y: f64) -> Point {
  Point::new(x * window.frame.size.width, y * window.frame.size.height)
}

fn window_point_from_hand_card(state: &BalatroState, window: &auv_driver::window::Window, card: &CardSlot) -> Point {
  window_point_from_frame_point(state, window, hand_card_click_frame_point(state, card))
}

fn hand_card_click_frame_point(state: &BalatroState, card: &CardSlot) -> Point {
  let width = (card.bbox.x2 - card.bbox.x1).max(1.0);
  let height = (card.bbox.y2 - card.bbox.y1).max(1.0);
  // Balatro hand cards overlap heavily. A raw bbox center or fixed ratio can
  // land inside a neighboring card's hit area. Estimate the visible horizontal
  // strip from adjacent hand-card boxes and click the middle of that strip.
  let index = card.slot.index as usize;
  let mut visible_left = card.bbox.x1;
  let mut visible_right = card.bbox.x2;
  if index > 0
    && let Some(previous) = state.hand.get(index - 1)
  {
    visible_left = visible_left.max(previous.bbox.x2.min(card.bbox.x2));
  }
  if let Some(next) = state.hand.get(index + 1) {
    visible_right = visible_right.min(next.bbox.x1.max(card.bbox.x1));
  }
  let x = if visible_right > visible_left + 8.0 {
    (visible_left + visible_right) / 2.0
  } else {
    card.bbox.x1 + width * 0.5
  };
  Point::new(f64::from(x), f64::from(card.bbox.y1 + height * 0.52))
}

#[cfg(test)]
#[path = "cli_test.rs"]
mod tests;
