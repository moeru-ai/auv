use std::path::PathBuf;

use auv_cli_invoke::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, InvokeReport, InvokeReportField, arg::ArgSpec, invoke_command,
};
use auv_game_balatro::{
  BlindSelectConfirmation, BlindSelectRequest, BlindSelectResult, BlindSkipConfirmation, BlindSkipRequest, BlindSkipResult,
  CardCommitRequest, CardCommitResult, CardCommitState, CardDetectionEvalWitnessInputs, CardDetectionEvalWitnessOutput,
  CardDetectionQualityInputs, CardDetectionQualityOutput, CardDetectionSemanticValidationInputs, CardDetectionSemanticValidationOutput,
  CardDetectionSpatialQueryInputs, CardDetectionSpatialQueryOutput, CardsClearOutcome, CardsClearRequest, CardsClearResult,
  CardsSelectRequest, CardsSelectResult, CashOutConfirmation, CashOutConfirmationRequest, CashOutRequest, CashOutResult,
  ConsumableUseRequest, ConsumableUseResult, ConsumableUseState, GameRestartOutcome, GameRestartRequest, GameRestartResult,
  ObjectSellOutcome, ObjectSellRequest, ObjectSellResult, ObjectZone, PackChoiceId, PackChooseRequest, PackChooseResult, PackChooseState,
  PackSkipConfirmation, PackSkipRequest, PackSkipResult, SlotId, StoreBuyOutcome, StoreBuyRequest, StoreBuyResult,
  StoreNextRoundConfirmation, StoreNextRoundConfirmationRequest, StoreNextRoundRequest, StoreNextRoundResult,
  build_card_detection_eval_witness, build_card_detection_quality, query_card_detection_spatial, validate_card_detection_semantic,
};
use auv_runtime::model::AuvResult;
use auv_tracing::Context;

pub const BLIND_SELECT_COMMAND_ID: &str = "game.balatro.blind_select";
pub const BLIND_SKIP_COMMAND_ID: &str = "game.balatro.blind_skip";
pub const CARDS_CLEAR_COMMAND_ID: &str = "game.balatro.cards_clear";
pub const CARDS_DISCARD_COMMAND_ID: &str = "game.balatro.cards_discard";
pub const CARDS_PLAY_COMMAND_ID: &str = "game.balatro.cards_play";
pub const CARDS_SELECT_COMMAND_ID: &str = "game.balatro.cards_select";
pub const CASH_OUT_COMMAND_ID: &str = "game.balatro.cash_out";
pub const CONSUMABLE_USE_COMMAND_ID: &str = "game.balatro.consumable_use";
pub const GAME_RESTART_COMMAND_ID: &str = "game.balatro.game_restart";
pub const CONSUMABLE_SELL_COMMAND_ID: &str = "game.balatro.consumable_sell";
pub const JOKER_SELL_COMMAND_ID: &str = "game.balatro.joker_sell";
pub const PACK_CHOOSE_COMMAND_ID: &str = "game.balatro.pack_choose";
pub const PACK_SKIP_COMMAND_ID: &str = "game.balatro.pack_skip";
pub const STORE_BUY_COMMAND_ID: &str = "game.balatro.store_buy";
pub const STORE_NEXT_ROUND_COMMAND_ID: &str = "game.balatro.store_next_round";

const CONFIRMATION_ARG: ArgSpec = ArgSpec {
  flag: "--confirmation",
  value_name: "MODE",
  required: false,
  help: "Outcome confirmation policy: targeted, weak, or none (default targeted).",
};
const TIMEOUT_MS_ARG: ArgSpec = ArgSpec {
  flag: "--timeout_ms",
  value_name: "MILLIS",
  required: false,
  help: "Maximum wait for post-action state observation (default 1200).",
};
const BLIND_SLOT_ARG: ArgSpec = ArgSpec {
  flag: "--slot",
  value_name: "BLIND_SLOT",
  required: true,
  help: "Blind slot in blind:N form.",
};
const CONFIRM_STARTED_ARG: ArgSpec = ArgSpec {
  flag: "--confirm_started",
  value_name: "BOOL",
  required: false,
  help: "Confirm that play started after selecting the blind (default true).",
};
const BLIND_SELECT_ARGS: &[ArgSpec] = &[BLIND_SLOT_ARG, CONFIRM_STARTED_ARG, TIMEOUT_MS_ARG];
const BLIND_CONFIRM_EXIT_ARG: ArgSpec = ArgSpec {
  flag: "--confirm_exit",
  value_name: "BOOL",
  required: false,
  help: "Confirm that blind selection closed after skipping (default true).",
};
const BLIND_SKIP_ARGS: &[ArgSpec] = &[BLIND_CONFIRM_EXIT_ARG, TIMEOUT_MS_ARG];
const CASH_OUT_ARGS: &[ArgSpec] = &[CONFIRMATION_ARG, TIMEOUT_MS_ARG];
const CARDS_CLEAR_ARGS: &[ArgSpec] = &[TIMEOUT_MS_ARG];
const HAND_SLOTS_ARG: ArgSpec = ArgSpec {
  flag: "--slots",
  value_name: "HAND_SLOTS",
  required: true,
  help: "Comma-separated cards in hand:N form.",
};
const CONFIRM_CHANGE_ARG: ArgSpec = ArgSpec {
  flag: "--confirm_change",
  value_name: "BOOL",
  required: false,
  help: "Confirm an observed phase, hand count, or hand fingerprint change after submission (default true).",
};
const CARDS_SELECT_ARGS: &[ArgSpec] = &[HAND_SLOTS_ARG, TIMEOUT_MS_ARG];
const CARD_COMMIT_ARGS: &[ArgSpec] = &[HAND_SLOTS_ARG, CONFIRM_CHANGE_ARG, TIMEOUT_MS_ARG];
const RESTART_CONFIRM_STARTED_ARG: ArgSpec = ArgSpec {
  flag: "--confirm_started",
  value_name: "BOOL",
  required: false,
  help: "Confirm that the restarted game reached a playable phase (default true).",
};
const GAME_RESTART_ARGS: &[ArgSpec] = &[RESTART_CONFIRM_STARTED_ARG, TIMEOUT_MS_ARG];
const CONSUMABLE_SLOT_ARG: ArgSpec = ArgSpec {
  flag: "--slot",
  value_name: "CONSUMABLE_SLOT",
  required: true,
  help: "Consumable slot in consumable:N form.",
};
const HAND_TARGETS_ARG: ArgSpec = ArgSpec {
  flag: "--hand_targets",
  value_name: "HAND_SLOTS",
  required: false,
  help: "Comma-separated target cards in hand:N form.",
};
const CONFIRM_USE_ARG: ArgSpec = ArgSpec {
  flag: "--confirm_use",
  value_name: "BOOL",
  required: false,
  help: "Confirm an observed consumable removal or score change after submission (default true).",
};
const CONSUMABLE_USE_ARGS: &[ArgSpec] = &[
  CONSUMABLE_SLOT_ARG,
  HAND_TARGETS_ARG,
  CONFIRM_USE_ARG,
  TIMEOUT_MS_ARG,
];
const PACK_CHOICE_ARG: ArgSpec = ArgSpec {
  flag: "--choice",
  value_name: "PACK_CHOICE",
  required: true,
  help: "Pack choice in pack:N form.",
};
const CONFIRM_APPLIED_ARG: ArgSpec = ArgSpec {
  flag: "--confirm_applied",
  value_name: "BOOL",
  required: false,
  help: "Confirm that the choice count decreased or the pack interface closed (default true).",
};
const PACK_CHOOSE_ARGS: &[ArgSpec] = &[
  PACK_CHOICE_ARG,
  HAND_TARGETS_ARG,
  CONFIRM_APPLIED_ARG,
  TIMEOUT_MS_ARG,
];
const OBJECT_SLOT_ARG: ArgSpec = ArgSpec {
  flag: "--slot",
  value_name: "OBJECT_SLOT",
  required: true,
  help: "Sellable slot in joker:N or consumable:N form.",
};
const CONFIRM_SALE_ARG: ArgSpec = ArgSpec {
  flag: "--confirm_sale",
  value_name: "BOOL",
  required: false,
  help: "Confirm an observed object removal or cash change after submission (default true).",
};
const OBJECT_SELL_ARGS: &[ArgSpec] = &[OBJECT_SLOT_ARG, CONFIRM_SALE_ARG, TIMEOUT_MS_ARG];
const CONFIRM_EXIT_ARG: ArgSpec = ArgSpec {
  flag: "--confirm_exit",
  value_name: "BOOL",
  required: false,
  help: "Confirm that the pack interface closed after delivery (default true).",
};
const PACK_SKIP_ARGS: &[ArgSpec] = &[CONFIRM_EXIT_ARG, TIMEOUT_MS_ARG];
const STORE_SLOT_ARG: ArgSpec = ArgSpec {
  flag: "--slot",
  value_name: "STORE_SLOT",
  required: true,
  help: "Store slot in store:N form.",
};
const CONFIRM_PURCHASE_ARG: ArgSpec = ArgSpec {
  flag: "--confirm_purchase",
  value_name: "BOOL",
  required: false,
  help: "Confirm an observed purchase state change after submission (default true).",
};
const STORE_BUY_ARGS: &[ArgSpec] = &[STORE_SLOT_ARG, CONFIRM_PURCHASE_ARG, TIMEOUT_MS_ARG];
const STORE_NEXT_ROUND_ARGS: &[ArgSpec] = &[CONFIRMATION_ARG, TIMEOUT_MS_ARG];

