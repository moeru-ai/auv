use std::path::PathBuf;

use auv_game_balatro::cli::{
  BlindsArgs, BlindsCommand, CardsArgs, CardsCommand, CliArgs, CliError, Command, ConsumablesArgs,
  ConsumablesCommand, Format, GameArgs, GameCommand, JokersArgs, JokersCommand, ObjectiveArgs,
  PackArgs, PackCommand, RoundsArgs, RoundsCommand, ScoresArgs, ScoresCommand, SetupArgs,
  StoreArgs, StoreCommand, VerifyModeArg, run,
};
use auv_game_balatro::output::OutputMode;
use auv_inference_ultralytics::InferenceDevice;
use clap::Parser;

#[test]
fn rejects_dry_run_for_balatro_operations() {
  let error = CliArgs::try_parse_from(["auv-game-balatro", "store", "next-round", "--dry-run"])
    .expect_err("Balatro mutating commands should not expose dry-run");

  assert!(
    error
      .to_string()
      .contains("unexpected argument '--dry-run'"),
    "{error}"
  );
}

#[test]
fn parses_setup_asset_command_flags() {
  let args = CliArgs::parse_from([
    "auv-game-balatro",
    "setup",
    "--love",
    "Balatro.love",
    "--cache-dir",
    "cache",
    "--force",
    "--json",
  ]);

  let Command::Setup(SetupArgs {
    love,
    app,
    cache_dir,
    check,
    force,
    json,
  }) = args.command
  else {
    panic!("expected setup command");
  };

  assert_eq!(love, Some(PathBuf::from("Balatro.love")));
  assert_eq!(app, None);
  assert_eq!(cache_dir, Some(PathBuf::from("cache")));
  assert!(!check);
  assert!(force);
  assert!(json);
}

#[test]
fn parses_details_aliases_for_balatro_operations() {
  let details = CliArgs::parse_from(["auv-game-balatro", "cards", "clear", "--details"]);
  let Command::Cards(CardsArgs {
    command: CardsCommand::Clear(details_args),
  }) = details.command
  else {
    panic!("expected cards clear command");
  };
  assert!(details_args.details);

  let detailed = CliArgs::parse_from(["auv-game-balatro", "pack", "skip", "--detailed"]);
  let Command::Pack(PackArgs {
    command: PackCommand::Skip(detailed_args),
  }) = detailed.command
  else {
    panic!("expected pack skip command");
  };
  assert!(detailed_args.details);
}

#[test]
fn parses_cards_hand_observation_flags() {
  let args = CliArgs::parse_from([
    "auv-game-balatro",
    "cards",
    "hand",
    "--image",
    "balatro.jpg",
    "--json",
    "--no-cache",
    "--target",
    "Balatro",
  ]);

  let Command::Cards(CardsArgs {
    command: CardsCommand::Hand(hand),
  }) = args.command
  else {
    panic!("expected cards hand command");
  };

  assert_eq!(hand.image, Some(PathBuf::from("balatro.jpg")));
  assert!(hand.json);
  assert!(hand.no_cache);
  assert_eq!(hand.target, "Balatro");
  assert_eq!(hand.output_mode(), OutputMode::Json);
}

#[test]
fn parses_store_buy_operation_flags() {
  let args = CliArgs::parse_from([
    "auv-game-balatro",
    "store",
    "buy",
    "--slot",
    "store:0",
    "--details",
    "--verify",
    "--verify-mode",
    "weak",
  ]);

  let Command::Store(StoreArgs {
    command: StoreCommand::Buy(buy),
  }) = args.command
  else {
    panic!("expected store buy command");
  };

  assert_eq!(buy.slot, "store:0");
  assert!(buy.control.details);
  assert!(buy.control.verify);
  assert_eq!(buy.control.verify_mode, VerifyModeArg::Weak);
}

