use super::*;

fn node<'a>(path: &'a str, name: &'a str, automation_id: &'a str, control_type: &'a str, x: f64) -> NodeEvidence<'a> {
  NodeEvidence {
    path,
    name,
    value: None,
    automation_id,
    control_type,
    bounds: Rect::new(x, 650.0, 40.0, 40.0),
  }
}

fn window_bounds() -> Rect {
  Rect::new(0.0, 0.0, 1_000.0, 800.0)
}

#[test]
fn chooses_localized_transport_button_names() {
  let nodes = [
    node("0/1", "上一首", "", "button", 420.0),
    node("0/2", "暂停", "", "button", 480.0),
    node("0/3", "下一首", "", "button", 540.0),
  ];

  assert_eq!(choose_control(nodes, TransportAction::PlayPause, window_bounds()).unwrap().path, "0/2");
  assert_eq!(choose_control(nodes, TransportAction::Next, window_bounds()).unwrap().path, "0/3");
  assert_eq!(choose_control(nodes, TransportAction::Previous, window_bounds()).unwrap().path, "0/1");
}

#[test]
fn automation_id_matches_when_accessible_name_is_missing() {
  let matched = choose_control([node("0/4", "", "player-next-track", "custom", 520.0)], TransportAction::Next, window_bounds()).unwrap();

  assert_eq!(matched.path, "0/4");
}

#[test]
fn chooses_live_netease_minibar_menu_item_for_play_pause() {
  let matched = choose_control(
    [node(
      "0/0/0/0/36/9",
      "play",
      "btn_pc_minibar_play",
      "menu item",
      480.0,
    )],
    TransportAction::PlayPause,
    window_bounds(),
  )
  .unwrap();

  assert_eq!(matched.path, "0/0/0/0/36/9");
}

#[test]
fn chooses_live_netease_pre_name_for_previous() {
  let matched = choose_control([node("0/0/0/0/36/8", "pre", "", "button", 440.0)], TransportAction::Previous, window_bounds()).unwrap();

  assert_eq!(matched.path, "0/0/0/0/36/8");
}

#[test]
fn exact_button_name_beats_weaker_automation_id_match() {
  let matched = choose_control(
    [
      node("0/1", "", "next-track", "button", 500.0),
      node("0/2", "Next", "", "button", 540.0),
    ],
    TransportAction::Next,
    window_bounds(),
  )
  .unwrap();

  assert_eq!(matched.path, "0/2");
}

#[test]
fn rejects_equal_best_matches_instead_of_invoking_arbitrarily() {
  let error = choose_control(
    [
      node("0/1", "Next", "", "button", 500.0),
      node("0/2", "Next", "", "button", 540.0),
    ],
    TransportAction::Next,
    window_bounds(),
  )
  .unwrap_err();

  assert!(error.contains("multiple UIA controls"));
}

#[test]
fn does_not_match_non_button_text_by_name_alone() {
  let error = choose_control([node("0/1", "Next", "", "text", 500.0)], TransportAction::Next, window_bounds()).unwrap_err();

  assert!(error.contains("no UIA control matched"));
}

#[test]
fn ignores_content_play_buttons_above_bottom_transport_band() {
  let error = choose_control(
    [NodeEvidence {
      path: "0/30/29",
      name: "play",
      value: None,
      automation_id: "",
      control_type: "button",
      bounds: Rect::new(300.0, 400.0, 43.0, 43.0),
    }],
    TransportAction::PlayPause,
    window_bounds(),
  )
  .unwrap_err();

  assert!(error.contains("no UIA control matched"));
}

#[test]
fn classify_playpause_state_detects_pause_from_localized_name() {
  assert_eq!(classify_playpause_state("暂停", None), PlaybackControlState::PauseVisible);
  assert_eq!(classify_playpause_state("Pause", None), PlaybackControlState::PauseVisible);
}

#[test]
fn classify_playpause_state_detects_play_from_localized_name() {
  assert_eq!(classify_playpause_state("播放", None), PlaybackControlState::PlayVisible);
  assert_eq!(classify_playpause_state("Play", None), PlaybackControlState::PlayVisible);
}

#[test]
fn classify_playpause_state_falls_back_to_value_when_name_is_generic() {
  assert_eq!(classify_playpause_state("播放暂停", Some("暂停")), PlaybackControlState::PauseVisible);
  assert_eq!(classify_playpause_state("playpause", Some("Play")), PlaybackControlState::PlayVisible);
}

#[test]
fn classify_playpause_state_is_unknown_for_generic_or_unrelated_labels() {
  assert_eq!(classify_playpause_state("播放暂停", None), PlaybackControlState::Unknown);
  assert_eq!(classify_playpause_state("playpause", Some("playpause")), PlaybackControlState::Unknown);
  assert_eq!(classify_playpause_state("", None), PlaybackControlState::Unknown);
  assert_eq!(classify_playpause_state("上一首", None), PlaybackControlState::Unknown);
}