pub fn group() -> CommandGroup {
  CommandGroup::new("balatro", "BALATRO")
    .command(blind_select_invoke_command())
    .command(blind_skip_invoke_command())
    .command(cards_clear_invoke_command())
    .command(cards_discard_invoke_command())
    .command(cards_play_invoke_command())
    .command(cards_select_invoke_command())
    .command(cash_out_invoke_command())
    .command(consumable_sell_invoke_command())
    .command(consumable_use_invoke_command())
    .command(game_restart_invoke_command())
    .command(joker_sell_invoke_command())
    .command(pack_choose_invoke_command())
    .command(pack_skip_invoke_command())
    .command(store_buy_invoke_command())
    .command(store_next_round_invoke_command())
}

fn parse_timeout_ms(inputs: &std::collections::BTreeMap<String, String>, default: u64) -> Result<u64, String> {
  let timeout_ms = match inputs.get("timeout_ms") {
    Some(raw) => raw.parse::<u64>().map_err(|_| format!("invalid --timeout_ms {raw:?}; expected an integer from 1 to 60000"))?,
    None => default,
  };
  if !(1..=60_000).contains(&timeout_ms) {
    return Err(format!("invalid --timeout_ms {timeout_ms}; expected an integer from 1 to 60000"));
  }
  Ok(timeout_ms)
}

#[invoke_command(
  id = "game.balatro.blind_select",
  group = "game",
  description = "Select a Balatro blind and return typed input delivery plus play-state confirmation.",
  args = BLIND_SELECT_ARGS,
)]
async fn blind_select(input: InvokeCommandInput) -> InvokeCommandResult {
  let request = parse_blind_select_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let result = execute_blind_select(request, input.cancellation).await?;
  blind_select_output(&result)
}

pub(crate) fn parse_blind_select_request(
  target: Option<&str>,
  inputs: &std::collections::BTreeMap<String, String>,
) -> Result<BlindSelectRequest, String> {
  for name in inputs.keys() {
    if !matches!(name.as_str(), "slot" | "confirm_started" | "timeout_ms") {
      return Err(format!("{BLIND_SELECT_COMMAND_ID} does not accept --{name}"));
    }
  }
  let slot = inputs.get("slot").ok_or_else(|| format!("{BLIND_SELECT_COMMAND_ID} requires --slot blind:N"))?;
  let index = slot
    .strip_prefix("blind:")
    .ok_or_else(|| format!("invalid --slot {slot:?}; expected blind:N"))?
    .parse::<u32>()
    .map_err(|_| format!("invalid --slot {slot:?}; expected blind:N"))?;
  let confirm_started = match inputs.get("confirm_started").map(String::as_str).unwrap_or("true") {
    "true" => true,
    "false" => false,
    other => return Err(format!("invalid --confirm_started {other:?}; expected true or false")),
  };
  let timeout_ms = parse_timeout_ms(inputs, 1200)?;
  Ok(BlindSelectRequest {
    target: target.unwrap_or("Balatro").to_string(),
    slot: SlotId::new(ObjectZone::Blind, index),
    confirm_started,
    timeout_ms,
  })
}

pub(crate) async fn execute_blind_select(
  request: BlindSelectRequest,
  cancellation: auv_cli_invoke::InvokeCancellation,
) -> Result<BlindSelectResult, String> {
  cancellation.check().map_err(|error| error.to_string())?;
  auv_game_balatro::blind_select(request).map_err(|error| error.to_string())
}

fn blind_select_output(result: &BlindSelectResult) -> InvokeCommandResult {
  let mut output = InvokeCommandOutput::from_result(result)?;
  output.report = Some(InvokeReport::new(
    vec![
      InvokeReportField::new("Target", &result.target),
      InvokeReportField::new("Slot", result.slot.to_string()),
      InvokeReportField::new("Delivery", format!("{:?}", result.delivery.selected_path)),
      InvokeReportField::new("Confirmation", blind_select_confirmation_label(&result.confirmation)),
    ],
    Vec::new(),
  ));
  Ok(output)
}

