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