#[test]
fn parses_store_read_buy_and_reroll_commands() {
  let read = CliArgs::parse_from([
    "auv-game-balatro",
    "store",
    "read",
    "--slot",
    "store:1",
    "--json",
  ]);
  let Command::Store(StoreArgs {
    command: StoreCommand::Read(read_args),
  }) = read.command
  else {
    panic!("expected store read command");
  };
  assert_eq!(read_args.slot, "store:1");
  assert!(read_args.observe.json);

  let buy = CliArgs::parse_from([
    "auv-game-balatro",
    "store",
    "buy",
    "--slot",
    "store:0",
    "--verify",
  ]);
  let Command::Store(StoreArgs {
    command: StoreCommand::Buy(buy_args),
  }) = buy.command
  else {
    panic!("expected store buy command");
  };
  assert_eq!(buy_args.slot, "store:0");
  assert!(buy_args.control.verify);

  let reroll = CliArgs::parse_from([
    "auv-game-balatro",
    "store",
    "reroll",
    "--verify-mode",
    "weak",
  ]);
  let Command::Store(StoreArgs {
    command: StoreCommand::Reroll(reroll_args),
  }) = reroll.command
  else {
    panic!("expected store reroll command");
  };
  assert_eq!(reroll_args.verify_mode, VerifyModeArg::Weak);
}

#[test]
fn parses_pack_read_choose_and_skip_commands() {
  let read = CliArgs::parse_from(["auv-game-balatro", "pack", "read", "--json"]);
  let Command::Pack(PackArgs {
    command: PackCommand::Read(read_args),
  }) = read.command
  else {
    panic!("expected pack read command");
  };
  assert!(read_args.json);

  let choose = CliArgs::parse_from([
    "auv-game-balatro",
    "pack",
    "choose",
    "--slot",
    "pack:2",
    "--verify",
  ]);
  let Command::Pack(PackArgs {
    command: PackCommand::Choose(choose_args),
  }) = choose.command
  else {
    panic!("expected pack choose command");
  };
  assert_eq!(choose_args.slot, "pack:2");
  assert!(choose_args.control.verify);

  let skip = CliArgs::parse_from(["auv-game-balatro", "pack", "skip", "--verify"]);
  let Command::Pack(PackArgs {
    command: PackCommand::Skip(skip_args),
  }) = skip.command
  else {
    panic!("expected pack skip command");
  };
  assert!(skip_args.verify);
}

#[test]
fn parses_consumable_and_joker_read_use_commands() {
  let read_joker = CliArgs::parse_from([
    "auv-game-balatro",
    "jokers",
    "read",
    "--slot",
    "joker:0",
    "--json",
  ]);
  let Command::Jokers(JokersArgs {
    command: JokersCommand::Read(joker_args),
  }) = read_joker.command
  else {
    panic!("expected jokers read command");
  };
  assert_eq!(joker_args.slot, "joker:0");
  assert!(joker_args.observe.json);

  let read_consumable = CliArgs::parse_from([
    "auv-game-balatro",
    "consumables",
    "read",
    "--slot",
    "consumable:1",
  ]);
  let Command::Consumables(ConsumablesArgs {
    command: ConsumablesCommand::Read(read_args),
  }) = read_consumable.command
  else {
    panic!("expected consumables read command");
  };
  assert_eq!(read_args.slot, "consumable:1");

  let use_consumable = CliArgs::parse_from([
    "auv-game-balatro",
    "consumables",
    "use",
    "--slot",
    "consumable:0",
    "--verify",
  ]);
  let Command::Consumables(ConsumablesArgs {
    command: ConsumablesCommand::Use(use_args),
  }) = use_consumable.command
  else {
    panic!("expected consumables use command");
  };
  assert_eq!(use_args.slot, "consumable:0");
  assert!(use_args.control.verify);
}