fn blind_select_confirmation_label(confirmation: &BlindSelectConfirmation) -> &'static str {
  match confirmation {
    BlindSelectConfirmation::NotRequested => "not requested",
    BlindSelectConfirmation::Started { .. } => "started",
    BlindSelectConfirmation::NotStarted { .. } => "not started",
  }
}

#[invoke_command(
  id = "game.balatro.blind_skip",
  group = "game",
  description = "Skip the current Balatro blind and return typed input delivery plus selection-exit confirmation.",
  args = BLIND_SKIP_ARGS,
)]
async fn blind_skip(input: InvokeCommandInput) -> InvokeCommandResult {
  let request = parse_blind_skip_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let result = execute_blind_skip(request, input.cancellation).await?;
  blind_skip_output(&result)
}

pub(crate) fn parse_blind_skip_request(
  target: Option<&str>,
  inputs: &std::collections::BTreeMap<String, String>,
) -> Result<BlindSkipRequest, String> {
  for name in inputs.keys() {
    if !matches!(name.as_str(), "confirm_exit" | "timeout_ms") {
      return Err(format!("{BLIND_SKIP_COMMAND_ID} does not accept --{name}"));
    }
  }
  let confirm_exit = match inputs.get("confirm_exit").map(String::as_str).unwrap_or("true") {
    "true" => true,
    "false" => false,
    other => return Err(format!("invalid --confirm_exit {other:?}; expected true or false")),
  };
  Ok(BlindSkipRequest {
    target: target.unwrap_or("Balatro").to_string(),
    confirm_exit,
    timeout_ms: parse_timeout_ms(inputs, 1200)?,
  })
}

pub(crate) async fn execute_blind_skip(
  request: BlindSkipRequest,
  cancellation: auv_cli_invoke::InvokeCancellation,
) -> Result<BlindSkipResult, String> {
  cancellation.check().map_err(|error| error.to_string())?;
  auv_game_balatro::blind_skip(request).map_err(|error| error.to_string())
}

fn blind_skip_output(result: &BlindSkipResult) -> InvokeCommandResult {
  let mut output = InvokeCommandOutput::from_result(result)?;
  output.report = Some(InvokeReport::new(
    vec![
      InvokeReportField::new("Target", &result.target),
      InvokeReportField::new("Delivery", format!("{:?}", result.delivery.selected_path)),
      InvokeReportField::new("Confirmation", blind_skip_confirmation_label(&result.confirmation)),
    ],
    Vec::new(),
  ));
  Ok(output)
}

fn blind_skip_confirmation_label(confirmation: &BlindSkipConfirmation) -> &'static str {
  match confirmation {
    BlindSkipConfirmation::NotRequested => "not requested",
    BlindSkipConfirmation::Exited { .. } => "exited",
    BlindSkipConfirmation::NotExited { .. } => "not exited",
  }
}

#[invoke_command(
  id = "game.balatro.cards_clear",
  group = "game",
  description = "Clear selected Balatro hand cards and return every typed input delivery plus the resulting selection state.",
  args = CARDS_CLEAR_ARGS,
)]
async fn cards_clear(input: InvokeCommandInput) -> InvokeCommandResult {
  let request = parse_cards_clear_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let result = execute_cards_clear(request, input.cancellation).await?;
  cards_clear_output(&result)
}

pub(crate) fn parse_cards_clear_request(
  target: Option<&str>,
  inputs: &std::collections::BTreeMap<String, String>,
) -> Result<CardsClearRequest, String> {
  for name in inputs.keys() {
    if name != "timeout_ms" {
      return Err(format!("{CARDS_CLEAR_COMMAND_ID} does not accept --{name}"));
    }
  }
  let timeout_ms = match inputs.get("timeout_ms") {
    Some(raw) => raw.parse::<u64>().map_err(|_| format!("invalid --timeout_ms {raw:?}; expected an integer from 1 to 60000"))?,
    None => 1500,
  };
  if !(1..=60_000).contains(&timeout_ms) {
    return Err(format!("invalid --timeout_ms {timeout_ms}; expected an integer from 1 to 60000"));
  }
  Ok(CardsClearRequest {
    target: target.unwrap_or("Balatro").to_string(),
    timeout_ms,
  })
}

pub(crate) async fn execute_cards_clear(
  request: CardsClearRequest,
  cancellation: auv_cli_invoke::InvokeCancellation,
) -> Result<CardsClearResult, String> {
  cancellation.check().map_err(|error| error.to_string())?;
  auv_game_balatro::cards_clear(request).map_err(|error| error.to_string())
}

fn cards_clear_output(result: &CardsClearResult) -> InvokeCommandResult {
  let mut output = InvokeCommandOutput::from_result(result)?;
  output.report = Some(InvokeReport::new(
    vec![
      InvokeReportField::new("Target", &result.target),
      InvokeReportField::new("Toggles", result.toggles.len().to_string()),
      InvokeReportField::new("Outcome", cards_clear_outcome_label(&result.outcome)),
    ],
    Vec::new(),
  ));
  Ok(output)
}

fn cards_clear_outcome_label(outcome: &CardsClearOutcome) -> &'static str {
  match outcome {
    CardsClearOutcome::Cleared => "cleared",
    CardsClearOutcome::RemainingSelected { .. } => "remaining selected",
    CardsClearOutcome::Incomplete { .. } => "incomplete",
  }
}

#[invoke_command(
  id = "game.balatro.cards_select",
  group = "game",
  description = "Select Balatro hand cards and return every typed toggle plus the final selection state.",
  args = CARDS_SELECT_ARGS,
)]
async fn cards_select(input: InvokeCommandInput) -> InvokeCommandResult {
  let request = parse_cards_select_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let result = execute_cards_select(request, input.cancellation).await?;
  cards_select_output(&result)
}

