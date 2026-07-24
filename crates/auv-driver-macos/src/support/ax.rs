use crate::types::{ObservedAxNode, ObservedAxTreeSnapshot};

pub fn find_best_ax_node<'a>(snapshot: &'a ObservedAxTreeSnapshot, query: &str) -> Option<&'a ObservedAxNode> {
  let query = query.trim().to_lowercase();
  snapshot
    .nodes
    .iter()
    .filter(|node| node.bounds.width > 0 && node.bounds.height > 0)
    .filter_map(|node| score_ax_node_match(node, &query).map(|score| (score, node)))
    .max_by(|left, right| left.0.cmp(&right.0))
    .map(|(_, node)| node)
}

pub fn find_now_playing_ax_node<'a>(
  snapshot: &'a ObservedAxTreeSnapshot,
  expected_title: &str,
  expected_artist: Option<&str>,
  scope_path_prefix: Option<&str>,
) -> Option<&'a ObservedAxNode> {
  let expected_title = expected_title.trim().to_lowercase();
  if expected_title.is_empty() {
    return None;
  }
  let expected_artist = expected_artist.map(|value| value.trim().to_lowercase()).filter(|value| !value.is_empty());
  let scope_path_prefix = scope_path_prefix.map(|value| value.trim().to_string()).filter(|value| !value.is_empty());

  snapshot
    .nodes
    .iter()
    .filter(|node| node.bounds.width > 0 && node.bounds.height > 0)
    .filter(|node| scope_path_prefix.as_ref().is_none_or(|prefix| node.path.starts_with(prefix)))
    .filter_map(|node| score_now_playing_ax_node_match(node, &expected_title, expected_artist.as_deref()).map(|score| (score, node)))
    .max_by(|left, right| left.0.cmp(&right.0))
    .map(|(_, node)| node)
}

pub fn ax_node_search_text(node: &ObservedAxNode) -> String {
  let searchable = [
    node.title.as_str(),
    node.description.as_str(),
    node.help.as_str(),
    node.identifier.as_str(),
    node.placeholder.as_str(),
    node.value.as_str(),
  ]
  .into_iter()
  .filter_map(|value| {
    let trimmed = value.trim();
    if trimmed.is_empty() {
      None
    } else {
      Some(trimmed)
    }
  })
  .collect::<Vec<_>>()
  .join(" ");
  normalize_ax_text(&searchable)
}

fn normalize_ax_text(value: &str) -> String {
  value.chars().filter(|character| !character.is_whitespace()).collect::<String>().to_lowercase()
}

fn score_now_playing_ax_node_match(node: &ObservedAxNode, expected_title: &str, expected_artist: Option<&str>) -> Option<i64> {
  let searchable = ax_node_search_text(node);
  if !searchable.contains(expected_title) {
    return None;
  }
  if let Some(expected_artist) = expected_artist
    && !searchable.contains(expected_artist)
  {
    return None;
  }

  let mut score = 100 - node.depth as i64;
  if node.title.to_lowercase().contains(expected_title) {
    score += 40;
  }
  if let Some(expected_artist) = expected_artist
    && node.title.to_lowercase().contains(expected_artist)
  {
    score += 20;
  }
  if node.role == "AXUnknown" || node.role == "AXStaticText" {
    score += 10;
  }
  if node.subrole == "AXStaticText" || node.subrole == "AXTextField" {
    score += 6;
  }

  Some(score)
}

pub fn no_matching_ax_node_error(snapshot: &ObservedAxTreeSnapshot, query: &str, expected_kind: &str) -> String {
  if snapshot.nodes.len() <= 1 {
    return format!(
      "no matching {expected_kind} node found for query {query}; observed only {} AX node(s), so the target UI may need to be revealed before retrying",
      snapshot.nodes.len()
    );
  }
  format!("no matching {expected_kind} node found for query {query}")
}

pub fn score_ax_node_match(node: &ObservedAxNode, query: &str) -> Option<i64> {
  if query.is_empty() {
    return None;
  }

  let fields = [
    ("title", node.title.as_str()),
    ("description", node.description.as_str()),
    ("help", node.help.as_str()),
    ("identifier", node.identifier.as_str()),
    ("placeholder", node.placeholder.as_str()),
    ("value", node.value.as_str()),
  ];

  let mut score = 0i64;
  for (label, raw_value) in fields {
    let value = raw_value.trim().to_lowercase();
    if value.is_empty() || !value.contains(query) {
      continue;
    }

    score += match label {
      "title" => 80,
      "description" => 72,
      "placeholder" => 64,
      "help" => 56,
      "identifier" => 40,
      _ => 24,
    };
    if value == query {
      score += 20;
    }
  }

  if score == 0 {
    return None;
  }

  if node.role == "AXTextField" || node.subrole == "AXSearchField" {
    score += 24;
  }
  if node.role == "AXButton" || node.role == "AXLink" {
    score += 18;
  }
  if node.role == "AXUnknown" {
    score += 8;
  }

  Some(score - node.depth as i64)
}

pub fn ax_node_center(node: &ObservedAxNode) -> (f64, f64) {
  (node.bounds.x as f64 + (node.bounds.width as f64 / 2.0), node.bounds.y as f64 + (node.bounds.height as f64 / 2.0))
}

/// Find the AX node best matching a screen point — used by OCR→AX pipelines
/// where the caller has a (x, y) anchor and wants the pressable control there.
///
/// Strategy: among nodes whose bounds contain the point, score by depth
/// (deeper wins, so we land on the actual control rather than its container)
/// plus a role bias toward pressable roles (AXButton, AXCheckBox, AXLink,
/// AXMenuItem). The role bias breaks ties when a button is wrapped by a
/// same-sized group element with no role of its own.
pub fn find_ax_node_at_point(snapshot: &ObservedAxTreeSnapshot, x: f64, y: f64) -> Option<&ObservedAxNode> {
  snapshot
    .nodes
    .iter()
    .filter(|node| node.bounds.width > 0 && node.bounds.height > 0)
    .filter(|node| ax_node_contains_point(node, x, y))
    .max_by_key(|node| score_ax_node_at_point(node))
}

fn ax_node_contains_point(node: &ObservedAxNode, x: f64, y: f64) -> bool {
  let left = node.bounds.x as f64;
  let top = node.bounds.y as f64;
  let right = left + node.bounds.width as f64;
  let bottom = top + node.bounds.height as f64;
  x >= left && x <= right && y >= top && y <= bottom
}

fn score_ax_node_at_point(node: &ObservedAxNode) -> i64 {
  let mut score = node.depth as i64 * 10;
  match node.role.as_str() {
    "AXButton" | "AXCheckBox" | "AXLink" | "AXMenuItem" | "AXMenuButton" => score += 50,
    "AXRadioButton" | "AXPopUpButton" => score += 40,
    "AXStaticText" => score += 20,
    "AXGroup" | "AXUnknown" => score -= 10,
    _ => {}
  }
  score
}