#[test]
fn parses_consumable_sell_and_target_operations() {
  let sell_consumable = CliArgs::parse_from([
    "auv-game-balatro",
    "consumables",
    "sell",
    "--slot",
    "consumable:0",
    "--verify",
  ]);
  let Command::Consumables(ConsumablesArgs {
    command: ConsumablesCommand::Sell(sell_args),
  }) = sell_consumable.command
  else {
    panic!("expected consumables sell command");
  };
  assert_eq!(sell_args.slot, "consumable:0");
  assert!(sell_args.control.verify);

  let use_consumable = CliArgs::parse_from([
    "auv-game-balatro",
    "consumables",
    "use",
    "--slot",
    "consumable:0",
    "--targets",
    "hand:1,hand:2",
    "--verify",
  ]);
  let Command::Consumables(ConsumablesArgs {
    command: ConsumablesCommand::Use(use_args),
  }) = use_consumable.command
  else {
    panic!("expected consumables use command");
  };
  assert_eq!(use_args.slot, "consumable:0");
  assert_eq!(use_args.targets, vec!["hand:1", "hand:2"]);
  assert!(use_args.control.verify);

  let pack_choose = CliArgs::parse_from([
    "auv-game-balatro",
    "pack",
    "choose",
    "--slot",
    "pack:0",
    "--targets",
    "hand:1",
    "--verify",
  ]);
  let Command::Pack(PackArgs {
    command: PackCommand::Choose(choose_args),
  }) = pack_choose.command
  else {
    panic!("expected pack choose command");
  };
  assert_eq!(choose_args.slot, "pack:0");
  assert_eq!(choose_args.targets, vec!["hand:1"]);
  assert!(choose_args.control.verify);
}

#[test]
fn deferred_mutating_object_commands_validate_slot_prefixes_first() {
  let cases = [
    (
      CliArgs::parse_from(["auv-game-balatro", "store", "buy", "--slot", "bad"]),
      "store:N",
    ),
    (
      CliArgs::parse_from(["auv-game-balatro", "consumables", "use", "--slot", "bad"]),
      "consumable:N",
    ),
    (
      CliArgs::parse_from(["auv-game-balatro", "jokers", "sell", "--slot", "bad"]),
      "joker:N",
    ),
  ];

  for (args, expected) in cases {
    let error = run(args).expect_err("invalid slot should fail before deferred execution");
    let CliError::Message(message) = error else {
      panic!("expected slot validation error, got {error:?}");
    };
    assert!(message.contains(expected), "{message}");
  }

  let reroll = CliArgs::parse_from(["auv-game-balatro", "store", "reroll"]);
  assert!(matches!(
    run(reroll),
    Err(CliError::Deferred {
      command: "store.reroll",
      ..
    })
  ));
}

#[test]
fn parses_game_state_format_json() {
  let args = CliArgs::parse_from([
    "auv-game-balatro",
    "game",
    "state",
    "--image",
    "balatro.jpg",
    "--format",
    "json",
  ]);

  let Command::Game(GameArgs {
    command: GameCommand::State(state),
  }) = args.command
  else {
    panic!("expected game state command");
  };

  assert_eq!(state.image, Some(PathBuf::from("balatro.jpg")));
  assert_eq!(state.format, Format::Json);
  assert_eq!(state.output_mode(), OutputMode::Json);
}

#[test]
fn parses_game_cash_out_operation_flags() {
  let args = CliArgs::parse_from([
    "auv-game-balatro",
    "game",
    "cash-out",
    "--verify",
    "--timeout-ms",
    "1800",
  ]);

  let Command::Game(GameArgs {
    command: GameCommand::CashOut(cash_out),
  }) = args.command
  else {
    panic!("expected game cash-out command");
  };

  assert!(cash_out.verify);
  assert_eq!(cash_out.timeout_ms, Some(1800));
}

#[test]
fn parses_game_restart_operation_flags() {
  let args = CliArgs::parse_from([
    "auv-game-balatro",
    "game",
    "restart",
    "--details",
    "--verify",
  ]);

  let Command::Game(GameArgs {
    command: GameCommand::Restart(restart),
  }) = args.command
  else {
    panic!("expected game restart command");
  };

  assert!(restart.details);
  assert!(restart.verify);
}