#[invoke_command(
  id = "game.balatro.cards_play",
  group = "game",
  description = "Select and play Balatro hand cards while preserving every typed input action and resulting hand state.",
  args = CARD_COMMIT_ARGS,
)]
async fn cards_play(input: InvokeCommandInput) -> InvokeCommandResult {
  let request = parse_card_commit_request(CARDS_PLAY_COMMAND_ID, input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let result = execute_cards_play(request, input.cancellation).await?;
  card_commit_output(&result)
}

#[invoke_command(
  id = "game.balatro.cards_discard",
  group = "game",
  description = "Select and discard Balatro hand cards while preserving every typed input action and resulting hand state.",
  args = CARD_COMMIT_ARGS,
)]
async fn cards_discard(input: InvokeCommandInput) -> InvokeCommandResult {
  let request = parse_card_commit_request(CARDS_DISCARD_COMMAND_ID, input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let result = execute_cards_discard(request, input.cancellation).await?;
  card_commit_output(&result)
}

pub(crate) fn parse_cards_select_request(
  target: Option<&str>,
  inputs: &std::collections::BTreeMap<String, String>,
) -> Result<CardsSelectRequest, String> {
  for name in inputs.keys() {
    if !matches!(name.as_str(), "slots" | "timeout_ms") {
      return Err(format!("{CARDS_SELECT_COMMAND_ID} does not accept --{name}"));
    }
  }
  Ok(CardsSelectRequest {
    target: target.unwrap_or("Balatro").to_string(),
    slots: parse_required_hand_slots(CARDS_SELECT_COMMAND_ID, inputs.get("slots"))?,
    timeout_ms: parse_timeout_ms(inputs, 1500)?,
  })
}

pub(crate) fn parse_card_commit_request(
  command_id: &str,
  target: Option<&str>,
  inputs: &std::collections::BTreeMap<String, String>,
) -> Result<CardCommitRequest, String> {
  for name in inputs.keys() {
    if !matches!(name.as_str(), "slots" | "confirm_change" | "timeout_ms") {
      return Err(format!("{command_id} does not accept --{name}"));
    }
  }
  let confirm_change = match inputs.get("confirm_change").map(String::as_str).unwrap_or("true") {
    "true" => true,
    "false" => false,
    other => return Err(format!("invalid --confirm_change {other:?}; expected true or false")),
  };
  Ok(CardCommitRequest {
    target: target.unwrap_or("Balatro").to_string(),
    slots: parse_required_hand_slots(command_id, inputs.get("slots"))?,
    confirm_change,
    timeout_ms: parse_timeout_ms(inputs, 1500)?,
  })
}

fn parse_required_hand_slots(command_id: &str, raw: Option<&String>) -> Result<Vec<SlotId>, String> {
  let raw = raw.ok_or_else(|| format!("{command_id} requires --slots hand:N[,hand:N...]"))?;
  let slots = parse_hand_slot_list(raw, "--slots")?;
  if slots.is_empty() {
    return Err(format!("{command_id} requires at least one --slots entry"));
  }
  if let Some(slot) = slots.iter().enumerate().find_map(|(index, slot)| slots[..index].contains(slot).then_some(slot)) {
    return Err(format!("{command_id} received duplicate slot {slot}"));
  }
  Ok(slots)
}

fn parse_hand_slot_list(raw: &str, flag: &str) -> Result<Vec<SlotId>, String> {
  raw
    .split(',')
    .filter(|slot| !slot.trim().is_empty())
    .map(|slot| {
      let slot = slot.trim();
      let index = slot
        .strip_prefix("hand:")
        .ok_or_else(|| format!("invalid {flag} entry {slot:?}; expected hand:N"))?
        .parse::<u32>()
        .map_err(|_| format!("invalid {flag} entry {slot:?}; expected hand:N"))?;
      Ok(SlotId::new(ObjectZone::Hand, index))
    })
    .collect()
}

pub(crate) async fn execute_cards_select(
  request: CardsSelectRequest,
  cancellation: auv_cli_invoke::InvokeCancellation,
) -> Result<CardsSelectResult, String> {
  cancellation.check().map_err(|error| error.to_string())?;
  auv_game_balatro::cards_select(request).map_err(|error| error.to_string())
}

pub(crate) async fn execute_cards_play(
  request: CardCommitRequest,
  cancellation: auv_cli_invoke::InvokeCancellation,
) -> Result<CardCommitResult, String> {
  cancellation.check().map_err(|error| error.to_string())?;
  auv_game_balatro::cards_play(request).map_err(|error| error.to_string())
}

pub(crate) async fn execute_cards_discard(
  request: CardCommitRequest,
  cancellation: auv_cli_invoke::InvokeCancellation,
) -> Result<CardCommitResult, String> {
  cancellation.check().map_err(|error| error.to_string())?;
  auv_game_balatro::cards_discard(request).map_err(|error| error.to_string())
}

fn cards_select_output(result: &CardsSelectResult) -> InvokeCommandResult {
  let mut output = InvokeCommandOutput::from_result(result)?;
  output.report = Some(InvokeReport::new(
    vec![
      InvokeReportField::new("Target", &result.target),
      InvokeReportField::new("Requested", result.selection.requested.len().to_string()),
      InvokeReportField::new("Toggles", result.selection.toggles.len().to_string()),
      InvokeReportField::new("State", format!("{:?}", result.selection.state)),
    ],
    Vec::new(),
  ));
  Ok(output)
}

fn card_commit_output(result: &CardCommitResult) -> InvokeCommandResult {
  let mut output = InvokeCommandOutput::from_result(result)?;
  output.report = Some(InvokeReport::new(
    vec![
      InvokeReportField::new("Target", &result.target),
      InvokeReportField::new("Operation", format!("{:?}", result.kind)),
      InvokeReportField::new("Actions", result.actions.len().to_string()),
      InvokeReportField::new("State", card_commit_state_label(&result.state)),
    ],
    Vec::new(),
  ));
  Ok(output)
}

fn card_commit_state_label(state: &CardCommitState) -> &'static str {
  match state {
    CardCommitState::Stopped { .. } => "stopped",
    CardCommitState::Submitted { .. } => "submitted",
  }
}

#[invoke_command(
  id = "game.balatro.cash_out",
  group = "game",
  description = "Activate Balatro cash-out and return typed delivery plus domain-specific outcome confirmation.",
  args = CASH_OUT_ARGS,
)]
async fn cash_out(input: InvokeCommandInput) -> InvokeCommandResult {
  let request = parse_cash_out_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let result = execute_cash_out(request, input.cancellation).await?;
  cash_out_output(&result)
}

pub(crate) fn parse_cash_out_request(
  target: Option<&str>,
  inputs: &std::collections::BTreeMap<String, String>,
) -> Result<CashOutRequest, String> {
  for name in inputs.keys() {
    if !matches!(name.as_str(), "confirmation" | "timeout_ms") {
      return Err(format!("{CASH_OUT_COMMAND_ID} does not accept --{name}"));
    }
  }
  let confirmation = match inputs.get("confirmation").map(String::as_str).unwrap_or("targeted") {
    "none" => CashOutConfirmationRequest::None,
    "targeted" => CashOutConfirmationRequest::Targeted,
    "weak" => CashOutConfirmationRequest::Weak,
    other => return Err(format!("invalid --confirmation {other:?}; expected targeted, weak, or none")),
  };
  let timeout_ms = match inputs.get("timeout_ms") {
    Some(raw) => raw.parse::<u64>().map_err(|_| format!("invalid --timeout_ms {raw:?}; expected an integer from 1 to 60000"))?,
    None => 1200,
  };
  if !(1..=60_000).contains(&timeout_ms) {
    return Err(format!("invalid --timeout_ms {timeout_ms}; expected an integer from 1 to 60000"));
  }
  Ok(CashOutRequest {
    target: target.unwrap_or("Balatro").to_string(),
    confirmation,
    timeout_ms,
  })
}

