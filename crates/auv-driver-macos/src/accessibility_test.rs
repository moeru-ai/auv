use super::*;
use crate::types::ObservedRect;

fn sample_snapshot() -> ObservedAxTreeSnapshot {
  ObservedAxTreeSnapshot {
    observed_at: "now".to_string(),
    app_name: "TextEdit".to_string(),
    bundle_id: "com.apple.TextEdit".to_string(),
    pid: 4242,
    window_title: "Untitled".to_string(),
    nodes: vec![
      ObservedAxNode {
        depth: 1,
        path: "0.1".to_string(),
        role: "AXWindow".to_string(),
        subrole: String::new(),
        title: "Untitled".to_string(),
        description: String::new(),
        help: String::new(),
        identifier: String::new(),
        placeholder: String::new(),
        value: String::new(),
        focused: false,
        bounds: ObservedRect {
          x: 0,
          y: 0,
          width: 800,
          height: 600,
        },
      },
      ObservedAxNode {
        depth: 2,
        path: "0.1.2".to_string(),
        role: "AXTextArea".to_string(),
        subrole: String::new(),
        title: "First Text View".to_string(),
        description: String::new(),
        help: String::new(),
        identifier: String::new(),
        placeholder: String::new(),
        value: "hello body".to_string(),
        focused: false,
        bounds: ObservedRect {
          x: 10,
          y: 40,
          width: 780,
          height: 540,
        },
      },
    ],
  }
}

#[test]
fn select_focus_node_prefers_query_match_with_role() {
  let snapshot = sample_snapshot();
  let node = select_focus_node(&snapshot, "First Text View", Some("AXTextArea"), "").expect("node");
  assert_eq!(node.path, "0.1.2");
  assert_eq!(node.role, "AXTextArea");
}

#[test]
fn select_focus_node_accepts_exact_path_candidate() {
  let snapshot = sample_snapshot();
  let node = select_focus_node(&snapshot, "", None, "0.1.2").expect("path candidate");
  assert_eq!(node.title, "First Text View");
}

#[test]
fn select_focus_node_rejects_unknown_candidate_without_fallback() {
  let snapshot = sample_snapshot();
  let error = select_focus_node(&snapshot, "First Text View", Some("AXTextArea"), "missing.path").expect_err("unknown candidate");
  assert!(error.to_string().contains("missing.path"));
}

#[test]
fn focus_text_options_validate_selector_before_native_capture() {
  for options in [
    FocusTextOptions {
      app: "".to_string(),
      selector: AxTextSelector::Query("Search".to_string()),
      expected_role: None,
    },
    FocusTextOptions {
      app: "com.example.Editor".to_string(),
      selector: AxTextSelector::Query("".to_string()),
      expected_role: None,
    },
    FocusTextOptions {
      app: "com.example.Editor".to_string(),
      selector: AxTextSelector::Path("  ".to_string()),
      expected_role: None,
    },
    FocusTextOptions {
      app: "com.example.Editor".to_string(),
      selector: AxTextSelector::Query("Search".to_string()),
      expected_role: Some("".to_string()),
    },
  ] {
    assert!(options.validate().is_err());
  }
}

#[test]
fn select_text_node_by_role_ignores_expected_content() {
  let snapshot = sample_snapshot();
  let node = select_text_node_by_role(&snapshot, "AXTextArea").expect("role node");
  assert_eq!(node.path, "0.1.2");
  assert_eq!(node.value, "hello body");
}

#[test]
fn text_read_contains_only_ax_facts() {
  let facts = serde_json::json!({
    "app": "com.apple.TextEdit",
    "pid": 4242,
    "path": "0.1.2",
    "role": "AXTextArea",
    "matched_text": "hello body"
  });

  let read: AxTextRead = serde_json::from_value(facts.clone()).expect("fact-only AX text read");

  assert_eq!(serde_json::to_value(read).expect("serialize text read"), facts);
}