#[test]
fn parses_objective_observation_includes() {
  let args = CliArgs::parse_from([
    "auv-game-balatro",
    "objective",
    "--include-scores",
    "--include-rounds",
    "--image",
    "balatro.jpg",
  ]);

  let Command::Objective(ObjectiveArgs {
    observe,
    include_scores,
    include_rounds,
  }) = args.command
  else {
    panic!("expected objective command");
  };

  assert!(include_scores);
  assert!(include_rounds);
  assert_eq!(observe.image, Some(PathBuf::from("balatro.jpg")));
  assert_eq!(observe.output_mode(), OutputMode::Human);
}

#[test]
fn parses_store_status_observation_defaults() {
  let args = CliArgs::parse_from([
    "auv-game-balatro",
    "store",
    "status",
    "--image",
    "balatro.jpg",
  ]);

  let Command::Store(StoreArgs {
    command: StoreCommand::Status(status),
  }) = args.command
  else {
    panic!("expected store status command");
  };

  assert_eq!(status.image, Some(PathBuf::from("balatro.jpg")));
  assert_eq!(status.device, InferenceDevice::Cpu);
  assert_eq!(status.output_mode(), OutputMode::Human);
}

#[test]
fn parses_observation_model_overrides() {
  let args = CliArgs::parse_from([
    "auv-game-balatro",
    "store",
    "status",
    "--entities-model",
    "entities.onnx",
    "--entities-classes",
    "entities.txt",
    "--ui-model",
    "ui.onnx",
    "--ui-classes",
    "ui.txt",
    "--card-corner-model",
    "card-corner.onnx",
  ]);

  let Command::Store(StoreArgs {
    command: StoreCommand::Status(status),
  }) = args.command
  else {
    panic!("expected store status command");
  };

  assert_eq!(status.entities_model, Some(PathBuf::from("entities.onnx")));
  assert_eq!(status.entities_classes, Some(PathBuf::from("entities.txt")));
  assert_eq!(status.ui_model, Some(PathBuf::from("ui.onnx")));
  assert_eq!(status.ui_classes, Some(PathBuf::from("ui.txt")));
  assert_eq!(
    status.card_corner_model,
    Some(PathBuf::from("card-corner.onnx"))
  );
}

#[test]
fn maps_json_out_to_file_output_mode() {
  let args = CliArgs::parse_from([
    "auv-game-balatro",
    "game",
    "state",
    "--json-out",
    "state.json",
    "--json",
  ]);

  let Command::Game(GameArgs {
    command: GameCommand::State(state),
  }) = args.command
  else {
    panic!("expected game state command");
  };

  assert_eq!(
    state.output_mode(),
    OutputMode::JsonFile(PathBuf::from("state.json"))
  );
}

#[test]
fn parses_operation_slots_and_timeout() {
  let args = CliArgs::parse_from([
    "auv-game-balatro",
    "cards",
    "play",
    "--slots",
    "hand:0,hand:1",
    "--timeout-ms",
    "750",
  ]);

  let Command::Cards(CardsArgs {
    command: CardsCommand::Play(play),
  }) = args.command
  else {
    panic!("expected cards play command");
  };

  assert_eq!(play.slots, "hand:0,hand:1");
  assert_eq!(play.control.timeout_ms, Some(750));
  assert_eq!(play.control.verify_mode, VerifyModeArg::Targeted);
}

#[test]
fn parses_scores_and_rounds_get_commands() {
  let scores = CliArgs::parse_from(["auv-game-balatro", "scores", "get", "--image", "a.png"]);
  let Command::Scores(ScoresArgs {
    command: ScoresCommand::Get(score_args),
  }) = scores.command
  else {
    panic!("expected scores get command");
  };
  assert_eq!(score_args.image, Some(PathBuf::from("a.png")));

  let rounds = CliArgs::parse_from(["auv-game-balatro", "rounds", "get", "--image", "b.png"]);
  let Command::Rounds(RoundsArgs {
    command: RoundsCommand::Get(round_args),
  }) = rounds.command
  else {
    panic!("expected rounds get command");
  };
  assert_eq!(round_args.image, Some(PathBuf::from("b.png")));
}