pub(crate) async fn execute_cash_out(
  request: CashOutRequest,
  cancellation: auv_cli_invoke::InvokeCancellation,
) -> Result<CashOutResult, String> {
  // A cancellation observed after delivery cannot turn an already-clicked
  // operation into a retryable frontend failure.
  cancellation.check().map_err(|error| error.to_string())?;
  auv_game_balatro::cash_out(request).map_err(|error| error.to_string())
}

fn cash_out_output(result: &CashOutResult) -> InvokeCommandResult {
  let mut output = InvokeCommandOutput::from_result(result)?;
  output.report = Some(InvokeReport::new(
    vec![
      InvokeReportField::new("Target", &result.target),
      InvokeReportField::new("Delivery", format!("{:?}", result.delivery.selected_path)),
      InvokeReportField::new("Confirmation", cash_out_confirmation_label(&result.confirmation)),
    ],
    Vec::new(),
  ));
  Ok(output)
}

fn cash_out_confirmation_label(confirmation: &CashOutConfirmation) -> &'static str {
  match confirmation {
    CashOutConfirmation::NotRequested => "not requested",
    CashOutConfirmation::Confirmed { .. } => "confirmed",
    CashOutConfirmation::NotConfirmed { .. } => "not confirmed",
  }
}

#[invoke_command(
  id = "game.balatro.consumable_use",
  group = "game",
  description = "Use a Balatro consumable and return each typed input action plus the operation state.",
  args = CONSUMABLE_USE_ARGS,
)]
async fn consumable_use(input: InvokeCommandInput) -> InvokeCommandResult {
  let request = parse_consumable_use_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let result = execute_consumable_use(request, input.cancellation).await?;
  consumable_use_output(&result)
}

pub(crate) fn parse_consumable_use_request(
  target: Option<&str>,
  inputs: &std::collections::BTreeMap<String, String>,
) -> Result<ConsumableUseRequest, String> {
  for name in inputs.keys() {
    if !matches!(name.as_str(), "slot" | "hand_targets" | "confirm_use" | "timeout_ms") {
      return Err(format!("{CONSUMABLE_USE_COMMAND_ID} does not accept --{name}"));
    }
  }
  let slot = inputs.get("slot").ok_or_else(|| format!("{CONSUMABLE_USE_COMMAND_ID} requires --slot consumable:N"))?;
  let index = slot
    .strip_prefix("consumable:")
    .ok_or_else(|| format!("invalid --slot {slot:?}; expected consumable:N"))?
    .parse::<u32>()
    .map_err(|_| format!("invalid --slot {slot:?}; expected consumable:N"))?;
  let hand_targets = parse_hand_targets(inputs.get("hand_targets"))?;
  let confirm_use = match inputs.get("confirm_use").map(String::as_str).unwrap_or("true") {
    "true" => true,
    "false" => false,
    other => return Err(format!("invalid --confirm_use {other:?}; expected true or false")),
  };
  Ok(ConsumableUseRequest {
    target: target.unwrap_or("Balatro").to_string(),
    slot: SlotId::new(ObjectZone::Consumable, index),
    hand_targets,
    confirm_use,
    timeout_ms: parse_timeout_ms(inputs, 1200)?,
  })
}

fn parse_hand_targets(raw: Option<&String>) -> Result<Vec<SlotId>, String> {
  let Some(raw) = raw.filter(|raw| !raw.trim().is_empty()) else {
    return Ok(Vec::new());
  };
  let slots = parse_hand_slot_list(raw, "--hand_targets")?;
  if let Some(slot) = slots.iter().enumerate().find_map(|(index, slot)| slots[..index].contains(slot).then_some(slot)) {
    return Err(format!("duplicate --hand_targets entry {slot}"));
  }
  Ok(slots)
}

pub(crate) async fn execute_consumable_use(
  request: ConsumableUseRequest,
  cancellation: auv_cli_invoke::InvokeCancellation,
) -> Result<ConsumableUseResult, String> {
  cancellation.check().map_err(|error| error.to_string())?;
  auv_game_balatro::consumable_use(request).map_err(|error| error.to_string())
}

fn consumable_use_output(result: &ConsumableUseResult) -> InvokeCommandResult {
  let mut output = InvokeCommandOutput::from_result(result)?;
  output.report = Some(InvokeReport::new(
    vec![
      InvokeReportField::new("Target", &result.target),
      InvokeReportField::new("Slot", result.slot.to_string()),
      InvokeReportField::new("Actions", result.actions.len().to_string()),
      InvokeReportField::new("State", consumable_use_state_label(&result.state)),
    ],
    Vec::new(),
  ));
  Ok(output)
}

fn consumable_use_state_label(state: &ConsumableUseState) -> &'static str {
  match state {
    ConsumableUseState::Stopped { .. } => "stopped",
    ConsumableUseState::Submitted { .. } => "submitted",
  }
}

#[invoke_command(
  id = "game.balatro.joker_sell",
  group = "game",
  description = "Select and sell a Balatro joker while preserving both typed input deliveries and sale state.",
  args = OBJECT_SELL_ARGS,
)]
async fn joker_sell(input: InvokeCommandInput) -> InvokeCommandResult {
  let request = parse_joker_sell_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let result = execute_object_sell(request, input.cancellation).await?;
  object_sell_output(&result)
}

#[invoke_command(
  id = "game.balatro.consumable_sell",
  group = "game",
  description = "Select and sell a Balatro consumable while preserving both typed input deliveries and sale state.",
  args = OBJECT_SELL_ARGS,
)]
async fn consumable_sell(input: InvokeCommandInput) -> InvokeCommandResult {
  let request = parse_consumable_sell_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let result = execute_object_sell(request, input.cancellation).await?;
  object_sell_output(&result)
}

pub(crate) fn parse_joker_sell_request(
  target: Option<&str>,
  inputs: &std::collections::BTreeMap<String, String>,
) -> Result<ObjectSellRequest, String> {
  parse_object_sell_request(JOKER_SELL_COMMAND_ID, ObjectZone::Joker, "joker:", target, inputs)
}

pub(crate) fn parse_consumable_sell_request(
  target: Option<&str>,
  inputs: &std::collections::BTreeMap<String, String>,
) -> Result<ObjectSellRequest, String> {
  parse_object_sell_request(CONSUMABLE_SELL_COMMAND_ID, ObjectZone::Consumable, "consumable:", target, inputs)
}

