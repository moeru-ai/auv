use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use auv_driver::capture::{Activation, Capture, CaptureOptions};
use auv_driver::geometry::{Point, RatioRect, Rect, WindowPoint};
use auv_driver::input::{ClickOptions, InputPolicy};
use auv_driver::selector::{App, Window};
use auv_driver::vision::TextRecognitionOptions;
use auv_inference_common::BoundingBox;
use auv_inference_ultralytics::InferenceDevice;
use clap::{Args, Parser, Subcommand, ValueEnum};
use image::{ImageError, RgbaImage};
use serde::Serialize;
use serde_json::Value;
use serde_json::json;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::config::BalatroModelConfig;
use crate::model::{
  BalatroPhase, BalatroState, ButtonTarget, CardSlot, ConsumableSlot, JokerSlot, RoundState,
  ScoreState, SlotId, StoreItem,
};
use crate::observation::{ObservationError, observe_image};
pub use crate::output::OutputMode;

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
  #[error(
    "observation command requires --image until live capture dispatch lands for this surface"
  )]
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

pub fn run_from_env() -> Result<(), CliError> {
  run(CliArgs::parse())
}

pub fn run(args: CliArgs) -> Result<(), CliError> {
  match args.command {
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
    }) => click_cards_commit("cards.play", "button_play", args),
    Command::Cards(CardsArgs {
      command: CardsCommand::Discard(args),
    }) => click_cards_commit("cards.discard", "button_discard", args),
    Command::Jokers(JokersArgs {
      command: JokersCommand::Ls(args),
    }) => write_observed_state(&args),
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
    println!(
      "Balatro setup {:?}: deck atlas {}",
      report.status,
      report.deck_atlas_path.display()
    );
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
  fs::write(
    &manifest_path,
    serde_json::to_string_pretty(&manifest)? + "\n",
  )?;

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
  Err(CliError::Message(format!(
    "could not resolve Balatro setup cache directory; pass --cache-dir"
  )))
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
  Err(CliError::Message(
    "could not find Balatro.love; pass --love <path> or --app <Balatro.app>".to_string(),
  ))
}