#[test]
fn parses_card_list_read_and_select_commands() {
  let list = CliArgs::parse_from(["auv-game-balatro", "cards", "ls", "--image", "hand.png"]);
  let Command::Cards(CardsArgs {
    command: CardsCommand::Ls(list_args),
  }) = list.command
  else {
    panic!("expected cards ls command");
  };
  assert_eq!(list_args.image, Some(PathBuf::from("hand.png")));

  let read = CliArgs::parse_from([
    "auv-game-balatro",
    "cards",
    "read",
    "--slot",
    "hand:3",
    "--no-cache",
    "--target",
    "Balatro",
  ]);
  let Command::Cards(CardsArgs {
    command: CardsCommand::Read(read_args),
  }) = read.command
  else {
    panic!("expected cards read command");
  };
  assert_eq!(read_args.slot, "hand:3");
  assert!(read_args.observe.no_cache);
  assert_eq!(read_args.observe.target, "Balatro");

  let select = CliArgs::parse_from([
    "auv-game-balatro",
    "cards",
    "select",
    "--slots",
    "hand:0,hand:2",
  ]);
  let Command::Cards(CardsArgs {
    command: CardsCommand::Select(select_args),
  }) = select.command
  else {
    panic!("expected cards select command");
  };
  assert_eq!(select_args.slots, "hand:0,hand:2");

  let clear = CliArgs::parse_from(["auv-game-balatro", "cards", "clear", "--verify"]);
  let Command::Cards(CardsArgs {
    command: CardsCommand::Clear(clear_args),
  }) = clear.command
  else {
    panic!("expected cards clear command");
  };
  assert!(clear_args.verify);
}

#[test]
fn parses_cards_discard_command() {
  let discard = CliArgs::parse_from([
    "auv-game-balatro",
    "cards",
    "discard",
    "--slots",
    "hand:1,hand:3",
    "--verify",
  ]);
  let Command::Cards(CardsArgs {
    command: CardsCommand::Discard(discard_args),
  }) = discard.command
  else {
    panic!("expected cards discard command");
  };
  assert_eq!(discard_args.slots, "hand:1,hand:3");
  assert!(discard_args.control.verify);
}