fn parse_object_sell_request(
  command_id: &str,
  zone: ObjectZone,
  slot_prefix: &str,
  target: Option<&str>,
  inputs: &std::collections::BTreeMap<String, String>,
) -> Result<ObjectSellRequest, String> {
  for name in inputs.keys() {
    if !matches!(name.as_str(), "slot" | "confirm_sale" | "timeout_ms") {
      return Err(format!("{command_id} does not accept --{name}"));
    }
  }
  let slot = inputs.get("slot").ok_or_else(|| format!("{command_id} requires --slot {slot_prefix}N"))?;
  let index = slot
    .strip_prefix(slot_prefix)
    .ok_or_else(|| format!("invalid --slot {slot:?}; expected {slot_prefix}N"))?
    .parse::<u32>()
    .map_err(|_| format!("invalid --slot {slot:?}; expected {slot_prefix}N"))?;
  let confirm_sale = match inputs.get("confirm_sale").map(String::as_str).unwrap_or("true") {
    "true" => true,
    "false" => false,
    other => return Err(format!("invalid --confirm_sale {other:?}; expected true or false")),
  };
  Ok(ObjectSellRequest {
    target: target.unwrap_or("Balatro").to_string(),
    slot: SlotId::new(zone, index),
    confirm_sale,
    timeout_ms: parse_timeout_ms(inputs, 1000)?,
  })
}

pub(crate) async fn execute_object_sell(
  request: ObjectSellRequest,
  cancellation: auv_cli_invoke::InvokeCancellation,
) -> Result<ObjectSellResult, String> {
  cancellation.check().map_err(|error| error.to_string())?;
  auv_game_balatro::object_sell(request).map_err(|error| error.to_string())
}

fn object_sell_output(result: &ObjectSellResult) -> InvokeCommandResult {
  let mut output = InvokeCommandOutput::from_result(result)?;
  output.report = Some(InvokeReport::new(
    vec![
      InvokeReportField::new("Target", &result.target),
      InvokeReportField::new("Slot", result.slot.to_string()),
      InvokeReportField::new("Outcome", object_sell_outcome_label(&result.outcome)),
    ],
    Vec::new(),
  ));
  Ok(output)
}

fn object_sell_outcome_label(outcome: &ObjectSellOutcome) -> &'static str {
  match outcome {
    ObjectSellOutcome::SelectionOnly { .. } => "selection only",
    ObjectSellOutcome::Submitted { .. } => "submitted",
  }
}

#[invoke_command(
  id = "game.balatro.game_restart",
  group = "game",
  description = "Restart Balatro and return every typed click delivery plus the observed resulting game state.",
  args = GAME_RESTART_ARGS,
)]
async fn game_restart(input: InvokeCommandInput) -> InvokeCommandResult {
  let request = parse_game_restart_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let result = execute_game_restart(request, input.cancellation).await?;
  game_restart_output(&result)
}

pub(crate) fn parse_game_restart_request(
  target: Option<&str>,
  inputs: &std::collections::BTreeMap<String, String>,
) -> Result<GameRestartRequest, String> {
  for name in inputs.keys() {
    if !matches!(name.as_str(), "confirm_started" | "timeout_ms") {
      return Err(format!("{GAME_RESTART_COMMAND_ID} does not accept --{name}"));
    }
  }
  let confirm_started = match inputs.get("confirm_started").map(String::as_str).unwrap_or("true") {
    "true" => true,
    "false" => false,
    other => return Err(format!("invalid --confirm_started {other:?}; expected true or false")),
  };
  Ok(GameRestartRequest {
    target: target.unwrap_or("Balatro").to_string(),
    confirm_started,
    timeout_ms: parse_timeout_ms(inputs, 1800)?,
  })
}

pub(crate) async fn execute_game_restart(
  request: GameRestartRequest,
  cancellation: auv_cli_invoke::InvokeCancellation,
) -> Result<GameRestartResult, String> {
  cancellation.check().map_err(|error| error.to_string())?;
  auv_game_balatro::game_restart(request).map_err(|error| error.to_string())
}

fn game_restart_output(result: &GameRestartResult) -> InvokeCommandResult {
  let mut output = InvokeCommandOutput::from_result(result)?;
  output.report = Some(InvokeReport::new(
    vec![
      InvokeReportField::new("Target", &result.target),
      InvokeReportField::new("Clicks", result.clicks.len().to_string()),
      InvokeReportField::new("Outcome", game_restart_outcome_label(&result.outcome)),
    ],
    Vec::new(),
  ));
  Ok(output)
}

fn game_restart_outcome_label(outcome: &GameRestartOutcome) -> &'static str {
  match outcome {
    GameRestartOutcome::NotChecked => "not checked",
    GameRestartOutcome::Started { .. } => "started",
    GameRestartOutcome::NotStarted { .. } => "not started",
    GameRestartOutcome::Incomplete { .. } => "incomplete",
  }
}

#[invoke_command(
  id = "game.balatro.pack_choose",
  group = "game",
  description = "Choose an active Balatro pack item and return each typed input action plus the operation state.",
  args = PACK_CHOOSE_ARGS,
)]
async fn pack_choose(input: InvokeCommandInput) -> InvokeCommandResult {
  let request = parse_pack_choose_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let result = execute_pack_choose(request, input.cancellation).await?;
  pack_choose_output(&result)
}

pub(crate) fn parse_pack_choose_request(
  target: Option<&str>,
  inputs: &std::collections::BTreeMap<String, String>,
) -> Result<PackChooseRequest, String> {
  for name in inputs.keys() {
    if !matches!(name.as_str(), "choice" | "hand_targets" | "confirm_applied" | "timeout_ms") {
      return Err(format!("{PACK_CHOOSE_COMMAND_ID} does not accept --{name}"));
    }
  }
  let choice = inputs.get("choice").ok_or_else(|| format!("{PACK_CHOOSE_COMMAND_ID} requires --choice pack:N"))?;
  let index = choice
    .strip_prefix("pack:")
    .ok_or_else(|| format!("invalid --choice {choice:?}; expected pack:N"))?
    .parse::<u32>()
    .map_err(|_| format!("invalid --choice {choice:?}; expected pack:N"))?;
  let confirm_applied = match inputs.get("confirm_applied").map(String::as_str).unwrap_or("true") {
    "true" => true,
    "false" => false,
    other => return Err(format!("invalid --confirm_applied {other:?}; expected true or false")),
  };
  Ok(PackChooseRequest {
    target: target.unwrap_or("Balatro").to_string(),
    choice: PackChoiceId::new(index),
    hand_targets: parse_hand_targets(inputs.get("hand_targets"))?,
    confirm_applied,
    timeout_ms: parse_timeout_ms(inputs, 1200)?,
  })
}