fn require_love_path(path: &Path) -> Result<PathBuf, CliError> {
  if path.exists() {
    Ok(path.to_path_buf())
  } else {
    Err(CliError::Message(format!(
      "Balatro.love does not exist: {}",
      path.display()
    )))
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
  let output = ProcessCommand::new("unzip")
    .arg("-p")
    .arg(love_path)
    .arg(DECK_ATLAS_LOVE_PATH)
    .output()?;
  if !output.status.success() {
    return Err(CliError::Message(format!(
      "failed to extract {DECK_ATLAS_LOVE_PATH} from {}",
      love_path.display()
    )));
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
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|duration| duration.as_millis())
    .unwrap_or_default()
}

fn click_store_reroll(_args: OperationControlArgs) -> Result<(), CliError> {
  deferred(
    "store.reroll",
    "store reroll input is implemented after store buy",
  )
}

#[cfg(target_os = "macos")]
fn click_joker_sell(args: SlotOperationArgs) -> Result<(), CliError> {
  let slot_index = parse_joker_slot_index(&args.slot)?;
  click_sell_object(ObjectReadZone::Joker, slot_index, args)
}

#[cfg(not(target_os = "macos"))]
fn click_joker_sell(args: SlotOperationArgs) -> Result<(), CliError> {
  parse_joker_slot_index(&args.slot)?;
  Err(CliError::Message(
    "jokers sell live operation is only available on macOS".to_string(),
  ))
}

#[cfg(target_os = "macos")]
fn click_consumable_sell(args: SlotOperationArgs) -> Result<(), CliError> {
  let slot_index = parse_consumable_slot_index(&args.slot)?;
  click_sell_object(ObjectReadZone::Consumable, slot_index, args)
}

#[cfg(not(target_os = "macos"))]
fn click_consumable_sell(args: SlotOperationArgs) -> Result<(), CliError> {
  parse_consumable_slot_index(&args.slot)?;
  Err(CliError::Message(
    "consumables sell live operation is only available on macOS".to_string(),
  ))
}

#[cfg(target_os = "macos")]
fn click_sell_object(
  zone: ObjectReadZone,
  slot_index: u32,
  args: SlotOperationArgs,
) -> Result<(), CliError> {
  use auv_driver::Driver;

  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local()?;
  let window = session
    .window()
    .resolve(Window::main_visible().owned_by(App::name(args.control.target.clone())))?;
  let operation = match zone {
    ObjectReadZone::Joker => "jokers.sell",
    ObjectReadZone::Consumable => "consumables.sell",
    ObjectReadZone::Store => {
      return Err(CliError::Message(
        "store items are not sellable through object sell".to_string(),
      ));
    }
  };
  let before_image = capture_window_to_temp(&session, &window, "object-sell-before")?;
  let before = observe_image(&before_image, &BalatroModelConfig::default(), true)?;
  let object_point = match zone {
    ObjectReadZone::Joker => {
      let joker = select_joker(&before, slot_index)?;
      window_point_from_joker(&before, &window, joker)
    }
    ObjectReadZone::Consumable => {
      let consumable = select_consumable(&before, slot_index)?;
      window_point_from_consumable(&before, &window, consumable)
    }
    ObjectReadZone::Store => unreachable!("store sell is rejected above"),
  };

  click_game_point(&session, &window, object_point)?;
  std::thread::sleep(Duration::from_millis(500));
  let selected_image = capture_window_to_temp(&session, &window, "object-sell-selected")?;
  let selected = observe_image(&selected_image, &BalatroModelConfig::default(), true)?;
  let sell_button = find_button(&selected, "button_sell")?;
  let sell_point = window_point_from_button(&selected, &window, sell_button);
  click_game_point(&session, &window, sell_point)?;

  let verification = if args.control.verify {
    let (after_image, after_result) = capture_observable_window(
      &session,
      &window,
      "object-sell-after",
      args.control.timeout_ms.unwrap_or(1000),
      500,
    )?;
    if args.control.verify_mode == VerifyModeArg::ActivationOnly {
      Some(json!({
        "mode": args.control.verify_mode.to_string(),
        "profile": "activation_only",
        "evidence": ["object_click_completed", "sell_click_completed"],
        "passed": true,
        "after_image": after_image,
      }))
    } else {
      match after_result {
        Ok(after) => Some(json!({
          "mode": args.control.verify_mode.to_string(),
          "profile": "weak",
          "evidence": sell_operation_evidence(zone, &before, &after),
          "before_joker_count": before.jokers.len(),
          "after_joker_count": after.jokers.len(),
          "before_consumable_count": before.consumables.len(),
          "after_consumable_count": after.consumables.len(),
          "before_cash": before.rounds.cash,
          "after_cash": after.rounds.cash,
          "passed": verify_sell_operation(zone, &before, &after),
          "after_image": after_image,
        })),
        Err(error) => Some(json!({
          "mode": args.control.verify_mode.to_string(),
          "profile": "weak",
          "evidence": Vec::<&str>::new(),
          "before_joker_count": before.jokers.len(),
          "before_consumable_count": before.consumables.len(),
          "before_cash": before.rounds.cash,
          "passed": false,
          "after_image": after_image,
          "error": error.to_string(),
        })),
      }
    }
  } else {
    None
  };

  write_operation_output(
    args.control.details,
    json!({
      "operation": operation,
      "target": args.control.target,
      "slot": args.slot,
      "object_point": object_point,
      "sell_button": sell_button,
      "sell_point": sell_point,
      "before_image": before_image,
      "selected_image": selected_image,
      "verification": verification,
    }),
  )
}

fn deferred(command: &'static str, reason: &'static str) -> Result<(), CliError> {
  Err(CliError::Deferred { command, reason })
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct CardReadResult {
  slot: crate::model::SlotId,
  bbox: auv_inference_common::BoundingBox,
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
  slot: crate::model::SlotId,
  kind: String,
  bbox: auv_inference_common::BoundingBox,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ActionTargetSource {
  YoloButton,
  // TODO(store-object-target-v1): object-origin targets are reserved for
  // store/pack item actions; Task 3 only resolves buttons and layout fallback.
  #[allow(dead_code)]
  YoloObject,
  LayoutFallback,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct ResolvedActionTarget {
  source: ActionTargetSource,
  label: String,
  frame_point: Point,
  fallback_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct PackChoice {
  slot_index: u32,
  kind: String,
  detector_label: String,
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
  bbox: auv_inference_common::BoundingBox,
  confidence: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct PackReadOutput {
  phase: BalatroPhase,
  choices: Vec<PackChoice>,
  skip_button: Option<ButtonTarget>,
  frame: crate::model::FrameRef,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct CardReadValue {
  status: &'static str,
  raw_text: Option<String>,
  normalized_text: Option<String>,
  rank: Option<String>,
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
  click_single_button("game.cash_out", "button_cash_out", args)
}

#[cfg(not(target_os = "macos"))]
fn click_game_cash_out(_args: OperationControlArgs) -> Result<(), CliError> {
  Err(CliError::Message(
    "game cash-out live operation is only available on macOS".to_string(),
  ))
}

#[cfg(target_os = "macos")]
fn click_game_restart(args: OperationControlArgs) -> Result<(), CliError> {
  use auv_driver::Driver;

  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local()?;
  let window = session
    .window()
    .resolve(Window::main_visible().owned_by(App::name(args.target.clone())))?;
  let before_image = capture_window_to_temp(&session, &window, "game-restart-before")?;
  let before = observe_image(&before_image, &BalatroModelConfig::default(), true).ok();
  let first_point = before
    .as_ref()
    .and_then(|state| {
      restart_primary_button(&state.buttons).map(|button| {
        (
          button.id.as_str(),
          window_point_from_button(state, &window, button),
        )
      })
    })
    .unwrap_or_else(|| {
      if before.is_none() {
        // NOTICE: Game Over overlays and some localized title screens are not
        // covered by the current Balatro YOLO UI dataset, so restart may begin
        // from a no-detection frame. Prefer the Game Over "start new run" slot
        // first because it is the common post-run recovery path; the localized
        // title-screen fallback below is still tried if this does not reveal an
        // observable new-run button.
        return (
          "game_over_start_new_run_layout",
          normalized_window_point(&window, 0.62, 0.805),
        );
      }
      // TODO(balatro-game-over-ui-v1): replace this Game Over layout fallback
      // with YOLO button evidence once game-over overlay buttons are in the UI
      // dataset.
      (
        "game_over_start_new_run_layout",
        normalized_window_point(&window, 0.62, 0.815),
      )
    });

  click_game_point(&session, &window, first_point.1)?;
  if first_point.0 == "game_over_start_new_run_layout" {
    std::thread::sleep(Duration::from_millis(300));
    click_game_point(&session, &window, first_point.1)?;
  }

  std::thread::sleep(Duration::from_millis(900));
  let intermediate_image = capture_window_to_temp(&session, &window, "game-restart-intermediate")?;
  let mut second_point = None;
  if let Ok(intermediate) = observe_image(&intermediate_image, &BalatroModelConfig::default(), true)
  {
    if let Some(button) = restart_primary_button(&intermediate.buttons) {
      let point = window_point_from_button(&intermediate, &window, button);
      click_game_point(&session, &window, point)?;
      second_point = Some(point);
    }
  } else if first_point.0 == "game_over_start_new_run_layout" {
    // NOTICE: If the first no-detection click did not expose the new-run
    // screen, treat the frame as the older localized title-screen fallback.
    // This keeps restart usable until both layouts have detector-backed button
    // classes.
    let point = normalized_window_point(&window, 0.31, 0.84);
    click_game_point(&session, &window, point)?;
    second_point = Some(point);
  }

  let verification = if args.verify {
    let (mut after_image, mut after_result) = capture_observable_window(
      &session,
      &window,
      "game-restart-after",
      args.timeout_ms.unwrap_or(1800),
      700,
    )?;
    let mut verification_retry_click_point = None;
    if let Ok(after) = &after_result {
      if after.phase == BalatroPhase::MainMenu {
        if let Some(button) = restart_primary_button(&after.buttons) {
          let point = window_point_from_button(after, &window, button);
          click_game_point(&session, &window, point)?;
          verification_retry_click_point = Some(point);
          (after_image, after_result) = capture_observable_window(
            &session,
            &window,
            "game-restart-after-retry",
            args.timeout_ms.unwrap_or(1800),
            700,
          )?;
        }
      }
    }
    match after_result {
      Ok(after) => Some(json!({
        "mode": args.verify_mode.to_string(),
        "after_phase": after.phase,
        "verification_retry_click_point": verification_retry_click_point,
        "passed": matches!(after.phase, BalatroPhase::BlindSelect | BalatroPhase::Playing | BalatroPhase::Store),
        "after_image": after_image,
      })),
      Err(error) => Some(json!({
        "mode": args.verify_mode.to_string(),
        "verification_retry_click_point": verification_retry_click_point,
        "passed": false,
        "after_image": after_image,
        "error": error.to_string(),
      })),
    }
  } else {
    None
  };

  write_operation_output(
    args.details,
    json!({
      "operation": "game.restart",
      "target": args.target,
      "strategy": first_point.0,
      "window_point": first_point.1,
      "intermediate_image": intermediate_image,
      "second_click_point": second_point,
      "before_image": before_image,
      "verification": verification,
    }),
  )
}

#[cfg(not(target_os = "macos"))]
fn click_game_restart(_args: OperationControlArgs) -> Result<(), CliError> {
  Err(CliError::Message(
    "game restart live operation is only available on macOS".to_string(),
  ))
}

#[cfg(target_os = "macos")]
fn click_store_buy(args: SlotOperationArgs) -> Result<(), CliError> {
  use auv_driver::Driver;

  let slot_index = parse_store_slot_index(&args.slot)?;
  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local()?;
  let window = session
    .window()
    .resolve(Window::main_visible().owned_by(App::name(args.control.target.clone())))?;
  let before_image = capture_window_to_temp(&session, &window, "store-buy-before")?;
  let before = observe_image(&before_image, &BalatroModelConfig::default(), true)?;
  let item = select_store_item(&before, slot_index)?;
  let item_point = window_point_from_store_item(&before, &window, item);

  click_game_point(&session, &window, item_point)?;
  std::thread::sleep(Duration::from_millis(500));
  let selected_image = capture_window_to_temp(&session, &window, "store-buy-selected")?;
  let selected = observe_image(&selected_image, &BalatroModelConfig::default(), true)?;
  let confirm_button = select_store_buy_confirm_button(&selected)?;
  let confirm_point = window_point_from_button(&selected, &window, confirm_button);
  click_game_point(&session, &window, confirm_point)?;

  let verification = if args.control.verify {
    let (after_image, after_result) = capture_observable_window(
      &session,
      &window,
      "store-buy-after",
      args.control.timeout_ms.unwrap_or(1000),
      500,
    )?;
    match after_result {
      Ok(after) => Some(json!({
        "mode": args.control.verify_mode.to_string(),
        "before_phase": before.phase,
        "after_phase": after.phase,
        "before_store_item_count": before.store.items.len(),
        "after_store_item_count": after.store.items.len(),
        "before_joker_count": before.jokers.len(),
        "after_joker_count": after.jokers.len(),
        "before_consumable_count": before.consumables.len(),
        "after_consumable_count": after.consumables.len(),
        "evidence": store_buy_evidence(&before, &after),
        "passed": verify_store_buy(&before, &after),
        "after_image": after_image,
      })),
      Err(error) => Some(json!({
        "mode": args.control.verify_mode.to_string(),
        "before_phase": before.phase,
        "before_store_item_count": before.store.items.len(),
        "before_joker_count": before.jokers.len(),
        "before_consumable_count": before.consumables.len(),
        "evidence": Vec::<&str>::new(),
        "passed": false,
        "after_image": after_image,
        "error": error.to_string(),
      })),
    }
  } else {
    None
  };

  write_operation_output(
    args.control.details,
    json!({
      "operation": "store.buy",
      "target": args.control.target,
      "slot": args.slot,
      "store_item": item,
      "item_point": item_point,
      "before_image": before_image,
      "confirm_button": confirm_button,
      "confirm_point": confirm_point,
      "selected_image": selected_image,
      "verification": verification,
    }),
  )
}

#[cfg(not(target_os = "macos"))]
fn click_store_buy(args: SlotOperationArgs) -> Result<(), CliError> {
  parse_store_slot_index(&args.slot)?;
  Err(CliError::Message(
    "store buy live operation is only available on macOS".to_string(),
  ))
}

fn observe_from_args(args: &ObserveArgs) -> Result<BalatroState, CliError> {
  let config = BalatroModelConfig::from_observe_args(args);
  if let Some(image) = args.image.as_deref() {
    return observe_image_with_ui_readings(image, &config, args.no_cache);
  }
  observe_live_target(&args.target, &config, args.no_cache)
}

fn observe_image_with_ui_readings(
  image: &Path,
  config: &BalatroModelConfig,
  no_cache: bool,
) -> Result<BalatroState, CliError> {
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
  read_cards_live(
    &args.observe.target,
    &config,
    args.observe.no_cache,
    &requested,
    args.frame_out.as_deref(),
  )
}

#[cfg(target_os = "macos")]
fn read_object_live(
  args: &SlotObserveArgs,
  zone: ObjectReadZone,
) -> Result<ObjectReadResult, CliError> {
  use auv_driver::Driver;

  let config = BalatroModelConfig::from_observe_args(&args.observe);
  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local()?;
  let window = session
    .window()
    .resolve(Window::main_visible().owned_by(App::name(args.observe.target.clone())))?;
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
fn read_object_live(
  args: &SlotObserveArgs,
  zone: ObjectReadZone,
) -> Result<ObjectReadResult, CliError> {
  let state = observe_from_args(&args.observe)?;
  object_read_from_state(&state, &args.slot, zone)
}

#[cfg(target_os = "macos")]
fn read_cards_from_image(
  image: &Path,
  config: &BalatroModelConfig,
  no_cache: bool,
  requested: &Option<Vec<u32>>,
) -> Result<Vec<CardReadResult>, CliError> {
  use auv_driver::Driver;

  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local()?;
  let capture = capture_from_image(image)?;
  let state = observe_image_with_ui_readings(image, config, no_cache)?;
  let cards = select_cards_for_read(&state, requested)?;
  let rank_templates = load_deck_rank_templates();
  cards
    .into_iter()
    .map(|card| {
      read_card_from_capture(
        &session,
        &capture,
        image,
        &state,
        card,
        rank_templates.as_deref(),
      )
    })
    .collect()
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

fn write_operation_output(details: bool, mut payload: Value) -> Result<(), CliError> {
  if !details {
    strip_operation_details(&mut payload);
  }
  write_output(OutputMode::Json, &payload)
}

fn strip_operation_details(value: &mut Value) {
  match value {
    Value::Object(map) => {
      let generated_detail_keys = map
        .keys()
        .filter(|key| {
          key.ends_with("_image")
            || key.ends_with("_images")
            || key.ends_with("_point")
            || key.ends_with("_points")
        })
        .cloned()
        .collect::<Vec<_>>();
      for key in generated_detail_keys {
        map.remove(&key);
      }

      for key in [
        "after_image",
        "after_store",
        "before_image",
        "before_interactions",
        "bbox",
        "button",
        "button_point",
        "buttons",
        "card_points",
        "choice",
        "choice_point",
        "click_targets",
        "commit_button",
        "confirm_button",
        "confirm_point",
        "confirm_target",
        "consumable",
        "consumable_point",
        "hand",
        "item_point",
        "raw_entities",
        "raw_ui",
        "selected_button",
        "selected_cards",
        "selected_image",
        "selected_interactions",
        "selected_target",
        "selection_evidence",
        "store_item",
        "use_button",
        "use_point",
        "window_point",
      ] {
        map.remove(key);
      }
      for value in map.values_mut() {
        strip_operation_details(value);
      }
    }
    Value::Array(values) => {
      for value in values {
        strip_operation_details(value);
      }
    }
    _ => {}
  }
}

#[cfg(target_os = "macos")]
fn click_consumable_use(args: TargetSlotOperationArgs) -> Result<(), CliError> {
  use auv_driver::Driver;

  let slot_index = parse_consumable_slot_index(&args.slot)?;
  let target_indices = parse_hand_target_indices(&args.targets)?;
  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local()?;
  let window = session
    .window()
    .resolve(Window::main_visible().owned_by(App::name(args.control.target.clone())))?;
  let before_image = capture_window_to_temp(&session, &window, "consumable-use-before")?;
  let before = observe_image(&before_image, &BalatroModelConfig::default(), true)?;
  let target_selection = if target_indices.is_empty() {
    None
  } else {
    Some(click_hand_targets(
      &session,
      &window,
      "consumable-use-targets",
      &before,
      &target_indices,
      args.control.timeout_ms,
    )?)
  };
  let consumable = select_consumable(&before, slot_index)?;
  let consumable_point = window_point_from_consumable(&before, &window, consumable);

  click_game_point(&session, &window, consumable_point)?;
  std::thread::sleep(Duration::from_millis(500));
  let selected_image = capture_window_to_temp(&session, &window, "consumable-use-selected")?;
  let selected = observe_image(&selected_image, &BalatroModelConfig::default(), true)?;
  let use_target = resolve_consumable_use_target(&selected, slot_index)?;
  let use_point = window_point_from_frame_point(&selected, &window, use_target.frame_point);
  click_game_point(&session, &window, use_point)?;

  let verification = if args.control.verify {
    std::thread::sleep(Duration::from_millis(
      args.control.timeout_ms.unwrap_or(1200),
    ));
    let after_image = capture_window_to_temp(&session, &window, "consumable-use-after")?;
    if args.control.verify_mode == VerifyModeArg::ActivationOnly {
      Some(json!({
        "mode": args.control.verify_mode.to_string(),
        "profile": "activation_only",
        "evidence": ["consumable_click_completed", "use_click_completed"],
        "passed": true,
        "after_image": after_image,
      }))
    } else {
      match observe_image(&after_image, &BalatroModelConfig::default(), true) {
        Ok(after) => Some(json!({
          "mode": args.control.verify_mode.to_string(),
          "profile": "weak",
          "evidence": consumable_use_evidence(&before, &after),
          "before_consumable_count": before.consumables.len(),
          "after_consumable_count": after.consumables.len(),
          "before_phase": before.phase,
          "after_phase": after.phase,
          "passed": verify_consumable_use(&before, &after),
          "after_image": after_image,
        })),
        Err(error) => Some(json!({
          "mode": args.control.verify_mode.to_string(),
          "profile": "weak",
          "evidence": Vec::<&str>::new(),
          "before_consumable_count": before.consumables.len(),
          "before_phase": before.phase,
          "passed": false,
          "after_image": after_image,
          "error": error.to_string(),
        })),
      }
    }
  } else {
    None
  };

  write_operation_output(
    args.control.details,
    json!({
      "operation": "consumables.use",
      "target": args.control.target,
      "slot": args.slot,
      "targets": args.targets,
      "consumable": consumable,
      "consumable_point": consumable_point,
      "before_image": before_image,
      "target_selection": target_selection,
      "use_target": use_target,
      "use_point": use_point,
      "selected_image": selected_image,
      "verification": verification,
    }),
  )
}

#[cfg(not(target_os = "macos"))]
fn click_consumable_use(args: TargetSlotOperationArgs) -> Result<(), CliError> {
  parse_consumable_slot_index(&args.slot)?;
  parse_hand_target_indices(&args.targets)?;
  Err(CliError::Message(
    "consumables use live operation is only available on macOS".to_string(),
  ))
}

#[cfg(target_os = "macos")]
fn click_pack_skip(args: OperationControlArgs) -> Result<(), CliError> {
  use auv_driver::Driver;

  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local()?;
  let window = session
    .window()
    .resolve(Window::main_visible().owned_by(App::name(args.target.clone())))?;
  let before_image = capture_window_to_temp(&session, &window, "pack-skip-before")?;
  let before = observe_image(&before_image, &BalatroModelConfig::default(), true)?;
  let button = find_button(&before, "button_card_pack_skip")?;
  let point = window_point_from_button(&before, &window, button);

  click_game_point(&session, &window, point)?;

  let verification = if args.verify {
    std::thread::sleep(Duration::from_millis(args.timeout_ms.unwrap_or(1200)));
    let after_image = capture_window_to_temp(&session, &window, "pack-skip-after")?;
    match observe_image(&after_image, &BalatroModelConfig::default(), true) {
      Ok(after) => {
        let after_choice_count = active_pack_choices(&after).len();
        Some(json!({
          "mode": args.verify_mode.to_string(),
          "before_phase": before.phase,
          "after_phase": after.phase,
          "before_choice_count": active_pack_choices(&before).len(),
          "after_choice_count": after_choice_count,
          "passed": best_button(&after.buttons, "button_card_pack_skip").is_none()
            || after_choice_count == 0,
          "after_image": after_image,
        }))
      }
      Err(error) => Some(json!({
        "mode": args.verify_mode.to_string(),
        "before_phase": before.phase,
        "before_choice_count": active_pack_choices(&before).len(),
        "passed": false,
        "after_image": after_image,
        "error": error.to_string(),
      })),
    }
  } else {
    None
  };

  write_operation_output(
    args.details,
    json!({
      "operation": "pack.skip",
      "target": args.target,
      "selected_button": button,
      "window_point": { "x": point.x, "y": point.y },
      "before_image": before_image,
      "verification": verification,
    }),
  )
}

#[cfg(not(target_os = "macos"))]
fn click_pack_skip(_args: OperationControlArgs) -> Result<(), CliError> {
  Err(CliError::Message(
    "pack skip live operation is only available on macOS".to_string(),
  ))
}

#[cfg(target_os = "macos")]
fn click_pack_choose(args: TargetSlotOperationArgs) -> Result<(), CliError> {
  use auv_driver::Driver;

  let slot_index = parse_pack_slot_index(&args.slot)?;
  let target_indices = parse_hand_target_indices(&args.targets)?;
  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local()?;
  let window = session
    .window()
    .resolve(Window::main_visible().owned_by(App::name(args.control.target.clone())))?;
  let before_image = capture_window_to_temp(&session, &window, "pack-choose-before")?;
  let before = observe_image(&before_image, &BalatroModelConfig::default(), true)?;
  let choices = active_pack_choices(&before);
  let choice = select_pack_choice(&choices, slot_index)?;
  let already_selected =
    target_indices.is_empty() && best_button(&before.buttons, "button_use").is_some();
  let (choice_point, selected_image, mut selected) = if already_selected {
    (None, before_image.clone(), before.clone())
  } else {
    let choice_point =
      window_point_from_frame_point(&before, &window, bbox_center_point(choice.bbox));

    click_game_point(&session, &window, choice_point)?;
    std::thread::sleep(Duration::from_millis(600));
    let selected_image = capture_window_to_temp(&session, &window, "pack-choose-selected")?;
    let selected = observe_image(&selected_image, &BalatroModelConfig::default(), true)?;
    (Some(choice_point), selected_image, selected)
  };
  let target_selection = if target_indices.is_empty() {
    None
  } else {
    let evidence = click_hand_targets(
      &session,
      &window,
      "pack-choose-targets",
      &selected,
      &target_indices,
      args.control.timeout_ms,
    )?;
    if let Some(after_targets) = evidence.state.clone() {
      selected = after_targets;
    }
    Some(evidence)
  };
  let confirm = resolve_pack_confirm_target(&selected, choice)?;
  let confirm_point = window_point_from_frame_point(&selected, &window, confirm.frame_point);
  click_game_point(&session, &window, confirm_point)?;

  let verification = if args.control.verify {
    std::thread::sleep(Duration::from_millis(
      args.control.timeout_ms.unwrap_or(1200),
    ));
    let after_image = capture_window_to_temp(&session, &window, "pack-choose-after")?;
    match observe_image(&after_image, &BalatroModelConfig::default(), true) {
      Ok(after) => {
        let after_choice_count = active_pack_choices(&after).len();
        Some(json!({
          "mode": args.control.verify_mode.to_string(),
          "before_choice_count": choices.len(),
          "after_choice_count": after_choice_count,
          "passed": best_button(&after.buttons, "button_card_pack_skip").is_none()
            || after_choice_count < choices.len(),
          "after_image": after_image,
        }))
      }
      Err(error) => Some(json!({
        "mode": args.control.verify_mode.to_string(),
        "before_choice_count": choices.len(),
        "passed": false,
        "after_image": after_image,
        "error": error.to_string(),
      })),
    }
  } else {
    None
  };

  write_operation_output(
    args.control.details,
    json!({
      "operation": "pack.choose",
      "target": args.control.target,
      "slot": args.slot,
      "targets": args.targets,
      "choice": choice,
      "choice_point": choice_point,
      "before_image": before_image,
      "target_selection": target_selection,
      "confirm_target": confirm,
      "confirm_point": confirm_point,
      "selected_image": selected_image,
      "verification": verification,
    }),
  )
}

#[cfg(not(target_os = "macos"))]
fn click_pack_choose(args: TargetSlotOperationArgs) -> Result<(), CliError> {
  parse_pack_slot_index(&args.slot)?;
  parse_hand_target_indices(&args.targets)?;
  Err(CliError::Message(
    "pack choose live operation is only available on macOS".to_string(),
  ))
}

#[cfg(target_os = "macos")]
fn click_store_next_round(args: OperationControlArgs) -> Result<(), CliError> {
  use auv_driver::Driver;

  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local()?;
  let window = session
    .window()
    .resolve(Window::main_visible().owned_by(App::name(args.target.clone())))?;
  let before_image = capture_window_to_temp(&session, &window, "store-next-round-before")?;
  let before = observe_image(&before_image, &BalatroModelConfig::default(), true)?;
  let selected_target = resolve_store_next_round_target(&before)?;
  let point = window_point_from_frame_point(&before, &window, selected_target.frame_point);

  session.window().click(
    &window,
    WindowPoint::new(point.x, point.y),
    ClickOptions {
      policy: InputPolicy::ForegroundPreferred,
      ..ClickOptions::default()
    },
  )?;

  let verification = if args.verify {
    std::thread::sleep(Duration::from_millis(args.timeout_ms.unwrap_or(1200)));
    let after_image = capture_window_to_temp(&session, &window, "store-next-round-after")?;
    match observe_image(&after_image, &BalatroModelConfig::default(), true) {
      Ok(after) => Some(json!({
        "mode": args.verify_mode.to_string(),
        "before_phase": before.phase,
        "after_phase": after.phase,
        "passed": has_store_layout_evidence(&before)
          && after.phase != BalatroPhase::Store
          && after.phase != BalatroPhase::Unknown,
        "after_store": after.store,
        "after_image": after_image,
      })),
      Err(error) => Some(json!({
        "mode": args.verify_mode.to_string(),
        "before_phase": before.phase,
        "passed": false,
        "after_image": after_image,
        "error": error.to_string(),
      })),
    }
  } else {
    None
  };

  write_operation_output(
    args.details,
    json!({
      "operation": "store.next_round",
      "target": args.target,
      "selected_target": selected_target,
      "window_point": { "x": point.x, "y": point.y },
      "verification": verification,
    }),
  )
}

#[cfg(not(target_os = "macos"))]
fn click_store_next_round(_args: OperationControlArgs) -> Result<(), CliError> {
  Err(CliError::Message(
    "store next-round live operation is only available on macOS".to_string(),
  ))
}

#[cfg(target_os = "macos")]
fn click_cards_select(args: MultiSlotOperationArgs) -> Result<(), CliError> {
  click_cards("cards.select", None, args)
}

#[cfg(not(target_os = "macos"))]
fn click_cards_select(_args: MultiSlotOperationArgs) -> Result<(), CliError> {
  Err(CliError::Message(
    "cards select live operation is only available on macOS".to_string(),
  ))
}

#[cfg(target_os = "macos")]
fn click_cards_clear(args: OperationControlArgs) -> Result<(), CliError> {
  use auv_driver::Driver;

  let operation = "cards.clear";
  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local()?;
  let window = session
    .window()
    .resolve(Window::main_visible().owned_by(App::name(args.target.clone())))?;
  let (before_image, before_result) = capture_observable_window(
    &session,
    &window,
    operation,
    args.timeout_ms.unwrap_or(1500),
    0,
  )?;
  let before = before_result?;
  let before_interactions = hand_card_interactions(&before);
  let selected_slots = before_interactions
    .iter()
    .filter(|interaction| interaction.selected)
    .map(|interaction| interaction.slot.index)
    .collect::<Vec<_>>();

  let mut click_state = before.clone();
  let mut click_targets = Vec::new();
  for slot_index in selected_slots {
    let card = select_hand_card(&click_state, slot_index)?;
    let point = window_point_from_hand_card(&click_state, &window, card);
    click_targets.push(json!({
      "phase": "clear_existing_selection",
      "slot": card.slot,
      "bbox": card.bbox,
      "point": point,
    }));
    click_game_point(&session, &window, point)?;
    std::thread::sleep(Duration::from_millis(160));
    let (_, state_result) = capture_observable_window(
      &session,
      &window,
      operation,
      args.timeout_ms.unwrap_or(1500),
      120,
    )?;
    click_state = state_result?;
  }

  let (after_image, after_result) = capture_observable_window(
    &session,
    &window,
    operation,
    args.timeout_ms.unwrap_or(1500),
    250,
  )?;
  let after = after_result?;
  let after_interactions = hand_card_interactions(&after);
  let remaining_selected_slots = after_interactions
    .iter()
    .filter(|interaction| interaction.selected)
    .map(|interaction| interaction.slot.index)
    .collect::<Vec<_>>();
  let passed = remaining_selected_slots.is_empty();

  write_operation_output(
    args.details,
    json!({
      "operation": operation,
      "target": args.target,
      "before_image": before_image,
      "after_image": after_image,
      "click_targets": click_targets,
      "selection_evidence": {
        "before_interactions": before_interactions,
        "after_interactions": after_interactions,
        "remaining_selected_slots": remaining_selected_slots,
        "passed": passed,
      },
      "verification": if args.verify {
        Some(json!({
          "mode": args.verify_mode.to_string(),
          "passed": passed,
          "evidence": if passed {
            vec!["no_selected_hand_cards_remaining"]
          } else {
            vec!["selected_hand_cards_remaining"]
          },
        }))
      } else {
        None
      },
    }),
  )?;

  if args.verify && !passed {
    return Err(CliError::Message(
      "card clear verification failed; selected hand cards remain".to_string(),
    ));
  }
  Ok(())
}

#[cfg(not(target_os = "macos"))]
fn click_cards_clear(_args: OperationControlArgs) -> Result<(), CliError> {
  Err(CliError::Message(
    "cards clear live operation is only available on macOS".to_string(),
  ))
}

#[cfg(target_os = "macos")]
fn click_cards_commit(
  operation: &str,
  button_id: &str,
  args: MultiSlotOperationArgs,
) -> Result<(), CliError> {
  click_cards(operation, Some(button_id), args)
}

#[cfg(not(target_os = "macos"))]
fn click_cards_commit(
  _operation: &str,
  _button_id: &str,
  _args: MultiSlotOperationArgs,
) -> Result<(), CliError> {
  Err(CliError::Message(
    "cards play/discard live operations are only available on macOS".to_string(),
  ))
}

#[cfg(target_os = "macos")]
fn click_cards(
  operation: &str,
  commit_button_id: Option<&str>,
  args: MultiSlotOperationArgs,
) -> Result<(), CliError> {
  use auv_driver::Driver;

  let slot_indices = parse_hand_slot_indices(&args.slots)?;
  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local()?;
  let window = session
    .window()
    .resolve(Window::main_visible().owned_by(App::name(args.control.target.clone())))?;
  let (before_image, before_result) = capture_observable_window(
    &session,
    &window,
    operation,
    args.control.timeout_ms.unwrap_or(1500),
    0,
  )?;
  let before = before_result?;
  let cards = select_hand_cards(&before, &slot_indices)?;
  let planned_card_points = cards
    .iter()
    .map(|card| window_point_from_hand_card(&before, &window, card))
    .collect::<Vec<_>>();

  let mut click_state = before.clone();
  let mut click_targets = Vec::new();
  let mut cleared_slots = Vec::new();
  let mut kept_selected_slots = Vec::new();
  if commit_button_id.is_some() {
    kept_selected_slots = selected_hand_slot_indices(&click_state, &slot_indices);
    for slot_index in selected_slots_to_clear(&click_state, &slot_indices) {
      let card = select_hand_card(&click_state, slot_index)?;
      let point = window_point_from_hand_card(&click_state, &window, card);
      click_targets.push(json!({
        "phase": "clear_existing_selection",
        "slot": card.slot,
        "bbox": card.bbox,
        "point": point,
      }));
      click_game_point(&session, &window, point)?;
      cleared_slots.push(slot_index);
      std::thread::sleep(Duration::from_millis(160));
      let (_, state_result) = capture_observable_window(
        &session,
        &window,
        operation,
        args.control.timeout_ms.unwrap_or(1500),
        120,
      )?;
      click_state = state_result?;
    }
  }

  for slot_index in requested_slots_to_select(&click_state, &slot_indices) {
    // NOTICE: Balatro hand-card clicks can be dropped while the card hover
    // description is appearing. Retry only after observing that the slot is
    // still unselected; blind double-clicking would toggle a correctly selected
    // card back off.
    for attempt in 1..=2 {
      if hand_slot_is_selected(&click_state, slot_index) {
        break;
      }
      let card = select_hand_card(&click_state, slot_index)?;
      let point = window_point_from_hand_card(&click_state, &window, card);
      click_targets.push(json!({
        "phase": "select_requested_slot",
        "attempt": attempt,
        "slot": card.slot,
        "bbox": card.bbox,
        "point": point,
      }));
      click_game_point(&session, &window, point)?;
      std::thread::sleep(Duration::from_millis(180));
      let (_, state_result) = capture_observable_window(
        &session,
        &window,
        operation,
        args.control.timeout_ms.unwrap_or(1500),
        120,
      )?;
      click_state = state_result?;
    }
  }

  // Planned hand slots are not trusted as semantic success. Balatro can move
  // cards during selection, the window may change between capture and click,
  // and jokers such as Hook can mutate the hand. Capture the selected state
  // before committing so callers can inspect which cards the game actually
  // accepted. Commit operations treat this as a hard gate: if the requested
  // cards are not raised, the command refuses to press play/discard.
  std::thread::sleep(Duration::from_millis(250));
  let selected_image = capture_window_to_temp(&session, &window, operation)?;
  let selected = observe_image(&selected_image, &BalatroModelConfig::default(), true)?;
  let selected_interactions = hand_card_interactions(&selected);
  let selected_slots = selected_hand_slot_indices(&selected, &hand_slot_indices(&selected));
  let requested_selected_slots = selected_hand_slot_indices(&selected, &slot_indices);
  let selection_passed = hand_selection_matches_requested(&selected, &slot_indices);
  let selection_evidence = json!({
    "selected_image": selected_image,
    "phase": selected.phase,
    "hand_count": selected.hand.len(),
    "requested_slots": slot_indices,
    "cleared_slots": cleared_slots,
    "kept_selected_slots": kept_selected_slots,
    "selected_slots": selected_slots,
    "requested_selected_slots": requested_selected_slots,
    "passed": selection_passed,
    "selected_interactions": selected_interactions,
    "hand": selected.hand.clone(),
    "buttons": selected.buttons.clone(),
  });

  if commit_button_id.is_some() && !selection_passed {
    write_operation_output(
      args.control.details,
      json!({
        "operation": operation,
        "target": args.control.target,
        "slots": args.slots,
        "before_image": before_image,
        "selected_cards": cards,
        "card_points": planned_card_points,
        "click_targets": click_targets,
        "selection_evidence": selection_evidence,
        "verification": {
          "mode": args.control.verify_mode.to_string(),
          "passed": false,
          "evidence": ["target_slots_not_selected_before_commit"],
        },
      }),
    )?;
    return Err(CliError::Message(
      "card selection verification failed before commit; refusing to press play/discard"
        .to_string(),
    ));
  }
  if commit_button_id.is_none() && args.control.verify && !selection_passed {
    write_operation_output(
      args.control.details,
      json!({
        "operation": operation,
        "target": args.control.target,
        "slots": args.slots,
        "before_image": before_image,
        "selected_cards": cards,
        "card_points": planned_card_points,
        "click_targets": click_targets,
        "selection_evidence": selection_evidence,
        "verification": {
          "mode": args.control.verify_mode.to_string(),
          "passed": false,
          "evidence": ["selected_hand_slots_do_not_match_requested_slots"],
        },
      }),
    )?;
    return Err(CliError::Message(
      "card select verification failed; selected hand slots do not match requested slots"
        .to_string(),
    ));
  }

  let mut commit_button = None;
  let mut button_point = None;
  if let Some(button_id) = commit_button_id {
    let button = find_button(&selected, button_id)?;
    let point = window_point_from_button(&selected, &window, button);
    session.window().click(
      &window,
      WindowPoint::new(point.x, point.y),
      ClickOptions {
        policy: InputPolicy::ForegroundPreferred,
        ..ClickOptions::default()
      },
    )?;
    commit_button = Some(button.clone());
    button_point = Some(point);
  }

  let verification = if args.control.verify {
    let (after_image, after_result) = capture_observable_window(
      &session,
      &window,
      operation,
      args.control.timeout_ms.unwrap_or(1500),
      600,
    )?;
    match after_result {
      Ok(after) => Some(json!({
        "mode": args.control.verify_mode.to_string(),
        "before_phase": before.phase,
        "after_phase": after.phase,
        "before_hand_count": before.hand.len(),
        "after_hand_count": after.hand.len(),
        "evidence": card_operation_evidence(operation, &before, &after),
        "passed": verify_card_operation(operation, &before, &after),
        "after_image": after_image,
      })),
      Err(error) => Some(json!({
        "mode": args.control.verify_mode.to_string(),
        "before_phase": before.phase,
        "before_hand_count": before.hand.len(),
        "passed": false,
        "after_image": after_image,
        "error": error.to_string(),
      })),
    }
  } else {
    None
  };

  write_operation_output(
    args.control.details,
    json!({
      "operation": operation,
      "target": args.control.target,
      "slots": args.slots,
      "before_image": before_image,
      "selected_cards": cards,
      "card_points": planned_card_points,
      "click_targets": click_targets,
      "selection_evidence": selection_evidence,
      "commit_button": commit_button,
      "button_point": button_point,
      "verification": verification,
    }),
  )
}

#[cfg(target_os = "macos")]
#[derive(Debug, Serialize)]
struct HandTargetSelection {
  selected_image: PathBuf,
  requested_slots: Vec<u32>,
  selected_slots: Vec<u32>,
  passed: bool,
  click_targets: Vec<Value>,
  #[serde(skip)]
  state: Option<BalatroState>,
}

#[cfg(target_os = "macos")]
fn click_hand_targets(
  session: &auv_driver_macos::MacosDriverSession,
  window: &auv_driver::window::Window,
  operation: &str,
  before: &BalatroState,
  slot_indices: &[u32],
  timeout_ms: Option<u64>,
) -> Result<HandTargetSelection, CliError> {
  let _cards = select_hand_cards(before, slot_indices)?;
  let mut click_state = before.clone();
  let mut click_targets = Vec::new();
  for slot_index in selected_slots_to_clear(&click_state, slot_indices) {
    let card = select_hand_card(&click_state, slot_index)?;
    let point = window_point_from_hand_card(&click_state, window, card);
    click_targets.push(json!({
      "phase": "clear_existing_selection",
      "slot": card.slot,
      "bbox": card.bbox,
      "point": point,
    }));
    click_game_point(session, window, point)?;
    std::thread::sleep(Duration::from_millis(160));
    let (_, state_result) =
      capture_observable_window(session, window, operation, timeout_ms.unwrap_or(1500), 120)?;
    click_state = state_result?;
  }

  for slot_index in requested_slots_to_select(&click_state, slot_indices) {
    for attempt in 1..=2 {
      if hand_slot_is_selected(&click_state, slot_index) {
        break;
      }
      let card = select_hand_card(&click_state, slot_index)?;
      let point = window_point_from_hand_card(&click_state, window, card);
      click_targets.push(json!({
        "phase": "select_requested_slot",
        "attempt": attempt,
        "slot": card.slot,
        "bbox": card.bbox,
        "point": point,
      }));
      click_game_point(session, window, point)?;
      std::thread::sleep(Duration::from_millis(180));
      let (_, state_result) =
        capture_observable_window(session, window, operation, timeout_ms.unwrap_or(1500), 120)?;
      click_state = state_result?;
    }
  }

  std::thread::sleep(Duration::from_millis(250));
  let selected_image = capture_window_to_temp(session, window, operation)?;
  let selected = observe_image(&selected_image, &BalatroModelConfig::default(), true)?;
  let selected_slots = selected_hand_slot_indices(&selected, &hand_slot_indices(&selected));
  let passed = hand_selection_matches_requested(&selected, slot_indices);
  if !passed {
    return Err(CliError::Message(
      "target hand selection verification failed; refusing to use consumable".to_string(),
    ));
  }

  Ok(HandTargetSelection {
    selected_image,
    requested_slots: slot_indices.to_vec(),
    selected_slots,
    passed,
    click_targets,
    state: Some(selected),
  })
}

#[cfg(target_os = "macos")]
fn click_blind_select(args: SlotOperationArgs) -> Result<(), CliError> {
  let slot_index = parse_blind_slot_index(&args.slot)?;
  click_blind_button(
    "blinds.select",
    "button_level_select",
    Some(slot_index),
    args.control,
  )
}

#[cfg(not(target_os = "macos"))]
fn click_blind_select(_args: SlotOperationArgs) -> Result<(), CliError> {
  Err(CliError::Message(
    "blinds select live operation is only available on macOS".to_string(),
  ))
}

#[cfg(target_os = "macos")]
fn click_blind_skip(args: OperationControlArgs) -> Result<(), CliError> {
  click_blind_button("blinds.skip", "button_level_skip", None, args)
}

#[cfg(not(target_os = "macos"))]
fn click_blind_skip(_args: OperationControlArgs) -> Result<(), CliError> {
  Err(CliError::Message(
    "blinds skip live operation is only available on macOS".to_string(),
  ))
}

#[cfg(target_os = "macos")]
fn click_blind_button(
  operation: &str,
  button_id: &str,
  slot_index: Option<u32>,
  args: OperationControlArgs,
) -> Result<(), CliError> {
  use auv_driver::Driver;

  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local()?;
  let window = session
    .window()
    .resolve(Window::main_visible().owned_by(App::name(args.target.clone())))?;
  let before_image = capture_window_to_temp(&session, &window, operation)?;
  let before = observe_image(&before_image, &BalatroModelConfig::default(), true)?;
  let button = select_button_for_slot(&before.buttons, button_id, slot_index)?;
  let point = window_point_from_button(&before, &window, button);

  session.window().click(
    &window,
    WindowPoint::new(point.x, point.y),
    ClickOptions {
      policy: InputPolicy::ForegroundPreferred,
      ..ClickOptions::default()
    },
  )?;

  let verification = if args.verify {
    let (after_image, after_result) = capture_observable_window(
      &session,
      &window,
      operation,
      args.timeout_ms.unwrap_or(1200),
      500,
    )?;
    match after_result {
      Ok(after) => {
        let passed = match operation {
          "blinds.select" => {
            before.phase == BalatroPhase::BlindSelect
              && (after.phase == BalatroPhase::Playing || !after.hand.is_empty())
          }
          "blinds.skip" => {
            before.phase == BalatroPhase::BlindSelect && after.phase != BalatroPhase::BlindSelect
          }
          _ => false,
        };
        Some(json!({
          "mode": args.verify_mode.to_string(),
          "before_phase": before.phase,
          "after_phase": after.phase,
          "after_hand_count": after.hand.len(),
          "passed": passed,
          "after_image": after_image,
        }))
      }
      Err(error) => Some(json!({
        "mode": args.verify_mode.to_string(),
        "before_phase": before.phase,
        "passed": false,
        "after_image": after_image,
        "error": error.to_string(),
      })),
    }
  } else {
    None
  };

  write_operation_output(
    args.details,
    json!({
      "operation": operation,
      "target": args.target,
      "slot_index": slot_index,
      "selected_button": button,
      "window_point": { "x": point.x, "y": point.y },
      "verification": verification,
    }),
  )
}

#[cfg(target_os = "macos")]
fn click_single_button(
  operation: &str,
  button_id: &str,
  args: OperationControlArgs,
) -> Result<(), CliError> {
  use auv_driver::Driver;

  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local()?;
  let window = session
    .window()
    .resolve(Window::main_visible().owned_by(App::name(args.target.clone())))?;
  let before_image = capture_window_to_temp(&session, &window, operation)?;
  let before = observe_image(&before_image, &BalatroModelConfig::default(), true)?;
  let button = find_button(&before, button_id)?;
  let point = window_point_from_button(&before, &window, button);

  click_game_point(&session, &window, point)?;

  let verification = if args.verify {
    let (after_image, after_result) = capture_observable_window(
      &session,
      &window,
      operation,
      args.timeout_ms.unwrap_or(1200),
      500,
    )?;
    match after_result {
      Ok(after) => Some(json!({
        "mode": args.verify_mode.to_string(),
        "before_phase": before.phase,
        "after_phase": after.phase,
        "button_still_visible": best_button(&after.buttons, button_id).is_some(),
        "passed": verify_single_button_activation(button_id, &before, &after),
        "after_image": after_image,
      })),
      Err(error) => Some(json!({
        "mode": args.verify_mode.to_string(),
        "before_phase": before.phase,
        "passed": false,
        "after_image": after_image,
        "error": error.to_string(),
      })),
    }
  } else {
    None
  };

  write_operation_output(
    args.details,
    json!({
      "operation": operation,
      "target": args.target,
      "button": button,
      "window_point": point,
      "before_image": before_image,
      "verification": verification,
    }),
  )
}

#[cfg(target_os = "macos")]
fn observe_live_target(
  target: &str,
  config: &BalatroModelConfig,
  no_cache: bool,
) -> Result<BalatroState, CliError> {
  use auv_driver::Driver;

  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local()?;
  let window = session
    .window()
    .resolve(Window::main_visible().owned_by(App::name(target.to_string())))?;
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
  use auv_driver::Driver;

  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local()?;
  let window = session
    .window()
    .resolve(Window::main_visible().owned_by(App::name(target.to_string())))?;
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
      let mut result = read_card_from_capture(
        &session,
        &capture,
        &frame,
        &state,
        card,
        rank_templates.as_deref(),
      )?;
      if should_hover_reread_card(&result.reading)
        && let Some(hover) = hover_reread_card(
          &session,
          &window,
          config,
          no_cache,
          &state,
          card,
          rank_templates.as_deref(),
        )?
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
  use auv_driver::Driver;

  let config = BalatroModelConfig::from_observe_args(args);
  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local()?;
  let window = session
    .window()
    .resolve(Window::main_visible().owned_by(App::name(args.target.clone())))?;
  let capture = capture_window(&session, &window)?;
  let frame = save_capture_to_temp(&capture, "pack-read")?;
  let state = observe_image_with_ui_readings(&frame, &config, args.no_cache)?;
  let mut choices = active_pack_choices(&state);
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
  choice: &mut PackChoice,
) -> Result<(), CliError> {
  let point = window_point_from_frame_point(state, window, bbox_center_point(choice.bbox));
  let screen = session
    .window()
    .to_screen_point(window, WindowPoint::new(point.x, point.y))?;
  let screen = screen.point();
  auv_driver_macos::native::pointer::move_point(screen.x, screen.y, 0)
    .map_err(CliError::Message)?;
  std::thread::sleep(Duration::from_millis(450));

  let capture = capture_window(session, window)?;
  let frame = save_capture_to_temp(&capture, "pack-hover-read")?;
  let region = pack_choice_hover_ocr_region();
  let recognition = session.vision().recognize_text_in_capture_with_options(
    &capture,
    region,
    TextRecognitionOptions::default()
      .with_recognition_languages(["zh-Hans", "en-US"])
      .with_custom_words(pack_ocr_words()),
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
  let screen = session
    .window()
    .to_screen_point(window, WindowPoint::new(point.x, point.y))?;
  let screen = screen.point();
  auv_driver_macos::native::pointer::move_point(screen.x, screen.y, 0)
    .map_err(CliError::Message)?;
  std::thread::sleep(Duration::from_millis(450));

  let capture = capture_window(session, window)?;
  let frame = save_capture_to_temp(&capture, "object-hover-read")?;
  let region = object_hover_ocr_region();
  let recognition = session.vision().recognize_text_in_capture_with_options(
    &capture,
    region,
    TextRecognitionOptions::default()
      .with_recognition_languages(["zh-Hans", "en-US"])
      .with_custom_words(object_ocr_words()),
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
) -> Result<(), CliError> {
  session.window().click(
    window,
    WindowPoint::new(point.x, point.y),
    ClickOptions {
      policy: InputPolicy::ForegroundPreferred,
      ..ClickOptions::default()
    },
  )?;
  Ok(())
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
    TextRecognitionOptions::default()
      .with_custom_words(card_ocr_words())
      .with_recognition_languages(["zh-Hans", "en-US"]),
  )?;
  let crop = save_capture_to_temp(&corner_capture, "card-corner")?;
  let suit = infer_suit_from_card_corner(capture, state, card);
  let mut source = "macos_vision_corner_ocr".to_string();
  let mut reading = parse_card_reading(&recognition.text, suit, None);
  if reading.rank.is_none()
    && let Some((rank, confidence)) =
      infer_rank_from_deck_template(&corner_capture.image, rank_templates, suit)
  {
    apply_inferred_rank(&mut reading, rank, confidence);
    source = format!("{source}+deck_template_rank");
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
  let screen = session
    .window()
    .to_screen_point(window, WindowPoint::new(point.x, point.y))?;
  let screen = screen.point();
  auv_driver_macos::native::pointer::move_point(screen.x, screen.y, 0)
    .map_err(CliError::Message)?;
  std::thread::sleep(Duration::from_millis(350));

  let capture = capture_window(session, window)?;
  let frame = save_capture_to_temp(&capture, "cards-hover-read")?;
  let hover_state = observe_image_with_ui_readings(&frame, config, no_cache)?;
  let hover_card = match select_hand_card(&hover_state, card.slot.index) {
    Ok(card) => card,
    Err(_) => return Ok(None),
  };
  let mut result = read_card_from_capture(
    session,
    &capture,
    &frame,
    &hover_state,
    hover_card,
    rank_templates,
  )?;
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
  let completeness = match (
    reading.rank.is_some(),
    reading.suit.is_some(),
    reading.valid,
  ) {
    (_, _, true) => 3,
    (true, false, false) | (false, true, false) => 2,
    _ => 1,
  };
  let confidence = reading
    .confidence
    .map(|confidence| (confidence.clamp(0.0, 1.0) * 100.0).round() as u8)
    .unwrap_or(0);
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
fn observe_live_target(
  _target: &str,
  _config: &BalatroModelConfig,
  _no_cache: bool,
) -> Result<BalatroState, CliError> {
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
fn capture_window(
  session: &auv_driver_macos::MacosDriverSession,
  window: &auv_driver::window::Window,
) -> Result<Capture, CliError> {
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
      CliError::Message(format!(
        "window capture failed ({primary_error}); display-region fallback also failed ({fallback_error})"
      ))
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
  let path = std::env::temp_dir().join(format!(
    "auv-game-balatro-{prefix}-{}-{}.png",
    std::process::id(),
    unique_nanos()
  ));
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
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|duration| duration.as_nanos())
    .unwrap_or_default()
}

fn find_button<'a>(state: &'a BalatroState, id: &str) -> Result<&'a ButtonTarget, CliError> {
  select_button_for_slot(&state.buttons, id, None)
}

fn select_store_buy_confirm_button(state: &BalatroState) -> Result<&ButtonTarget, CliError> {
  best_button(&state.buttons, "button_purchase")
    .or_else(|| best_button(&state.buttons, "button_use"))
    .ok_or_else(|| {
      CliError::Message(
        "could not find button_purchase or button_use in selected store item frame".to_string(),
      )
    })
}

fn best_button<'a>(buttons: &'a [ButtonTarget], id: &str) -> Option<&'a ButtonTarget> {
  buttons
    .iter()
    .filter(|button| button.id == id)
    .max_by(|left, right| {
      left
        .confidence
        .partial_cmp(&right.confidence)
        .unwrap_or(std::cmp::Ordering::Equal)
    })
}

fn restart_primary_button(buttons: &[ButtonTarget]) -> Option<&ButtonTarget> {
  best_button(buttons, "button_new_run_play")
    .or_else(|| best_button(buttons, "button_main_menu_play"))
}

fn resolve_store_next_round_target(state: &BalatroState) -> Result<ResolvedActionTarget, CliError> {
  if let Some(button) = best_button(&state.buttons, "button_store_next_round") {
    return Ok(ResolvedActionTarget {
      source: ActionTargetSource::YoloButton,
      label: button.id.clone(),
      frame_point: bbox_center_point(button.bbox),
      fallback_reason: None,
    });
  }

  let has_store_evidence = has_store_layout_evidence(state);
  let purchase_visible = best_button(&state.buttons, "button_purchase").is_some();
  let pack_choices_visible = best_button(&state.buttons, "button_card_pack_skip").is_some()
    || !active_pack_choices(state).is_empty();
  if has_store_evidence && !purchase_visible && !pack_choices_visible {
    // Store phase classification or the derived store flag is the evidence
    // that keeps this fallback scoped to the live store screen; a visible
    // purchase button means a store item is selected, so this point is unsafe.
    // NOTICE: Fallback ratios come from a live 1646x963 store capture point
    // around (594,429), normalized to (0.361,0.446). Remove this once stable
    // YOLO/button target detection covers `button_store_next_round`.
    return Ok(ResolvedActionTarget {
      source: ActionTargetSource::LayoutFallback,
      label: "button_store_next_round".to_string(),
      frame_point: Point::new(
        f64::from(state.frame.image_size.width) * 0.361,
        f64::from(state.frame.image_size.height) * 0.446,
      ),
      fallback_reason: Some("yolo_button_missing_visible_layout_match".to_string()),
    });
  }

  Err(CliError::Message(
    "could not find button_store_next_round in observed Balatro frame".to_string(),
  ))
}

fn resolve_consumable_use_target(
  state: &BalatroState,
  slot_index: u32,
) -> Result<ResolvedActionTarget, CliError> {
  if let Some(button) = best_button(&state.buttons, "button_use") {
    return Ok(ResolvedActionTarget {
      source: ActionTargetSource::YoloButton,
      label: button.id.clone(),
      frame_point: bbox_center_point(button.bbox),
      fallback_reason: None,
    });
  }

  let consumable = select_consumable(state, slot_index)?;
  let width = consumable.bbox.width().max(1.0);
  let height = consumable.bbox.height().max(1.0);
  let x = (consumable.bbox.x2 + width * 0.32).min(state.frame.image_size.width as f32 - 1.0);
  let y = (consumable.bbox.y1 + height * 0.60).min(state.frame.image_size.height as f32 - 1.0);
  Ok(ResolvedActionTarget {
    source: ActionTargetSource::LayoutFallback,
    label: "button_use".to_string(),
    frame_point: Point::new(f64::from(x), f64::from(y)),
    fallback_reason: Some("consumable_use_button_missing_selected_card_layout_match".to_string()),
  })
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
    matches!(
      detection.label.as_str(),
      "joker_card" | "tarot_card" | "planet_card" | "card_pack"
    ) && center_x > width * 0.42
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

fn resolve_pack_confirm_target(
  state: &BalatroState,
  choice: &PackChoice,
) -> Result<ResolvedActionTarget, CliError> {
  if let Some(button) = best_button(&state.buttons, "button_use") {
    return Ok(ResolvedActionTarget {
      source: ActionTargetSource::YoloButton,
      label: button.id.clone(),
      frame_point: bbox_center_point(button.bbox),
      fallback_reason: None,
    });
  }

  if best_button(&state.buttons, "button_card_pack_skip").is_none()
    && active_pack_choices(state).is_empty()
  {
    return Err(CliError::Message(
      "could not resolve pack confirm target without active pack evidence".to_string(),
    ));
  }

  // NOTICE: The 0.82 height fallback comes from live active-pack captures where
  // Balatro places the confirm button below the selected choice. Remove this
  // fallback once `button_use` detection is stable for active pack selections.
  Ok(ResolvedActionTarget {
    source: ActionTargetSource::LayoutFallback,
    label: "pack_confirm".to_string(),
    frame_point: Point::new(
      f64::from((choice.bbox.x1 + choice.bbox.x2) / 2.0),
      f64::from(state.frame.image_size.height) * 0.82,
    ),
    fallback_reason: Some("pack_confirm_button_missing_visible_layout_match".to_string()),
  })
}

fn blind_buttons(state: &BalatroState) -> Vec<&ButtonTarget> {
  let mut buttons = state
    .buttons
    .iter()
    .filter(|button| {
      matches!(
        button.id.as_str(),
        "button_level_select" | "button_level_skip"
      )
    })
    .collect::<Vec<_>>();
  buttons.sort_by(|left, right| {
    left
      .bbox
      .x1
      .partial_cmp(&right.bbox.x1)
      .unwrap_or(std::cmp::Ordering::Equal)
  });
  buttons
}

fn select_button_for_slot<'a>(
  buttons: &'a [ButtonTarget],
  id: &str,
  slot_index: Option<u32>,
) -> Result<&'a ButtonTarget, CliError> {
  let mut matches = buttons
    .iter()
    .filter(|button| button.id == id)
    .collect::<Vec<_>>();

  if let Some(index) = slot_index {
    matches.sort_by(|left, right| {
      left
        .bbox
        .x1
        .partial_cmp(&right.bbox.x1)
        .unwrap_or(std::cmp::Ordering::Equal)
    });
    matches
      .get(index as usize)
      .copied()
      .ok_or_else(|| CliError::Message(format!("could not find {id} at blind:{index}")))
  } else {
    matches
      .into_iter()
      .max_by(|left, right| {
        left
          .confidence
          .partial_cmp(&right.confidence)
          .unwrap_or(std::cmp::Ordering::Equal)
      })
      .ok_or_else(|| CliError::Message(format!("could not find {id} in observed Balatro frame")))
  }
}

fn parse_blind_slot_index(slot: &str) -> Result<u32, CliError> {
  let Some(index) = slot.strip_prefix("blind:") else {
    return Err(CliError::Message(format!(
      "blind select requires --slot blind:N, got {slot}"
    )));
  };
  index
    .parse::<u32>()
    .map_err(|_| CliError::Message(format!("blind slot index must be an integer, got {slot}")))
}

fn parse_prefixed_slot_index(slot: &str, prefix: &str) -> Result<u32, CliError> {
  let slot = slot.trim();
  let expected = format!("{prefix}:");
  let Some(index) = slot.strip_prefix(&expected) else {
    return Err(CliError::Message(format!(
      "object operation requires --slot {prefix}:N, got {slot}"
    )));
  };
  index.parse::<u32>().map_err(|_| {
    CliError::Message(format!(
      "{prefix} slot index must be an integer, got {slot}"
    ))
  })
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
        return Err(CliError::Message(format!(
          "card operation requires --slots hand:N[,hand:N...], got {slot}"
        )));
      };
      index
        .parse::<u32>()
        .map_err(|_| CliError::Message(format!("hand slot index must be an integer, got {slot}")))
    })
    .collect()
}

fn parse_hand_target_indices(targets: &[String]) -> Result<Vec<u32>, CliError> {
  targets
    .iter()
    .map(|target| {
      parse_prefixed_slot_index(target, "hand").map_err(|_| {
        CliError::Message(format!(
          "targeted consumable operation requires --targets hand:N[,hand:N...], got {target}"
        ))
      })
    })
    .collect()
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
      if state.phase != BalatroPhase::Playing && is_score_ui_label(label) {
        return None;
      }
      is_numeric_ui_label(label).then(|| {
        crop_detection_to_temp(&image, evidence.detection.bbox, label)
          .map(|crop| (label.to_string(), crop))
      })?
    })
    .collect::<Vec<_>>();

  for (label, crop) in crops {
    if is_single_ui_digit_label(&label)
      && let Some(digit) = infer_single_ui_digit_from_crop(&crop)
      && is_allowed_single_ui_digit(&label, digit)
    {
      apply_ui_numeric_reading(
        &label,
        &digit.to_string(),
        &mut state.scores,
        &mut state.rounds,
      );
      continue;
    }
    if use_score_digit_reader(&label)
      && let Some(text) =
        infer_ui_digit_text_from_crop_with_foreground(&crop, score_ui_digit_foreground(&label))
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

fn is_score_ui_label(label: &str) -> bool {
  matches!(
    label,
    "ui_score_chips"
      | "ui_score_current"
      | "ui_score_mult"
      | "ui_score_round_score"
      | "ui_score_target_score"
  )
}

fn is_numeric_ui_label(label: &str) -> bool {
  matches!(
    label,
    "ui_score_chips"
      | "ui_score_current"
      | "ui_score_mult"
      | "ui_score_round_score"
      | "ui_score_target_score"
      | "ui_data_cash"
      | "ui_data_discards_left"
      | "ui_data_hands_left"
      | "ui_round_ante_current"
      | "ui_round_ante_left"
      | "ui_round_round_current"
      | "ui_round_round_left"
  )
}

fn is_single_ui_digit_label(label: &str) -> bool {
  matches!(
    label,
    "ui_data_discards_left"
      | "ui_data_hands_left"
      | "ui_round_ante_current"
      | "ui_round_ante_left"
      | "ui_round_round_current"
      | "ui_round_round_left"
  )
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
  matches!(
    label,
    "ui_score_chips" | "ui_score_current" | "ui_score_mult" | "ui_score_round_score"
  )
}

fn apply_ui_numeric_reading(
  label: &str,
  text: &str,
  scores: &mut ScoreState,
  rounds: &mut RoundState,
) {
  let Some(value) = normalize_ui_numeric_text_for_label(label, text) else {
    return;
  };
  match label {
    "ui_score_chips" => scores.chips = Some(value),
    "ui_score_current" => scores.current_score = Some(value),
    "ui_score_mult" => scores.mult = Some(value),
    "ui_score_round_score" => scores.round_score = Some(value),
    "ui_score_target_score" => scores.target_score = Some(value),
    "ui_data_cash" => rounds.cash = Some(value),
    "ui_data_discards_left" => rounds.discards_left = Some(value),
    "ui_data_hands_left" => rounds.hands_left = Some(value),
    "ui_round_ante_current" => rounds.ante_current = Some(value),
    "ui_round_ante_left" => rounds.ante_left = Some(value),
    "ui_round_round_current" => rounds.round_current = Some(value),
    "ui_round_round_left" => rounds.round_left = Some(value),
    _ => {}
  }
}

fn infer_single_ui_digit_from_crop(crop: &Path) -> Option<u8> {
  let image = image::open(crop).ok()?.to_rgba8();
  let reading = infer_ui_digit_text_from_image_with_foreground(&image, UiDigitForeground::Colored)?;
  let mut chars = reading.chars();
  let digit = chars.next()?.to_digit(10)? as u8;
  chars.next().is_none().then_some(digit)
}

fn infer_ui_digit_text_from_crop_with_foreground(
  crop: &Path,
  foreground: UiDigitForeground,
) -> Option<String> {
  let image = image::open(crop).ok()?.to_rgba8();
  infer_ui_digit_text_from_image_with_foreground(&image, foreground)
}

fn infer_ui_digit_text_from_image_with_foreground(
  image: &RgbaImage,
  foreground: UiDigitForeground,
) -> Option<String> {
  let mut digits = String::new();
  for points in ui_digit_glyph_segments(image, foreground)? {
    let mask = normalized_ui_digit_mask_from_points(points)?;
    if let Some(digit) = infer_ui_digit_from_mask(&mask) {
      digits.push(char::from(b'0' + digit));
    }
  }
  (!digits.is_empty()).then_some(digits)
}

fn infer_ui_digit_from_mask(mask: &[bool; UI_DIGIT_MASK_CELLS]) -> Option<u8> {
  UI_DIGIT_TEMPLATES
    .iter()
    .map(|template| (template.digit, ui_digit_mask_distance(&mask, template.rows)))
    .min_by(|left, right| {
      left
        .1
        .partial_cmp(&right.1)
        .unwrap_or(std::cmp::Ordering::Equal)
    })
    .and_then(|(digit, distance)| (distance <= 0.32).then_some(digit))
}

fn normalized_ui_digit_mask_from_points(
  foreground: Vec<(u32, u32)>,
) -> Option<[bool; UI_DIGIT_MASK_CELLS]> {
  let min_x = foreground.iter().map(|(x, _)| *x).min()?;
  let min_y = foreground.iter().map(|(_, y)| *y).min()?;
  let max_x = foreground.iter().map(|(x, _)| *x).max()?;
  let max_y = foreground.iter().map(|(_, y)| *y).max()?;
  let width = (max_x - min_x + 1).max(1);
  let height = (max_y - min_y + 1).max(1);
  let foreground = foreground
    .into_iter()
    .collect::<std::collections::HashSet<_>>();
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

fn ui_digit_glyph_segments(
  image: &RgbaImage,
  foreground: UiDigitForeground,
) -> Option<Vec<Vec<(u32, u32)>>> {
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

  let mut segments = segments
    .into_iter()
    .filter(|segment| segment.len() >= 20)
    .collect::<Vec<_>>();
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
      "#####", "#....", "#....", "#####", "....#", "....#", "#####",
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

fn normalize_ui_numeric_text_for_label(label: &str, text: &str) -> Option<String> {
  let value = normalize_ui_numeric_text(text)?;
  if is_single_ui_digit_label(label) {
    return value
      .chars()
      .find(|character| character.is_ascii_digit())
      .map(|character| character.to_string());
  }
  Some(value)
}

fn normalize_ui_numeric_text(text: &str) -> Option<String> {
  let normalized = text
    .chars()
    .filter_map(|character| match character {
      '0'..='9' | '$' | '/' | '+' | '-' | '.' => Some(character),
      'x' | 'X' | '×' => Some('x'),
      'O' | 'o' | '〇' | '○' => Some('0'),
      ',' | ' ' | '\n' | '\r' | '\t' => None,
      _ => None,
    })
    .collect::<String>();
  (!normalized.is_empty()).then_some(normalized)
}

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
  let path = std::env::temp_dir().join(format!(
    "auv-game-balatro-ui-{}-{}-{}.png",
    label,
    std::process::id(),
    unique_nanos()
  ));
  resized.save(&path).ok()?;
  Some(path)
}

fn select_hand_cards<'a>(
  state: &'a BalatroState,
  slot_indices: &[u32],
) -> Result<Vec<&'a CardSlot>, CliError> {
  slot_indices
    .iter()
    .map(|index| select_hand_card(state, *index))
    .collect()
}

fn select_hand_card(state: &BalatroState, slot_index: u32) -> Result<&CardSlot, CliError> {
  state
    .hand
    .get(slot_index as usize)
    .ok_or_else(|| CliError::Message(format!("could not find hand:{slot_index}")))
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
      bbox: card.bbox.clone(),
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
  selected_hand_slot_indices(state, &hand_slot_indices(state))
    .into_iter()
    .filter(|slot_index| !requested.contains(slot_index))
    .collect()
}

fn requested_slots_to_select(state: &BalatroState, requested: &[u32]) -> Vec<u32> {
  requested
    .iter()
    .copied()
    .filter(|slot_index| !hand_slot_is_selected(state, *slot_index))
    .collect()
}

fn hand_slot_is_selected(state: &BalatroState, slot_index: u32) -> bool {
  if best_button(&state.buttons, "button_play").is_none()
    && best_button(&state.buttons, "button_discard").is_none()
    && best_button(&state.buttons, "button_use").is_none()
  {
    return false;
  }
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
  card.bbox.y1 <= baseline_y - 18.0
}

fn hand_selection_baseline_y(state: &BalatroState) -> Option<f32> {
  state
    .hand
    .iter()
    .map(|card| card.bbox.y1)
    .max_by(|left, right| left.total_cmp(right))
}

fn hand_slot_indices(state: &BalatroState) -> Vec<u32> {
  (0..state.hand.len() as u32).collect()
}

fn select_cards_for_read<'a>(
  state: &'a BalatroState,
  requested: &Option<Vec<u32>>,
) -> Result<Vec<&'a CardSlot>, CliError> {
  match requested {
    Some(indices) => select_hand_cards(state, indices),
    None => Ok(state.hand.iter().collect()),
  }
}

fn select_store_item(state: &BalatroState, index: u32) -> Result<&StoreItem, CliError> {
  state
    .store
    .items
    .get(index as usize)
    .ok_or_else(|| CliError::Message(format!("could not find store:{index}")))
}

fn select_joker(state: &BalatroState, index: u32) -> Result<&JokerSlot, CliError> {
  state
    .jokers
    .get(index as usize)
    .ok_or_else(|| CliError::Message(format!("could not find joker:{index}")))
}

fn select_consumable(state: &BalatroState, index: u32) -> Result<&ConsumableSlot, CliError> {
  state
    .consumables
    .get(index as usize)
    .ok_or_else(|| CliError::Message(format!("could not find consumable:{index}")))
}

fn object_read_from_state(
  state: &BalatroState,
  slot: &str,
  zone: ObjectReadZone,
) -> Result<ObjectReadResult, CliError> {
  let (slot, kind, bbox, confidence) = match zone {
    ObjectReadZone::Store => {
      let index = parse_store_slot_index(slot)?;
      let item = select_store_item(state, index)?;
      (
        item.slot,
        object_kind_label(&item.kind)?,
        item.bbox,
        item.confidence,
      )
    }
    ObjectReadZone::Joker => {
      let index = parse_joker_slot_index(slot)?;
      let joker = select_joker(state, index)?;
      (
        joker.slot,
        "joker".to_string(),
        joker.bbox,
        joker.confidence,
      )
    }
    ObjectReadZone::Consumable => {
      let index = parse_consumable_slot_index(slot)?;
      let consumable = select_consumable(state, index)?;
      (
        consumable.slot,
        object_kind_label(&consumable.kind)?,
        consumable.bbox,
        consumable.confidence,
      )
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
      let in_choice_area = center_x > width * 0.28
        && center_x < width * 0.78
        && center_y > height * 0.55
        && center_y < height * 0.86;
      let is_choice = matches!(
        detection.label.as_str(),
        "joker_card" | "tarot_card" | "planet_card" | "spectral_card" | "poker_card_front"
      ) && in_choice_area;
      is_choice.then(|| PackChoice {
        slot_index: 0,
        kind: detection.label.clone(),
        detector_label: detection.label.clone(),
        hint: pack_choice_hint(&detection.label).to_string(),
        hover_required: true,
        hover_text: None,
        hover_frame: None,
        hover_ocr_region: None,
        hover_error: None,
        bbox: detection.bbox,
        confidence: detection.confidence,
      })
    })
    .collect::<Vec<_>>();
  choices.sort_by(|left, right| {
    left
      .bbox
      .x1
      .partial_cmp(&right.bbox.x1)
      .unwrap_or(std::cmp::Ordering::Equal)
  });
  for (index, choice) in choices.iter_mut().enumerate() {
    choice.slot_index = index as u32;
  }
  choices
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
  choices
    .get(index as usize)
    .ok_or_else(|| CliError::Message(format!("could not find pack:{index}")))
}

fn ocr_region_for_card(state: &BalatroState, card: &CardSlot) -> RatioRect {
  let width = f64::from(state.frame.image_size.width).max(1.0);
  let height = f64::from(state.frame.image_size.height).max(1.0);
  let card_w = f64::from(card.bbox.width().max(1.0));
  let card_h = f64::from(card.bbox.height().max(1.0));
  RatioRect::new(
    f64::from(card.bbox.x1) / width,
    f64::from(card.bbox.y1) / height,
    (card_w * 0.38) / width,
    (card_h * 0.46) / height,
  )
}

#[cfg(target_os = "macos")]
fn card_corner_capture(capture: &Capture, state: &BalatroState, card: &CardSlot) -> Capture {
  let (x, y, width, height) = card_corner_pixels(capture, state, card);
  let crop = image::imageops::crop_imm(&capture.image, x, y, width, height).to_image();
  let scale = 6;
  let resized = image::imageops::resize(
    &crop,
    width * scale,
    height * scale,
    image::imageops::FilterType::Nearest,
  );
  Capture {
    image: resized,
    bounds: Rect::new(
      0.0,
      0.0,
      f64::from(width * scale),
      f64::from(height * scale),
    ),
    scale_factor: capture.scale_factor,
    backend: format!("{}:card-corner", capture.backend),
    fallback_reason: capture.fallback_reason.clone(),
  }
}

#[cfg(target_os = "macos")]
fn infer_suit_from_card_corner(
  capture: &Capture,
  state: &BalatroState,
  card: &CardSlot,
) -> Option<&'static str> {
  let (x, y, width, height) = card_corner_pixels(capture, state, card);
  let mut hearts = 0u32;
  let mut diamonds = 0u32;
  let mut clubs = 0u32;
  let mut spades = 0u32;
  for py in y..(y + height) {
    for px in x..(x + width) {
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
fn card_corner_pixels(
  capture: &Capture,
  state: &BalatroState,
  card: &CardSlot,
) -> (u32, u32, u32, u32) {
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
      let height = (cell_h as f32 * 0.35).ceil() as u32;
      let crop = image::imageops::crop_imm(&atlas, x, y, width, height).to_image();
      if let Some(mask) = normalized_foreground_mask(&crop) {
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
  image::open(deck_atlas_path)
    .ok()
    .map(|image| image.to_rgba8())
}

#[cfg(target_os = "macos")]
fn infer_rank_from_deck_template(
  corner: &RgbaImage,
  templates: Option<&[RankTemplate]>,
  suit: Option<&str>,
) -> Option<(String, f32)> {
  let width = (corner.width() as f32 * 0.45).ceil() as u32;
  let height = (corner.height() as f32 * 0.56).ceil() as u32;
  let observed_region =
    image::imageops::crop_imm(corner, 0, 0, width.max(1), height.max(1)).to_image();
  let observed = normalized_observed_rank_mask(&observed_region)?;
  templates?
    .iter()
    .filter(|template| suit.is_none_or(|suit| template.suit == suit))
    .map(|template| {
      let distance = mask_distance(&observed, &template.mask);
      (template.rank, 1.0 - distance)
    })
    .max_by(|left, right| {
      left
        .1
        .partial_cmp(&right.1)
        .unwrap_or(std::cmp::Ordering::Equal)
    })
    .and_then(|(rank, confidence)| {
      (confidence >= 0.75).then(|| (rank.to_string(), confidence.clamp(0.0, 1.0)))
    })
}

#[cfg(target_os = "macos")]
fn normalized_observed_rank_mask(image: &RgbaImage) -> Option<NormalizedMask> {
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

  // The card-corner crop contains rank above suit. OCR benefits from seeing
  // both, but template matching must isolate the rank glyph or a `9♥` can look
  // closer to another rank plus suit blob than to the rank alone.
  let bbox_h = max_y - min_y + 1;
  let rank_h = ((bbox_h as f32 * 0.45).ceil() as u32).max(1);
  let crop = image::imageops::crop_imm(image, min_x, min_y, max_x - min_x + 1, rank_h).to_image();
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
      pixels[ty * MASK_W + tx] = is_card_glyph_pixel(
        image
          .get_pixel(sx.min(image.width() - 1), sy.min(image.height() - 1))
          .0,
      );
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
fn mask_distance(left: &NormalizedMask, right: &NormalizedMask) -> f32 {
  let different = left
    .pixels
    .iter()
    .zip(&right.pixels)
    .filter(|(left, right)| left != right)
    .count();
  different as f32 / left.pixels.len().max(1) as f32
}

fn parse_card_reading(
  raw_text: &str,
  suit: Option<&str>,
  confidence: Option<f32>,
) -> CardReadValue {
  let normalized = normalize_card_text(raw_text);
  let rank = extract_rank(&normalized);
  let suit = suit
    .map(str::to_string)
    .or_else(|| detect_suit(&normalized));
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
    suit,
    suit_symbol,
    short_code,
    confidence,
    valid,
  }
}

fn apply_inferred_rank(reading: &mut CardReadValue, rank: String, confidence: f32) {
  reading.rank = Some(rank);
  reading.confidence = Some(reading.confidence.unwrap_or(confidence).max(confidence));
  reading.short_code = reading
    .rank
    .as_ref()
    .zip(reading.suit.as_deref())
    .map(|(rank, suit)| {
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
  reading.valid = reading.rank.is_some() && reading.suit.is_some();
  reading.status = if reading.valid { "read" } else { "partial" };
}

fn should_hover_reread_card(reading: &CardReadValue) -> bool {
  !reading.valid
    || reading
      .confidence
      .is_some_and(|confidence| confidence < 0.85)
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
  .find_map(|(suit, patterns)| {
    patterns
      .iter()
      .any(|pattern| text.contains(pattern))
      .then(|| suit.to_string())
  })
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
    "A", "K", "Q", "J", "10", "9", "8", "7", "6", "5", "4", "3", "2", "红桃", "方片", "方块",
    "黑桃", "梅花", "Hearts", "Diamonds", "Spades",
  ]
}

fn window_point_from_button(
  state: &BalatroState,
  window: &auv_driver::window::Window,
  button: &ButtonTarget,
) -> Point {
  window_point_from_frame_point(state, window, bbox_center_point(button.bbox))
}

fn window_point_from_store_item(
  state: &BalatroState,
  window: &auv_driver::window::Window,
  item: &StoreItem,
) -> Point {
  window_point_from_frame_point(state, window, bbox_center_point(item.bbox))
}

fn window_point_from_joker(
  state: &BalatroState,
  window: &auv_driver::window::Window,
  joker: &JokerSlot,
) -> Point {
  window_point_from_frame_point(state, window, bbox_center_point(joker.bbox))
}

fn window_point_from_consumable(
  state: &BalatroState,
  window: &auv_driver::window::Window,
  consumable: &ConsumableSlot,
) -> Point {
  window_point_from_frame_point(state, window, bbox_center_point(consumable.bbox))
}

fn bbox_center_point(bbox: auv_inference_common::BoundingBox) -> Point {
  Point::new(
    f64::from((bbox.x1 + bbox.x2) / 2.0),
    f64::from((bbox.y1 + bbox.y2) / 2.0),
  )
}

fn window_point_from_frame_point(
  state: &BalatroState,
  window: &auv_driver::window::Window,
  point: Point,
) -> Point {
  let width = f64::from(state.frame.image_size.width).max(1.0);
  let height = f64::from(state.frame.image_size.height).max(1.0);
  Point::new(
    point.x / width * window.frame.size.width,
    point.y / height * window.frame.size.height,
  )
}

fn normalized_window_point(window: &auv_driver::window::Window, x: f64, y: f64) -> Point {
  Point::new(x * window.frame.size.width, y * window.frame.size.height)
}

fn window_point_from_hand_card(
  state: &BalatroState,
  window: &auv_driver::window::Window,
  card: &CardSlot,
) -> Point {
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

fn verify_card_operation(operation: &str, before: &BalatroState, after: &BalatroState) -> bool {
  match operation {
    "cards.select" => before.phase == BalatroPhase::Playing && after.phase == BalatroPhase::Playing,
    "cards.play" | "cards.discard" => {
      before.phase == BalatroPhase::Playing
        && (after.phase != BalatroPhase::Playing
          || after.hand.len() != before.hand.len()
          || hand_fingerprints_changed(before, after))
    }
    _ => false,
  }
}

fn card_operation_evidence(
  operation: &str,
  before: &BalatroState,
  after: &BalatroState,
) -> Vec<&'static str> {
  let mut evidence = Vec::new();
  if before.phase == BalatroPhase::Playing {
    evidence.push("before_phase_playing");
  }
  if after.phase != before.phase {
    evidence.push("phase_changed");
  }
  if after.hand.len() != before.hand.len() {
    evidence.push("hand_count_changed");
  }
  if matches!(operation, "cards.play" | "cards.discard") && hand_fingerprints_changed(before, after)
  {
    evidence.push("hand_fingerprints_changed");
  }
  evidence
}

fn hand_fingerprints_changed(before: &BalatroState, after: &BalatroState) -> bool {
  let before_fingerprints = hand_fingerprints(before);
  let after_fingerprints = hand_fingerprints(after);
  !before_fingerprints.is_empty()
    && !after_fingerprints.is_empty()
    && before_fingerprints != after_fingerprints
}

fn hand_fingerprints(state: &BalatroState) -> Vec<&str> {
  state
    .hand
    .iter()
    .filter_map(|card| card.cache.visual_fingerprint.as_deref())
    .collect()
}

fn verify_store_buy(before: &BalatroState, after: &BalatroState) -> bool {
  after.store.items.len() < before.store.items.len()
    || after.jokers.len() > before.jokers.len()
    || after.consumables.len() > before.consumables.len()
    || after.phase != before.phase
}

fn store_buy_evidence(before: &BalatroState, after: &BalatroState) -> Vec<&'static str> {
  let mut evidence = Vec::new();
  if after.store.items.len() < before.store.items.len() {
    evidence.push("store_item_count_decreased");
  }
  if after.jokers.len() > before.jokers.len() {
    evidence.push("joker_count_increased");
  }
  if after.consumables.len() > before.consumables.len() {
    evidence.push("consumable_count_increased");
  }
  if after.phase != before.phase {
    evidence.push("phase_changed");
  }
  evidence
}

fn verify_sell_operation(
  zone: ObjectReadZone,
  before: &BalatroState,
  after: &BalatroState,
) -> bool {
  match zone {
    ObjectReadZone::Joker => {
      after.jokers.len() < before.jokers.len() || cash_changed(before, after)
    }
    ObjectReadZone::Consumable => {
      after.consumables.len() < before.consumables.len() || cash_changed(before, after)
    }
    ObjectReadZone::Store => false,
  }
}

fn sell_operation_evidence(
  zone: ObjectReadZone,
  before: &BalatroState,
  after: &BalatroState,
) -> Vec<&'static str> {
  let mut evidence = Vec::new();
  if zone == ObjectReadZone::Joker && after.jokers.len() < before.jokers.len() {
    evidence.push("joker_count_decreased");
  }
  if zone == ObjectReadZone::Consumable && after.consumables.len() < before.consumables.len() {
    evidence.push("consumable_count_decreased");
  }
  if cash_changed(before, after) {
    evidence.push("cash_changed");
  }
  evidence
}

fn cash_changed(before: &BalatroState, after: &BalatroState) -> bool {
  matches!(
    (&before.rounds.cash, &after.rounds.cash),
    (Some(before_cash), Some(after_cash)) if before_cash != after_cash
  )
}

fn verify_single_button_activation(
  button_id: &str,
  before: &BalatroState,
  after: &BalatroState,
) -> bool {
  match button_id {
    "button_cash_out" => {
      best_button(&before.buttons, "button_cash_out").is_some()
        && (best_button(&after.buttons, "button_cash_out").is_none()
          || after.phase == BalatroPhase::Store
          || after.store.is_store)
    }
    _ => {
      best_button(&before.buttons, button_id).is_some()
        && best_button(&after.buttons, button_id).is_none()
    }
  }
}

fn verify_consumable_use(before: &BalatroState, after: &BalatroState) -> bool {
  after.consumables.len() < before.consumables.len()
    || after.phase != before.phase
    || after.scores != before.scores
}

fn consumable_use_evidence(before: &BalatroState, after: &BalatroState) -> Vec<&'static str> {
  let mut evidence = Vec::new();
  if after.consumables.len() < before.consumables.len() {
    evidence.push("consumable_count_decreased");
  }
  if after.phase != before.phase {
    evidence.push("phase_changed");
  }
  if after.scores != before.scores {
    evidence.push("scores_changed");
  }
  evidence
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::model::{
    BALATRO_STATE_SCHEMA_VERSION, CacheHint, ConsumableKind, ConsumableSlot, FrameRef, JokerSlot,
    ObjectZone, Reading, RoundState, ScoreState, SlotId, StoreItem, StoreItemKind, StoreState,
  };
  use auv_inference_common::ImageSize;
  use auv_inference_common::{BoundingBox, Detection};

  fn button(id: &str, x1: f32, confidence: f32) -> ButtonTarget {
    ButtonTarget {
      id: id.to_string(),
      label: id.trim_start_matches("button_").to_string(),
      bbox: BoundingBox {
        x1,
        y1: 10.0,
        x2: x1 + 20.0,
        y2: 30.0,
      },
      confidence,
    }
  }

  #[test]
  fn restart_primary_button_accepts_main_menu_play() {
    let buttons = vec![button("button_main_menu_play", 100.0, 0.8)];

    let resolved = restart_primary_button(&buttons).expect("main menu play should start a run");

    assert_eq!(resolved.id, "button_main_menu_play");
  }

  #[test]
  fn setup_extracts_deck_atlas_to_cache_and_reuses_it() {
    let root = unique_temp_dir("balatro-setup-extract");
    let love_path = create_fake_love(&root);
    let cache_dir = root.join("cache");
    let args = SetupArgs {
      love: Some(love_path.clone()),
      app: None,
      cache_dir: Some(cache_dir.clone()),
      check: false,
      force: false,
      json: true,
    };

    let report = setup_balatro_assets(&args).unwrap();

    assert_eq!(report.status, SetupStatus::Extracted);
    assert_eq!(
      report.source_love_path.as_deref(),
      Some(love_path.as_path())
    );
    assert!(report.deck_atlas_path.exists());
    assert!(report.manifest_path.exists());
    image::open(&report.deck_atlas_path).expect("extracted deck atlas should be an image");
    let manifest = fs::read_to_string(&report.manifest_path).unwrap();
    assert!(manifest.contains(SETUP_MANIFEST_SCHEMA_VERSION));
    assert!(manifest.contains("deck_atlas_sha256"));

    let reused = setup_balatro_assets(&args).unwrap();

    assert_eq!(reused.status, SetupStatus::Reused);
    assert_eq!(reused.source_love_path, None);

    let checked = setup_balatro_assets(&SetupArgs {
      check: true,
      ..args
    })
    .unwrap();

    assert_eq!(checked.status, SetupStatus::Ready);
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn deck_atlas_can_load_from_setup_cache() {
    let root = unique_temp_dir("balatro-setup-cache-load");
    let cache_dir = root.join("cache");
    fs::create_dir_all(&cache_dir).unwrap();
    let deck_atlas_path = cache_dir.join(DECK_ATLAS_CACHE_FILE);
    RgbaImage::from_pixel(4, 3, image::Rgba([1, 2, 3, 255]))
      .save(&deck_atlas_path)
      .unwrap();

    let atlas = load_deck_atlas_from_setup_cache(&cache_dir).unwrap();

    assert_eq!(atlas.width(), 4);
    assert_eq!(atlas.height(), 3);
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn blind_select_resolves_slot_by_left_to_right_button_order() {
    let buttons = vec![
      button("button_level_select", 300.0, 0.96),
      button("button_level_select", 100.0, 0.94),
      button("button_level_select", 500.0, 0.98),
    ];

    let selected = select_button_for_slot(&buttons, "button_level_select", Some(1)).unwrap();

    assert_eq!(selected.bbox.x1, 300.0);
  }

  #[test]
  fn blind_skip_uses_highest_confidence_skip_button_without_slot() {
    let buttons = vec![
      button("button_level_skip", 200.0, 0.91),
      button("button_level_skip", 100.0, 0.97),
    ];

    let selected = select_button_for_slot(&buttons, "button_level_skip", None).unwrap();

    assert_eq!(selected.confidence, 0.97);
  }

  #[test]
  fn hand_slot_parser_accepts_comma_separated_hand_indices_only() {
    assert_eq!(
      parse_hand_slot_indices("hand:0,hand:2,hand:4").unwrap(),
      vec![0, 2, 4]
    );

    assert!(parse_hand_slot_indices("store:0").is_err());
    assert!(parse_hand_slot_indices("hand:x").is_err());
  }

  #[test]
  fn object_slot_parsers_accept_expected_zone_prefixes() {
    assert_eq!(parse_store_slot_index("store:0").unwrap(), 0);
    assert_eq!(parse_joker_slot_index("joker:1").unwrap(), 1);
    assert_eq!(parse_consumable_slot_index("consumable:2").unwrap(), 2);
    assert_eq!(parse_pack_slot_index("pack:3").unwrap(), 3);

    assert!(parse_store_slot_index("joker:0").is_err());
    assert!(parse_joker_slot_index("store:1").is_err());
    assert!(parse_consumable_slot_index("pack:2").is_err());
    assert!(parse_pack_slot_index("pack:x").is_err());
  }

  #[test]
  fn store_slot_selection_uses_observed_store_items() {
    let state = synthetic_store_state(vec![
      store_item(0, StoreItemKind::Joker, 500.0),
      store_item(1, StoreItemKind::Planet, 700.0),
    ]);

    let selected = select_store_item(&state, 1).unwrap();

    assert_eq!(selected.slot, SlotId::new(ObjectZone::Store, 1));
    assert_eq!(selected.kind, StoreItemKind::Planet);
    assert!(select_store_item(&state, 2).is_err());
  }

  #[test]
  fn consumable_slot_selection_uses_observed_consumables() {
    let mut state = synthetic_store_state(Vec::new());
    state.consumables = vec![consumable_item(0, 1000.0), consumable_item(1, 1200.0)];

    assert_eq!(select_consumable(&state, 1).unwrap().bbox.x1, 1200.0);
    assert!(select_consumable(&state, 2).is_err());
  }

  #[test]
  fn object_read_from_state_returns_static_evidence() {
    let mut state = synthetic_store_state(vec![store_item(0, StoreItemKind::Joker, 500.0)]);
    state.jokers = vec![joker_item(0, 700.0)];
    state.consumables = vec![consumable_item(0, 1000.0)];

    let store = object_read_from_state(&state, "store:0", ObjectReadZone::Store).unwrap();
    assert_eq!(store.slot.to_string(), "store:0");
    assert_eq!(store.kind, "joker");
    assert_eq!(store.reading.status, "unread");
    assert_eq!(store.reading.raw_text, None);
    assert_eq!(store.reading.confidence, None);
    assert_eq!(store.evidence.source, "observation_without_hover_ocr");
    assert!(store.evidence.hover_required);

    let joker = object_read_from_state(&state, "joker:0", ObjectReadZone::Joker).unwrap();
    assert_eq!(joker.slot.to_string(), "joker:0");
    assert_eq!(joker.kind, "joker");
    assert_eq!(joker.reading.status, "unread");
    assert_eq!(joker.reading.raw_text, None);
    assert_eq!(joker.reading.confidence, None);
    assert_eq!(joker.evidence.source, "observation_without_hover_ocr");
    assert!(joker.evidence.hover_required);

    let consumable =
      object_read_from_state(&state, "consumable:0", ObjectReadZone::Consumable).unwrap();
    assert_eq!(consumable.slot.to_string(), "consumable:0");
    assert_eq!(consumable.kind, "planet");
    assert_eq!(consumable.reading.status, "unread");
    assert_eq!(consumable.reading.raw_text, None);
    assert_eq!(consumable.reading.confidence, None);
    assert_eq!(consumable.evidence.source, "observation_without_hover_ocr");
    assert!(consumable.evidence.hover_required);
  }

  #[test]
  fn active_pack_choices_are_lower_row_cards() {
    let mut state = synthetic_store_state(Vec::new());
    state.phase = BalatroPhase::Unknown;
    state.raw_entities = vec![
      crate::model::ObjectEvidence {
        model: "entities-test".to_string(),
        detection: raw_detection("joker_card", 500.0, 70.0, 650.0, 260.0),
      },
      crate::model::ObjectEvidence {
        model: "entities-test".to_string(),
        detection: raw_detection("spectral_card", 860.0, 610.0, 1000.0, 800.0),
      },
      crate::model::ObjectEvidence {
        model: "entities-test".to_string(),
        detection: raw_detection("tarot_card", 540.0, 610.0, 680.0, 800.0),
      },
      crate::model::ObjectEvidence {
        model: "entities-test".to_string(),
        detection: raw_detection("joker_card", 1020.0, 610.0, 1160.0, 800.0),
      },
      crate::model::ObjectEvidence {
        model: "entities-test".to_string(),
        detection: raw_detection("planet_card", 700.0, 610.0, 840.0, 800.0),
      },
      crate::model::ObjectEvidence {
        model: "entities-test".to_string(),
        detection: raw_detection("poker_card_front", 380.0, 610.0, 520.0, 800.0),
      },
      crate::model::ObjectEvidence {
        model: "entities-test".to_string(),
        detection: raw_detection("poker_card_front", 460.0, 790.0, 600.0, 940.0),
      },
      crate::model::ObjectEvidence {
        model: "entities-test".to_string(),
        detection: raw_detection("poker_card_front", 1320.0, 610.0, 1480.0, 800.0),
      },
      crate::model::ObjectEvidence {
        model: "entities-test".to_string(),
        detection: raw_detection("poker_card_stack", 1380.0, 610.0, 1540.0, 800.0),
      },
    ];
    state
      .buttons
      .push(button("button_card_pack_skip", 1080.0, 0.95));

    let choices = active_pack_choices(&state);

    assert_eq!(choices.len(), 5);
    assert_eq!(
      choices
        .iter()
        .map(|choice| choice.kind.as_str())
        .collect::<Vec<_>>(),
      vec![
        "poker_card_front",
        "tarot_card",
        "planet_card",
        "spectral_card",
        "joker_card"
      ]
    );
    assert_eq!(
      choices
        .iter()
        .map(|choice| choice.slot_index)
        .collect::<Vec<_>>(),
      vec![0, 1, 2, 3, 4]
    );
  }

  #[test]
  fn pack_choice_hover_region_covers_center_tooltip_area() {
    assert_eq!(
      pack_choice_hover_ocr_region(),
      RatioRect::new(0.20, 0.02, 0.70, 0.72)
    );
  }

  #[test]
  fn pack_confirm_fallback_requires_active_pack_evidence() {
    let mut state = synthetic_store_state(Vec::new());
    state.phase = BalatroPhase::Unknown;
    let choice = PackChoice {
      slot_index: 0,
      kind: "tarot_card".to_string(),
      detector_label: "tarot_card".to_string(),
      hint: pack_choice_hint("tarot_card").to_string(),
      hover_required: true,
      hover_text: None,
      hover_frame: None,
      hover_ocr_region: None,
      hover_error: None,
      bbox: BoundingBox {
        x1: 540.0,
        y1: 610.0,
        x2: 680.0,
        y2: 800.0,
      },
      confidence: 0.9,
    };

    assert!(resolve_pack_confirm_target(&state, &choice).is_err());

    state.buttons.push(button("button_use", 760.0, 0.96));
    let use_button = resolve_pack_confirm_target(&state, &choice).unwrap();
    assert_eq!(use_button.source, ActionTargetSource::YoloButton);

    state.buttons.clear();
    state
      .buttons
      .push(button("button_card_pack_skip", 1080.0, 0.95));
    let fallback = resolve_pack_confirm_target(&state, &choice).unwrap();
    assert_eq!(fallback.source, ActionTargetSource::LayoutFallback);
    assert_eq!(
      fallback.fallback_reason.as_deref(),
      Some("pack_confirm_button_missing_visible_layout_match")
    );
  }

  #[test]
  fn consumable_use_target_prefers_yolo_button_then_layout_fallback() {
    let mut state = synthetic_store_state(Vec::new());
    state.consumables = vec![consumable_item(0, 1250.0)];
    state.buttons.push(button("button_use", 1420.0, 0.96));
    let use_button = resolve_consumable_use_target(&state, 0).unwrap();
    assert_eq!(use_button.source, ActionTargetSource::YoloButton);

    state.buttons.clear();
    let fallback = resolve_consumable_use_target(&state, 0).unwrap();
    assert_eq!(fallback.source, ActionTargetSource::LayoutFallback);
    assert_eq!(
      fallback.fallback_reason.as_deref(),
      Some("consumable_use_button_missing_selected_card_layout_match")
    );
    assert!(fallback.frame_point.x > 1250.0);
  }

  #[test]
  fn store_next_round_target_prefers_yolo_button_then_layout_fallback() {
    let mut state = synthetic_store_state(Vec::new());
    state
      .buttons
      .push(button("button_store_next_round", 490.0, 0.98));
    let target = resolve_store_next_round_target(&state).unwrap();
    assert_eq!(target.source, ActionTargetSource::YoloButton);

    state.buttons.clear();
    state.store.is_store = true;
    state.store.can_next_round = false;
    let fallback = resolve_store_next_round_target(&state).unwrap();
    assert_eq!(fallback.source, ActionTargetSource::LayoutFallback);
    assert_eq!(
      fallback.fallback_reason.as_deref(),
      Some("yolo_button_missing_visible_layout_match")
    );

    state.buttons.push(button("button_purchase", 580.0, 0.96));
    assert!(resolve_store_next_round_target(&state).is_err());
  }

  #[test]
  fn store_next_round_target_falls_back_from_visible_store_pack_evidence() {
    let mut state = synthetic_store_state(Vec::new());
    state.phase = BalatroPhase::Unknown;
    state.store.is_store = false;
    state.raw_entities = vec![crate::model::ObjectEvidence {
      model: "entities-test".to_string(),
      detection: raw_detection("card_pack", 1010.0, 680.0, 1160.0, 915.0),
    }];

    let fallback = resolve_store_next_round_target(&state).unwrap();

    assert_eq!(fallback.source, ActionTargetSource::LayoutFallback);
    assert_eq!(
      fallback.fallback_reason.as_deref(),
      Some("yolo_button_missing_visible_layout_match")
    );
  }

  #[test]
  fn store_next_round_target_does_not_fallback_during_pack_choice() {
    let mut state = synthetic_store_state(Vec::new());
    state.phase = BalatroPhase::Unknown;
    state.store.is_store = false;
    state.raw_entities = vec![crate::model::ObjectEvidence {
      model: "entities-test".to_string(),
      detection: raw_detection("tarot_card", 540.0, 610.0, 680.0, 800.0),
    }];
    state
      .buttons
      .push(button("button_card_pack_skip", 1080.0, 0.95));

    assert!(resolve_store_next_round_target(&state).is_err());
  }

  #[test]
  fn store_next_round_target_falls_back_from_empty_store_shell() {
    let mut state = synthetic_store_state(Vec::new());
    state.phase = BalatroPhase::Unknown;
    state.store.is_store = false;
    state.buttons.push(button("button_run_info", 180.0, 0.95));
    state.buttons.push(button("button_options", 190.0, 0.95));

    let fallback = resolve_store_next_round_target(&state).unwrap();

    assert_eq!(fallback.source, ActionTargetSource::LayoutFallback);
    assert_eq!(
      fallback.fallback_reason.as_deref(),
      Some("yolo_button_missing_visible_layout_match")
    );
  }

  #[test]
  fn store_buy_confirm_accepts_purchase_or_use_button() {
    let mut selected = synthetic_store_state(Vec::new());
    selected.buttons.push(button("button_use", 700.0, 0.94));

    let use_button = select_store_buy_confirm_button(&selected).unwrap();
    assert_eq!(use_button.id, "button_use");

    selected
      .buttons
      .push(button("button_purchase", 500.0, 0.96));
    let purchase_button = select_store_buy_confirm_button(&selected).unwrap();
    assert_eq!(purchase_button.id, "button_purchase");
  }

  #[test]
  fn card_read_parser_combines_rank_text_with_inferred_suit() {
    let reading = parse_card_reading("10", Some("spades"), Some(0.99));

    assert_eq!(reading.rank.as_deref(), Some("10"));
    assert_eq!(reading.suit.as_deref(), Some("spades"));
    assert_eq!(reading.short_code.as_deref(), Some("10S"));
    assert_eq!(reading.confidence, Some(0.99));
    assert!(reading.valid);
  }

  #[test]
  fn card_hover_reread_policy_targets_partial_or_low_confidence_reads() {
    let high_confidence = parse_card_reading("Q", Some("spades"), Some(0.90));
    let low_confidence = parse_card_reading("Q", Some("spades"), Some(0.84));
    let partial = parse_card_reading("F", Some("spades"), Some(0.79));

    assert!(!should_hover_reread_card(&high_confidence));
    assert!(should_hover_reread_card(&low_confidence));
    assert!(should_hover_reread_card(&partial));
  }

  #[test]
  fn card_operation_verification_accepts_changed_hand_fingerprints() {
    let mut before = synthetic_store_state(Vec::new());
    before.phase = BalatroPhase::Playing;
    before.hand = vec![
      hand_card(0, "before-a"),
      hand_card(1, "before-b"),
      hand_card(2, "before-c"),
    ];

    let mut after = before.clone();
    after.hand = vec![
      hand_card(0, "after-a"),
      hand_card(1, "after-b"),
      hand_card(2, "after-c"),
    ];

    assert!(verify_card_operation("cards.play", &before, &after));
    assert!(verify_card_operation("cards.discard", &before, &after));
  }

  #[test]
  fn sell_operation_verification_accepts_joker_count_decrease() {
    let mut before = synthetic_store_state(Vec::new());
    before.jokers = vec![joker_item(0, 100.0), joker_item(1, 140.0)];
    let mut after = before.clone();
    after.jokers.pop();

    assert!(verify_sell_operation(
      ObjectReadZone::Joker,
      &before,
      &after
    ));
    assert_eq!(
      sell_operation_evidence(ObjectReadZone::Joker, &before, &after),
      vec!["joker_count_decreased"]
    );
  }

  #[test]
  fn sell_operation_verification_accepts_consumable_count_decrease() {
    let mut before = synthetic_store_state(Vec::new());
    before.consumables = vec![consumable_item(0, 100.0), consumable_item(1, 140.0)];
    let mut after = before.clone();
    after.consumables.pop();

    assert!(verify_sell_operation(
      ObjectReadZone::Consumable,
      &before,
      &after
    ));
    assert_eq!(
      sell_operation_evidence(ObjectReadZone::Consumable, &before, &after),
      vec!["consumable_count_decreased"]
    );
  }

  #[test]
  fn sell_operation_verification_accepts_cash_change_when_detection_is_noisy() {
    let mut before = synthetic_store_state(Vec::new());
    before.jokers = vec![joker_item(0, 100.0)];
    before.rounds.cash = Some("$7".to_string());
    let mut after = before.clone();
    after.rounds.cash = Some("$9".to_string());

    assert!(verify_sell_operation(
      ObjectReadZone::Joker,
      &before,
      &after
    ));
    assert_eq!(
      sell_operation_evidence(ObjectReadZone::Joker, &before, &after),
      vec!["cash_changed"]
    );
  }

  #[test]
  fn selected_hand_slot_indices_require_requested_cards_to_be_raised() {
    let mut selected = synthetic_store_state(Vec::new());
    selected.phase = BalatroPhase::Playing;
    selected.hand = vec![
      hand_card(0, "card-a"),
      hand_card(1, "card-b"),
      hand_card(2, "card-c"),
    ];
    selected.hand[0].bbox.y1 -= 24.0;
    selected.hand[0].bbox.y2 -= 24.0;
    selected.hand[2].bbox.y1 -= 24.0;
    selected.hand[2].bbox.y2 -= 24.0;
    selected.buttons.push(button("button_play", 600.0, 0.96));

    assert_eq!(
      selected_hand_slot_indices(&selected, &[0, 1, 2]),
      vec![0, 2]
    );
  }

  #[test]
  fn selected_hand_slot_indices_ignore_normal_hand_fan_variation() {
    let mut state = synthetic_store_state(Vec::new());
    state.phase = BalatroPhase::Playing;
    state.hand = vec![
      hand_card(0, "card-a"),
      hand_card(1, "card-b"),
      hand_card(2, "card-c"),
    ];
    state.hand[1].bbox.y1 -= 8.0;
    state.hand[1].bbox.y2 -= 8.0;

    assert!(selected_hand_slot_indices(&state, &[0, 1, 2]).is_empty());
  }

  #[test]
  fn selected_hand_slot_indices_require_play_or_discard_button_evidence() {
    let mut state = synthetic_store_state(Vec::new());
    state.phase = BalatroPhase::Playing;
    state.hand = vec![
      hand_card(0, "card-a"),
      hand_card(1, "card-b"),
      hand_card(2, "card-c"),
    ];
    state.hand[1].bbox.y1 -= 24.0;
    state.hand[1].bbox.y2 -= 24.0;

    assert!(selected_hand_slot_indices(&state, &[0, 1, 2]).is_empty());
  }

  #[test]
  fn selected_hand_slot_indices_accept_use_button_evidence_for_consumable_targets() {
    let mut state = synthetic_store_state(Vec::new());
    state.hand = vec![
      hand_card(0, "card-a"),
      hand_card(1, "card-b"),
      hand_card(2, "card-c"),
    ];
    state.hand[1].bbox.y1 -= 24.0;
    state.hand[1].bbox.y2 -= 24.0;
    state.buttons.push(button("button_use", 600.0, 0.96));

    assert_eq!(selected_hand_slot_indices(&state, &[0, 1, 2]), vec![1]);
  }

  #[test]
  fn parse_hand_target_indices_accepts_comma_split_hand_slots() {
    let targets = vec!["hand:1".to_string(), "hand:2".to_string()];

    assert_eq!(parse_hand_target_indices(&targets).unwrap(), vec![1, 2]);
  }

  #[test]
  fn parse_hand_target_indices_rejects_non_hand_targets() {
    let targets = vec!["joker:1".to_string()];
    let error = parse_hand_target_indices(&targets).unwrap_err();

    assert!(error.to_string().contains("hand:N"), "{error}");
  }

  #[test]
  fn hand_selection_matches_requested_rejects_extra_selected_slots() {
    let mut state = synthetic_store_state(Vec::new());
    state.phase = BalatroPhase::Playing;
    state.hand = vec![
      hand_card(0, "card-a"),
      hand_card(1, "card-b"),
      hand_card(2, "card-c"),
    ];
    state.hand[0].bbox.y1 -= 24.0;
    state.hand[0].bbox.y2 -= 24.0;
    state.hand[2].bbox.y1 -= 24.0;
    state.hand[2].bbox.y2 -= 24.0;
    state.buttons.push(button("button_play", 600.0, 0.96));

    assert!(!hand_selection_matches_requested(&state, &[0]));
    assert!(hand_selection_matches_requested(&state, &[0, 2]));
  }

  #[test]
  fn hand_selection_plan_keeps_requested_selected_cards() {
    let mut state = synthetic_store_state(Vec::new());
    state.phase = BalatroPhase::Playing;
    state.hand = vec![
      hand_card(0, "card-a"),
      hand_card(1, "card-b"),
      hand_card(2, "card-c"),
    ];
    state.hand[1].bbox.y1 -= 24.0;
    state.hand[1].bbox.y2 -= 24.0;
    state.hand[2].bbox.y1 -= 24.0;
    state.hand[2].bbox.y2 -= 24.0;
    state.buttons.push(button("button_play", 600.0, 0.96));

    assert_eq!(selected_slots_to_clear(&state, &[1]), vec![2]);
    assert_eq!(requested_slots_to_select(&state, &[0, 1]), vec![0]);
  }

  #[test]
  fn hand_card_click_frame_point_uses_visible_strip_between_neighbors() {
    let mut state = synthetic_store_state(Vec::new());
    state.phase = BalatroPhase::Playing;
    state.hand = vec![
      hand_card(0, "card-a"),
      hand_card(1, "card-b"),
      hand_card(2, "card-c"),
    ];
    let point = hand_card_click_frame_point(&state, &state.hand[1]);

    assert!(point.x > state.hand[0].bbox.x2 as f64);
    assert!(point.x < state.hand[2].bbox.x1 as f64);
    assert!((point.y - 120.8).abs() < 0.001);
  }

  #[test]
  fn hand_card_interactions_report_click_points_and_selection_state() {
    let mut state = synthetic_store_state(Vec::new());
    state.phase = BalatroPhase::Playing;
    state.hand = vec![
      hand_card(0, "card-a"),
      hand_card(1, "card-b"),
      hand_card(2, "card-c"),
    ];
    state.hand[1].bbox.y1 -= 24.0;
    state.hand[1].bbox.y2 -= 24.0;
    state.buttons.push(button("button_play", 600.0, 0.96));

    let interactions = hand_card_interactions(&state);

    assert_eq!(interactions.len(), 3);
    assert_eq!(interactions[1].slot.index, 1);
    assert!(interactions[1].selected);
    assert!(!interactions[0].selected);
    assert_eq!(
      interactions[1].visual_fingerprint.as_deref(),
      Some("card-b")
    );
    assert!(interactions[1].click_frame_point.x > state.hand[0].bbox.x2 as f64);
    assert!(interactions[1].click_frame_point.x < state.hand[2].bbox.x1 as f64);
  }

  #[test]
  fn operation_summary_strips_bbox_and_trace_details() {
    let mut payload = serde_json::json!({
      "operation": "cards.select",
      "target": "Balatro",
      "selected_cards": [
        {
          "slot": { "zone": "hand", "index": 0 },
          "bbox": { "x1": 1.0, "y1": 2.0, "x2": 3.0, "y2": 4.0 },
        }
      ],
      "click_targets": [
        {
          "slot": { "zone": "hand", "index": 0 },
          "bbox": { "x1": 1.0, "y1": 2.0, "x2": 3.0, "y2": 4.0 },
          "point": { "x": 12.0, "y": 34.0 },
        }
      ],
      "selection_evidence": {
        "requested_slots": [0],
        "selected_slots": [0],
        "passed": true
      },
      "verification": {
        "mode": "targeted",
        "passed": true,
        "after_image": "/tmp/after.png",
        "retry_click_point": { "x": 1.0, "y": 2.0 },
      }
    });

    strip_operation_details(&mut payload);

    assert_eq!(payload["operation"], "cards.select");
    assert_eq!(payload["verification"]["passed"], true);
    assert!(payload.get("selected_cards").is_none());
    assert!(payload.get("click_targets").is_none());
    assert!(payload.get("selection_evidence").is_none());
    assert!(payload["verification"].get("after_image").is_none());
    assert!(payload["verification"].get("retry_click_point").is_none());
  }

  #[test]
  fn operation_details_are_left_intact_when_not_stripped() {
    let payload = serde_json::json!({
      "operation": "cards.select",
      "selected_cards": [
        {
          "bbox": { "x1": 1.0, "y1": 2.0, "x2": 3.0, "y2": 4.0 },
        }
      ],
    });

    assert!(payload["selected_cards"][0].get("bbox").is_some());
  }

  #[test]
  fn ui_numeric_readings_populate_scores_and_rounds_by_label() {
    let mut scores = ScoreState::default();
    let mut rounds = RoundState::default();

    apply_ui_numeric_reading("ui_score_chips", " 1O ", &mut scores, &mut rounds);
    apply_ui_numeric_reading("ui_score_mult", " x 4 ", &mut scores, &mut rounds);
    apply_ui_numeric_reading("ui_score_round_score", " 280/ ", &mut scores, &mut rounds);
    apply_ui_numeric_reading("ui_score_target_score", " 300 ", &mut scores, &mut rounds);
    apply_ui_numeric_reading("ui_data_hands_left", " 31 ", &mut scores, &mut rounds);
    apply_ui_numeric_reading("ui_data_discards_left", " 2 ", &mut scores, &mut rounds);
    apply_ui_numeric_reading("ui_data_cash", " $7 ", &mut scores, &mut rounds);
    apply_ui_numeric_reading("ui_round_ante_current", " 1 ", &mut scores, &mut rounds);

    assert_eq!(scores.chips.as_deref(), Some("10"));
    assert_eq!(scores.mult.as_deref(), Some("x4"));
    assert_eq!(scores.round_score.as_deref(), Some("280/"));
    assert_eq!(scores.target_score.as_deref(), Some("300"));
    assert_eq!(rounds.hands_left.as_deref(), Some("3"));
    assert_eq!(rounds.discards_left.as_deref(), Some("2"));
    assert_eq!(rounds.cash.as_deref(), Some("$7"));
    assert_eq!(rounds.ante_current.as_deref(), Some("1"));
  }

  #[test]
  fn ui_numeric_readings_ignore_empty_normalized_text() {
    let mut scores = ScoreState::default();
    let mut rounds = RoundState::default();

    apply_ui_numeric_reading("ui_data_hands_left", "abc", &mut scores, &mut rounds);

    assert!(rounds.hands_left.is_none());
  }

  #[test]
  fn ui_digit_reader_segments_multiple_glyphs() {
    let image = synthetic_ui_digit_image("300");

    let reading =
      infer_ui_digit_text_from_image_with_foreground(&image, UiDigitForeground::Colored);

    assert_eq!(reading.as_deref(), Some("300"));
  }

  #[test]
  fn ui_digit_score_reading_formats_mult_label() {
    assert_eq!(
      ui_digit_text_for_label("ui_score_mult", "3").as_deref(),
      Some("x3")
    );
    assert_eq!(
      ui_digit_text_for_label("ui_score_target_score", "300").as_deref(),
      Some("300")
    );
  }

  #[test]
  fn ui_digit_score_reading_drops_round_score_chip_icon() {
    assert_eq!(
      ui_digit_text_for_label("ui_score_round_score", "00").as_deref(),
      Some("0")
    );
    assert_eq!(
      ui_digit_text_for_label("ui_score_round_score", "0300").as_deref(),
      Some("300")
    );
  }

  #[test]
  fn ui_digit_reader_matches_balatro_thick_one() {
    let mask = mask_from_rows([
      "####.", "####.", ".###.", ".###.", ".###.", "#####", "#####",
    ]);

    assert_eq!(infer_ui_digit_from_mask(&mask), Some(1));
  }

  #[test]
  fn white_ui_digit_reader_ignores_colored_score_background() {
    let mut image = RgbaImage::from_pixel(80, 56, image::Rgba([220, 70, 60, 255]));
    draw_synthetic_ui_digit(&mut image, '0', 20, image::Rgba([245, 245, 245, 255]));

    let reading = infer_ui_digit_text_from_image_with_foreground(&image, UiDigitForeground::White);

    assert_eq!(reading.as_deref(), Some("0"));
  }

  #[test]
  fn ui_digit_reader_ignores_score_punctuation_sized_glyphs() {
    let mut image = RgbaImage::from_pixel(240, 56, image::Rgba([20, 25, 24, 255]));
    let color = image::Rgba([240, 80, 60, 255]);
    draw_synthetic_ui_digit_scaled(&mut image, '1', 0, 8, color);
    draw_synthetic_ui_digit_scaled(&mut image, '4', 44, 8, color);
    draw_synthetic_ui_digit_scaled(&mut image, '4', 90, 5, color);
    draw_synthetic_ui_digit_scaled(&mut image, '0', 132, 8, color);
    draw_synthetic_ui_digit_scaled(&mut image, '4', 176, 8, color);

    let reading =
      infer_ui_digit_text_from_image_with_foreground(&image, UiDigitForeground::Colored);

    assert_eq!(reading.as_deref(), Some("1404"));
  }

  fn synthetic_ui_digit_image(text: &str) -> RgbaImage {
    let scale = 8;
    let gap = 4;
    let width = text.len() as u32 * UI_DIGIT_MASK_W as u32 * scale
      + text.len().saturating_sub(1) as u32 * gap;
    let height = UI_DIGIT_MASK_H as u32 * scale;
    let mut image = RgbaImage::from_pixel(width, height, image::Rgba([20, 25, 24, 255]));
    let mut cursor_x = 0;
    for character in text.chars() {
      draw_synthetic_ui_digit(
        &mut image,
        character,
        cursor_x,
        image::Rgba([240, 80, 60, 255]),
      );
      cursor_x += UI_DIGIT_MASK_W as u32 * scale + gap;
    }
    image
  }

  fn draw_synthetic_ui_digit(
    image: &mut RgbaImage,
    character: char,
    cursor_x: u32,
    color: image::Rgba<u8>,
  ) {
    draw_synthetic_ui_digit_scaled(image, character, cursor_x, 8, color);
  }

  fn draw_synthetic_ui_digit_scaled(
    image: &mut RgbaImage,
    character: char,
    cursor_x: u32,
    scale: u32,
    color: image::Rgba<u8>,
  ) {
    let digit = character.to_digit(10).unwrap() as u8;
    let template = UI_DIGIT_TEMPLATES
      .iter()
      .find(|template| template.digit == digit)
      .unwrap();
    for (row_index, row) in template.rows.iter().enumerate() {
      for (column_index, pixel) in row.chars().enumerate() {
        if pixel != '#' {
          continue;
        }
        for y in 0..scale {
          for x in 0..scale {
            image.put_pixel(
              cursor_x + column_index as u32 * scale + x,
              row_index as u32 * scale + y,
              color,
            );
          }
        }
      }
    }
  }

  fn mask_from_rows(rows: [&str; 7]) -> [bool; UI_DIGIT_MASK_CELLS] {
    let mut mask = [false; UI_DIGIT_MASK_CELLS];
    for (row_index, row) in rows.iter().enumerate() {
      for (column_index, character) in row.chars().enumerate() {
        mask[row_index * UI_DIGIT_MASK_W + column_index] = character == '#';
      }
    }
    mask
  }

  fn synthetic_store_state(items: Vec<StoreItem>) -> BalatroState {
    BalatroState {
      schema_version: BALATRO_STATE_SCHEMA_VERSION.to_owned(),
      frame: FrameRef {
        source: "synthetic-store.png".to_owned(),
        image_size: ImageSize {
          width: 1600,
          height: 960,
        },
      },
      phase: BalatroPhase::Store,
      scores: ScoreState::default(),
      rounds: RoundState::default(),
      hand: Vec::new(),
      jokers: Vec::new(),
      consumables: Vec::new(),
      store: StoreState {
        is_store: true,
        item_count: items.len() as u32,
        can_reroll: false,
        can_next_round: true,
        items,
      },
      buttons: Vec::new(),
      diagnostics: Vec::new(),
      raw_entities: Vec::new(),
      raw_ui: Vec::new(),
    }
  }

  fn store_item(index: u32, kind: StoreItemKind, x1: f32) -> StoreItem {
    StoreItem {
      slot: SlotId::new(ObjectZone::Store, index),
      kind,
      bbox: BoundingBox {
        x1,
        y1: 390.0,
        x2: x1 + 140.0,
        y2: 610.0,
      },
      confidence: 0.9,
      reading: Reading::unread(),
      cache: CacheHint::default(),
    }
  }

  fn consumable_item(index: u32, x1: f32) -> ConsumableSlot {
    ConsumableSlot {
      slot: SlotId::new(ObjectZone::Consumable, index),
      kind: ConsumableKind::Planet,
      bbox: BoundingBox {
        x1,
        y1: 10.0,
        x2: x1 + 20.0,
        y2: 40.0,
      },
      confidence: 0.9,
      reading: Reading::unread(),
      cache: CacheHint::default(),
    }
  }

  fn joker_item(index: u32, x1: f32) -> JokerSlot {
    JokerSlot {
      slot: SlotId::new(ObjectZone::Joker, index),
      bbox: BoundingBox {
        x1,
        y1: 10.0,
        x2: x1 + 20.0,
        y2: 40.0,
      },
      confidence: 0.9,
      reading: Reading::unread(),
      cache: CacheHint::default(),
    }
  }

  fn hand_card(index: u32, fingerprint: &str) -> CardSlot {
    CardSlot {
      slot: SlotId::new(ObjectZone::Hand, index),
      kind: "poker_card_front".to_string(),
      bbox: BoundingBox {
        x1: 100.0 + index as f32 * 20.0,
        y1: 100.0,
        x2: 120.0 + index as f32 * 20.0,
        y2: 140.0,
      },
      confidence: 0.9,
      reading: Reading::unread(),
      cache: CacheHint {
        needs_reading: true,
        visual_fingerprint: Some(fingerprint.to_string()),
        changed_since_last_read: true,
      },
    }
  }

  fn raw_detection(label: &str, x1: f32, y1: f32, x2: f32, y2: f32) -> Detection {
    Detection {
      class_id: 0,
      label: label.to_string(),
      confidence: 0.9,
      bbox: BoundingBox { x1, y1, x2, y2 },
    }
  }

  fn unique_temp_dir(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
      "auv-game-balatro-{label}-{}-{}",
      std::process::id(),
      now_millis()
    ));
    fs::create_dir_all(&path).unwrap();
    path
  }

  fn create_fake_love(root: &Path) -> PathBuf {
    let source_root = root.join("love-src");
    let atlas_dir = source_root.join("resources").join("textures").join("2x");
    fs::create_dir_all(&atlas_dir).unwrap();
    RgbaImage::from_pixel(4, 3, image::Rgba([64, 32, 16, 255]))
      .save(atlas_dir.join("8BitDeck.png"))
      .unwrap();
    let love_path = root.join("Balatro.love");
    let output = ProcessCommand::new("zip")
      .arg("-q")
      .arg("-r")
      .arg(&love_path)
      .arg("resources")
      .current_dir(&source_root)
      .output()
      .expect("zip should be available for setup extraction tests");
    assert!(
      output.status.success(),
      "failed to create fake love archive: {}",
      String::from_utf8_lossy(&output.stderr)
    );
    love_path
  }
}
