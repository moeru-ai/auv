use auv_driver::Rect;
use serde::{Deserialize, Serialize};

use crate::app::LabelSet;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SettingsNode {
  pub path: String,
  pub role: String,
  pub name: String,
  pub value: Option<String>,
  pub bounds: Rect,
}

impl SettingsNode {
  pub fn label(&self) -> Option<&str> {
    if !self.name.trim().is_empty() {
      Some(self.name.as_str())
    } else {
      self.value.as_deref().filter(|value| !value.trim().is_empty())
    }
  }

  pub fn is_actionable(&self) -> bool {
    let role = self.role.to_ascii_lowercase();
    role.contains("button") || role.contains("menu") || role.contains("list") || role.contains("page") || role.contains("radio")
  }

  pub fn is_slider(&self) -> bool {
    let role = self.role.to_ascii_lowercase();
    role.contains("slider")
  }

  pub fn is_switch(&self) -> bool {
    let role = self.role.to_ascii_lowercase();
    role.contains("switch") || role.contains("toggle")
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MatchedNode {
  pub path: String,
  pub label: String,
  pub matched_label: String,
  pub role: String,
  pub bounds: Rect,
  pub value: Option<String>,
}

pub fn find_labeled_node(nodes: &[SettingsNode], labels: LabelSet) -> Option<MatchedNode> {
  nodes
    .iter()
    .filter_map(|node| {
      let label = node.label()?;
      let matched_label = labels.best_match(label)?;
      Some((node.is_actionable(), node, label, matched_label))
    })
    .max_by_key(|(actionable, node, _, _)| (*actionable, std::cmp::Reverse(node.path.len())))
    .map(|(_, node, label, matched_label)| matched_node(node, label, matched_label))
}

pub fn find_slider_near_label(nodes: &[SettingsNode], labels: LabelSet) -> Option<MatchedNode> {
  let label_node = find_labeled_node(nodes, labels)?;
  let label_center_y = label_node.bounds.center().y;
  nodes
    .iter()
    .filter(|node| node.is_slider())
    .min_by(|left, right| {
      let left_distance = (left.bounds.center().y - label_center_y).abs();
      let right_distance = (right.bounds.center().y - label_center_y).abs();
      left_distance.total_cmp(&right_distance)
    })
    .map(|node| {
      let label = node.label().unwrap_or("slider");
      matched_node(node, label, "slider")
    })
}

pub fn find_switch_near_label(nodes: &[SettingsNode], labels: LabelSet) -> Option<MatchedNode> {
  let label_node = find_labeled_node(nodes, labels)?;
  let label_center_y = label_node.bounds.center().y;
  nodes
    .iter()
    .filter(|node| node.is_switch())
    .min_by(|left, right| {
      let left_distance = (left.bounds.center().y - label_center_y).abs();
      let right_distance = (right.bounds.center().y - label_center_y).abs();
      left_distance.total_cmp(&right_distance)
    })
    .map(|node| {
      let label = node.label().unwrap_or("switch");
      matched_node(node, label, "switch")
    })
}

pub fn visible_labels(nodes: &[SettingsNode]) -> Vec<String> {
  nodes.iter().filter_map(SettingsNode::label).map(str::trim).filter(|label| !label.is_empty()).map(ToOwned::to_owned).collect()
}

fn matched_node(node: &SettingsNode, label: &str, matched_label: &str) -> MatchedNode {
  MatchedNode {
    path: node.path.clone(),
    label: label.to_string(),
    matched_label: matched_label.to_string(),
    role: node.role.clone(),
    bounds: node.bounds,
    value: node.value.clone(),
  }
}

#[cfg(target_os = "linux")]
impl From<&auv_driver_linux::AxNode> for SettingsNode {
  fn from(node: &auv_driver_linux::AxNode) -> Self {
    Self {
      path: node.path.clone(),
      role: node.control_type.clone(),
      name: node.name.clone(),
      value: node.value.clone(),
      bounds: node.bounds,
    }
  }
}

#[cfg(test)]
mod tests {
  use auv_driver::Rect;

  use super::{SettingsNode, find_labeled_node, find_slider_near_label, find_switch_near_label, visible_labels};
  use crate::app::LabelSet;

  fn node(path: &str, role: &str, name: &str, value: Option<&str>, bounds: Rect) -> SettingsNode {
    SettingsNode {
      path: path.to_string(),
      role: role.to_string(),
      name: name.to_string(),
      value: value.map(ToOwned::to_owned),
      bounds,
    }
  }

  #[test]
  fn label_uses_value_when_accessible_name_is_empty() {
    let node = node("/switch", "switch", "", Some(" Enabled "), Rect::new(0.0, 0.0, 10.0, 10.0));

    assert_eq!(node.label(), Some(" Enabled "));
    assert!(node.is_switch());
  }

  #[test]
  fn find_labeled_node_prefers_actionable_nodes() {
    let nodes = vec![
      node("/text", "text", "System", None, Rect::new(0.0, 0.0, 10.0, 10.0)),
      node("/button", "button", "System", None, Rect::new(0.0, 20.0, 10.0, 10.0)),
    ];

    let matched = find_labeled_node(&nodes, LabelSet::new(&["System"])).expect("system node should match");

    assert_eq!(matched.path, "/button");
    assert_eq!(matched.matched_label, "System");
  }

  #[test]
  fn nearby_controls_and_visible_labels_are_derived_from_snapshot() {
    let nodes = vec![
      node("/label", "label", "Pointer Speed", None, Rect::new(0.0, 100.0, 20.0, 20.0)),
      node("/slider-far", "slider", "", None, Rect::new(0.0, 180.0, 100.0, 20.0)),
      node("/slider-near", "slider", "", None, Rect::new(0.0, 110.0, 100.0, 20.0)),
      node("/switch", "switch", "Natural", None, Rect::new(0.0, 140.0, 20.0, 20.0)),
      node("/value", "label", "", Some(" Enabled "), Rect::new(0.0, 160.0, 20.0, 20.0)),
    ];

    let pointer_speed = LabelSet::new(&["Pointer Speed"]);
    let natural = LabelSet::new(&["Natural"]);
    let slider = find_slider_near_label(&nodes, pointer_speed).expect("pointer speed slider should match");
    let switch = find_switch_near_label(&nodes, natural).expect("natural switch should match");

    assert_eq!(slider.path, "/slider-near");
    assert_eq!(switch.path, "/switch");
    assert_eq!(visible_labels(&nodes), vec!["Pointer Speed", "Natural", "Enabled"]);
  }
}