pub(crate) async fn execute_pack_choose(
  request: PackChooseRequest,
  cancellation: auv_cli_invoke::InvokeCancellation,
) -> Result<PackChooseResult, String> {
  cancellation.check().map_err(|error| error.to_string())?;
  auv_game_balatro::pack_choose(request).map_err(|error| error.to_string())
}

fn pack_choose_output(result: &PackChooseResult) -> InvokeCommandResult {
  let mut output = InvokeCommandOutput::from_result(result)?;
  output.report = Some(InvokeReport::new(
    vec![
      InvokeReportField::new("Target", &result.target),
      InvokeReportField::new("Choice", result.choice.id.to_string()),
      InvokeReportField::new("Actions", result.actions.len().to_string()),
      InvokeReportField::new("State", pack_choose_state_label(&result.state)),
    ],
    Vec::new(),
  ));
  Ok(output)
}

fn pack_choose_state_label(state: &PackChooseState) -> &'static str {
  match state {
    PackChooseState::Stopped { .. } => "stopped",
    PackChooseState::Submitted { .. } => "submitted",
  }
}

#[invoke_command(
  id = "game.balatro.pack_skip",
  group = "game",
  description = "Skip the active Balatro card pack and return typed delivery plus pack-exit confirmation.",
  args = PACK_SKIP_ARGS,
)]
async fn pack_skip(input: InvokeCommandInput) -> InvokeCommandResult {
  let request = parse_pack_skip_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let result = execute_pack_skip(request, input.cancellation).await?;
  pack_skip_output(&result)
}

pub(crate) fn parse_pack_skip_request(
  target: Option<&str>,
  inputs: &std::collections::BTreeMap<String, String>,
) -> Result<PackSkipRequest, String> {
  for name in inputs.keys() {
    if !matches!(name.as_str(), "confirm_exit" | "timeout_ms") {
      return Err(format!("{PACK_SKIP_COMMAND_ID} does not accept --{name}"));
    }
  }
  let confirm_exit = match inputs.get("confirm_exit").map(String::as_str).unwrap_or("true") {
    "true" => true,
    "false" => false,
    other => return Err(format!("invalid --confirm_exit {other:?}; expected true or false")),
  };
  let timeout_ms = match inputs.get("timeout_ms") {
    Some(raw) => raw.parse::<u64>().map_err(|_| format!("invalid --timeout_ms {raw:?}; expected an integer from 1 to 60000"))?,
    None => 1200,
  };
  if !(1..=60_000).contains(&timeout_ms) {
    return Err(format!("invalid --timeout_ms {timeout_ms}; expected an integer from 1 to 60000"));
  }
  Ok(PackSkipRequest {
    target: target.unwrap_or("Balatro").to_string(),
    confirm_exit,
    timeout_ms,
  })
}

pub(crate) async fn execute_pack_skip(
  request: PackSkipRequest,
  cancellation: auv_cli_invoke::InvokeCancellation,
) -> Result<PackSkipResult, String> {
  cancellation.check().map_err(|error| error.to_string())?;
  auv_game_balatro::pack_skip(request).map_err(|error| error.to_string())
}

fn pack_skip_output(result: &PackSkipResult) -> InvokeCommandResult {
  let mut output = InvokeCommandOutput::from_result(result)?;
  output.report = Some(InvokeReport::new(
    vec![
      InvokeReportField::new("Target", &result.target),
      InvokeReportField::new("Delivery", format!("{:?}", result.delivery.selected_path)),
      InvokeReportField::new("Confirmation", pack_skip_confirmation_label(&result.confirmation)),
    ],
    Vec::new(),
  ));
  Ok(output)
}

fn pack_skip_confirmation_label(confirmation: &PackSkipConfirmation) -> &'static str {
  match confirmation {
    PackSkipConfirmation::NotRequested => "not requested",
    PackSkipConfirmation::Confirmed { .. } => "confirmed",
    PackSkipConfirmation::NotConfirmed { .. } => "not confirmed",
  }
}

#[invoke_command(
  id = "game.balatro.store_buy",
  group = "game",
  description = "Select and submit a Balatro store purchase while preserving both typed input deliveries and purchase state.",
  args = STORE_BUY_ARGS,
)]
async fn store_buy(input: InvokeCommandInput) -> InvokeCommandResult {
  let request = parse_store_buy_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let result = execute_store_buy(request, input.cancellation).await?;
  store_buy_output(&result)
}

pub(crate) fn parse_store_buy_request(
  target: Option<&str>,
  inputs: &std::collections::BTreeMap<String, String>,
) -> Result<StoreBuyRequest, String> {
  for name in inputs.keys() {
    if !matches!(name.as_str(), "slot" | "confirm_purchase" | "timeout_ms") {
      return Err(format!("{STORE_BUY_COMMAND_ID} does not accept --{name}"));
    }
  }
  let slot = inputs.get("slot").ok_or_else(|| format!("{STORE_BUY_COMMAND_ID} requires --slot store:N"))?;
  let index = slot
    .strip_prefix("store:")
    .ok_or_else(|| format!("invalid --slot {slot:?}; expected store:N"))?
    .parse::<u32>()
    .map_err(|_| format!("invalid --slot {slot:?}; expected store:N"))?;
  let confirm_purchase = match inputs.get("confirm_purchase").map(String::as_str).unwrap_or("true") {
    "true" => true,
    "false" => false,
    other => return Err(format!("invalid --confirm_purchase {other:?}; expected true or false")),
  };
  Ok(StoreBuyRequest {
    target: target.unwrap_or("Balatro").to_string(),
    slot: SlotId::new(ObjectZone::Store, index),
    confirm_purchase,
    timeout_ms: parse_timeout_ms(inputs, 1000)?,
  })
}

pub(crate) async fn execute_store_buy(
  request: StoreBuyRequest,
  cancellation: auv_cli_invoke::InvokeCancellation,
) -> Result<StoreBuyResult, String> {
  cancellation.check().map_err(|error| error.to_string())?;
  auv_game_balatro::store_buy(request).map_err(|error| error.to_string())
}

fn store_buy_output(result: &StoreBuyResult) -> InvokeCommandResult {
  let mut output = InvokeCommandOutput::from_result(result)?;
  output.report = Some(InvokeReport::new(
    vec![
      InvokeReportField::new("Target", &result.target),
      InvokeReportField::new("Slot", result.slot.to_string()),
      InvokeReportField::new("Outcome", store_buy_outcome_label(&result.outcome)),
    ],
    Vec::new(),
  ));
  Ok(output)
}