#[test]
fn parses_joker_consumable_store_and_blind_object_commands() {
  let jokers = CliArgs::parse_from(["auv-game-balatro", "jokers", "ls", "--image", "j.png"]);
  let Command::Jokers(JokersArgs {
    command: JokersCommand::Ls(joker_args),
  }) = jokers.command
  else {
    panic!("expected jokers ls command");
  };
  assert_eq!(joker_args.image, Some(PathBuf::from("j.png")));

  let joker_read = CliArgs::parse_from(["auv-game-balatro", "jokers", "read", "--slot", "joker:1"]);
  let Command::Jokers(JokersArgs {
    command: JokersCommand::Read(joker_read_args),
  }) = joker_read.command
  else {
    panic!("expected jokers read command");
  };
  assert_eq!(joker_read_args.slot, "joker:1");

  let joker_sell = CliArgs::parse_from(["auv-game-balatro", "jokers", "sell", "--slot", "joker:2"]);
  let Command::Jokers(JokersArgs {
    command: JokersCommand::Sell(joker_sell_args),
  }) = joker_sell.command
  else {
    panic!("expected jokers sell command");
  };
  assert_eq!(joker_sell_args.slot, "joker:2");

  let consumables =
    CliArgs::parse_from(["auv-game-balatro", "consumables", "ls", "--image", "c.png"]);
  let Command::Consumables(ConsumablesArgs {
    command: ConsumablesCommand::Ls(consumable_args),
  }) = consumables.command
  else {
    panic!("expected consumables ls command");
  };
  assert_eq!(consumable_args.image, Some(PathBuf::from("c.png")));

  let consumable_use = CliArgs::parse_from([
    "auv-game-balatro",
    "consumables",
    "use",
    "--slot",
    "consumable:0",
  ]);
  let Command::Consumables(ConsumablesArgs {
    command: ConsumablesCommand::Use(use_args),
  }) = consumable_use.command
  else {
    panic!("expected consumables use command");
  };
  assert_eq!(use_args.slot, "consumable:0");

  let store_ls = CliArgs::parse_from(["auv-game-balatro", "store", "ls", "--image", "s.png"]);
  let Command::Store(StoreArgs {
    command: StoreCommand::Ls(store_args),
  }) = store_ls.command
  else {
    panic!("expected store ls command");
  };
  assert_eq!(store_args.image, Some(PathBuf::from("s.png")));

  let next_round = CliArgs::parse_from([
    "auv-game-balatro",
    "store",
    "next-round",
    "--target",
    "Balatro",
    "--details",
  ]);
  let Command::Store(StoreArgs {
    command: StoreCommand::NextRound(next_round_args),
  }) = next_round.command
  else {
    panic!("expected store next-round command");
  };
  assert!(next_round_args.details);
  assert_eq!(next_round_args.target, "Balatro");

  let reroll = CliArgs::parse_from(["auv-game-balatro", "store", "reroll", "--verify"]);
  let Command::Store(StoreArgs {
    command: StoreCommand::Reroll(reroll_args),
  }) = reroll.command
  else {
    panic!("expected store reroll command");
  };
  assert!(reroll_args.verify);

  let blinds = CliArgs::parse_from(["auv-game-balatro", "blinds", "ls", "--image", "b.png"]);
  let Command::Blinds(BlindsArgs {
    command: BlindsCommand::Ls(blind_args),
  }) = blinds.command
  else {
    panic!("expected blinds ls command");
  };
  assert_eq!(blind_args.image, Some(PathBuf::from("b.png")));

  let blind_select = CliArgs::parse_from([
    "auv-game-balatro",
    "blinds",
    "select",
    "--slot",
    "blind:small",
  ]);
  let Command::Blinds(BlindsArgs {
    command: BlindsCommand::Select(select_args),
  }) = blind_select.command
  else {
    panic!("expected blinds select command");
  };
  assert_eq!(select_args.slot, "blind:small");

  let blind_skip = CliArgs::parse_from(["auv-game-balatro", "blinds", "skip", "--details"]);
  let Command::Blinds(BlindsArgs {
    command: BlindsCommand::Skip(skip_args),
  }) = blind_skip.command
  else {
    panic!("expected blinds skip command");
  };
  assert!(skip_args.details);
}

#[test]
fn rejects_missing_targets_for_targeted_commands() {
  assert!(
    CliArgs::try_parse_from(["auv-game-balatro", "cards", "read"]).is_err(),
    "cards read should require --slot"
  );
  assert!(
    CliArgs::try_parse_from(["auv-game-balatro", "store", "buy"]).is_err(),
    "store buy should require --slot"
  );
  assert!(
    CliArgs::try_parse_from(["auv-game-balatro", "cards", "play"]).is_err(),
    "cards play should require --slots"
  );
}

#[test]
fn displays_cli_value_enums() {
  assert_eq!(Format::default(), Format::Text);
  assert_eq!(Format::Text.to_string(), "text");
  assert_eq!(Format::Json.to_string(), "json");
  assert_eq!(VerifyModeArg::Targeted.to_string(), "targeted");
  assert_eq!(VerifyModeArg::Weak.to_string(), "weak");
  assert_eq!(VerifyModeArg::ActivationOnly.to_string(), "activation-only");
}