fn store_buy_outcome_label(outcome: &StoreBuyOutcome) -> &'static str {
  match outcome {
    StoreBuyOutcome::SelectionOnly { .. } => "selection only",
    StoreBuyOutcome::Submitted { .. } => "submitted",
  }
}

#[invoke_command(
  id = "game.balatro.store_next_round",
  group = "game",
  description = "Leave the Balatro store and return typed delivery plus app-specific state confirmation.",
  args = STORE_NEXT_ROUND_ARGS,
)]
async fn store_next_round(input: InvokeCommandInput) -> InvokeCommandResult {
  let request = parse_store_next_round_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let result = execute_store_next_round(request, input.cancellation).await?;
  store_next_round_output(&result)
}

pub(crate) fn parse_store_next_round_request(
  target: Option<&str>,
  inputs: &std::collections::BTreeMap<String, String>,
) -> Result<StoreNextRoundRequest, String> {
  for name in inputs.keys() {
    if !matches!(name.as_str(), "confirmation" | "timeout_ms") {
      return Err(format!("{STORE_NEXT_ROUND_COMMAND_ID} does not accept --{name}"));
    }
  }
  let confirmation = match inputs.get("confirmation").map(String::as_str).unwrap_or("targeted") {
    "none" => StoreNextRoundConfirmationRequest::None,
    "targeted" => StoreNextRoundConfirmationRequest::Targeted,
    "weak" => StoreNextRoundConfirmationRequest::Weak,
    other => return Err(format!("invalid --confirmation {other:?}; expected targeted, weak, or none")),
  };
  let timeout_ms = match inputs.get("timeout_ms") {
    Some(raw) => raw.parse::<u64>().map_err(|_| format!("invalid --timeout_ms {raw:?}; expected an integer from 1 to 60000"))?,
    None => 1200,
  };
  if !(1..=60_000).contains(&timeout_ms) {
    return Err(format!("invalid --timeout_ms {timeout_ms}; expected an integer from 1 to 60000"));
  }
  Ok(StoreNextRoundRequest {
    target: target.unwrap_or("Balatro").to_string(),
    confirmation,
    timeout_ms,
  })
}

pub(crate) async fn execute_store_next_round(
  request: StoreNextRoundRequest,
  cancellation: auv_cli_invoke::InvokeCancellation,
) -> Result<StoreNextRoundResult, String> {
  // A cancellation observed after delivery cannot turn an already-clicked
  // operation into a retryable frontend failure.
  cancellation.check().map_err(|error| error.to_string())?;
  auv_game_balatro::store_next_round(request).map_err(|error| error.to_string())
}

fn store_next_round_output(result: &StoreNextRoundResult) -> InvokeCommandResult {
  let mut output = InvokeCommandOutput::from_result(result)?;
  output.report = Some(InvokeReport::new(
    vec![
      InvokeReportField::new("Target", &result.target),
      InvokeReportField::new("Delivery", format!("{:?}", result.delivery.selected_path)),
      InvokeReportField::new("Confirmation", store_next_round_confirmation_label(&result.confirmation)),
    ],
    Vec::new(),
  ));
  Ok(output)
}

fn store_next_round_confirmation_label(confirmation: &StoreNextRoundConfirmation) -> &'static str {
  match confirmation {
    StoreNextRoundConfirmation::NotRequested => "not requested",
    StoreNextRoundConfirmation::Confirmed { .. } => "confirmed",
    StoreNextRoundConfirmation::NotConfirmed { .. } => "not confirmed",
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BalatroConsumptionProbeChainOutput {
  pub semantic: CardDetectionSemanticValidationOutput,
  pub query: CardDetectionSpatialQueryOutput,
  pub witness: CardDetectionEvalWitnessOutput,
  pub quality: CardDetectionQualityOutput,
}

pub async fn run_balatro_card_detection_semantic_validation(
  bundle_input: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<CardDetectionSemanticValidationOutput> {
  let result = validate_card_detection_semantic(CardDetectionSemanticValidationInputs {
    bundle_input,
    output_dir,
  })?;
  let context = Context::current();
  let _ = auv_game_balatro::card_detection_semantic::publish_card_detection_semantic(Some(&context), &result.manifest).await;
  Ok(result)
}

pub async fn run_balatro_card_detection_spatial_query(
  card_detection_semantic_manifest_path: PathBuf,
  target_slot: SlotId,
  output_dir: PathBuf,
) -> AuvResult<CardDetectionSpatialQueryOutput> {
  let result = query_card_detection_spatial(CardDetectionSpatialQueryInputs {
    card_detection_semantic_manifest_path,
    target_slot,
    output_dir,
  })?;
  let context = Context::current();
  let _ = auv_game_balatro::card_detection_spatial_query::publish_card_detection_spatial_query(Some(&context), &result.manifest).await;
  Ok(result)
}

pub async fn run_balatro_card_detection_eval_witness(
  card_detection_semantic_manifest_path: PathBuf,
  card_detection_spatial_query_manifest_path: PathBuf,
  expected_slots_path: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<CardDetectionEvalWitnessOutput> {
  let result = build_card_detection_eval_witness(&CardDetectionEvalWitnessInputs {
    card_detection_semantic_manifest_path,
    card_detection_spatial_query_manifest_path,
    expected_slots_path,
    output_dir,
  })?;
  let context = Context::current();
  let _ = auv_game_balatro::card_detection_eval_witness::publish_card_detection_witness(Some(&context), &result.manifest).await;
  Ok(result)
}

pub async fn run_balatro_card_detection_quality(
  witness_manifest_path: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<CardDetectionQualityOutput> {
  let result = build_card_detection_quality(&CardDetectionQualityInputs {
    witness_manifest_path,
    output_dir,
  })?;
  let context = Context::current();
  let _ = auv_game_balatro::card_detection_quality::publish_card_detection_quality(Some(&context), &result.manifest).await;
  Ok(result)
}

pub async fn run_balatro_consumption_probe_chain(
  bundle_input: PathBuf,
  expected_slots_path: PathBuf,
  target_slot: SlotId,
  work_dir: PathBuf,
) -> AuvResult<BalatroConsumptionProbeChainOutput> {
  let semantic = run_balatro_card_detection_semantic_validation(bundle_input, work_dir.join("semantic")).await?;
  let query = run_balatro_card_detection_spatial_query(semantic.manifest_path.clone(), target_slot, work_dir.join("query")).await?;
  let witness = run_balatro_card_detection_eval_witness(
    semantic.manifest_path.clone(),
    query.manifest_path.clone(),
    expected_slots_path,
    work_dir.join("witness"),
  )
  .await?;
  let quality = run_balatro_card_detection_quality(witness.manifest_path.clone(), work_dir.join("quality")).await?;
  Ok(BalatroConsumptionProbeChainOutput {
    semantic,
    query,
    witness,
    quality,
  })
}
