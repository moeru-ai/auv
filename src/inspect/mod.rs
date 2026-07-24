//! Canonical root run inspection.

use auv_tracing::{RunSnapshot, RunStore};

use crate::contract::RecognitionSource;
use crate::run_read::{list_detector_recognition_lineage, list_input_action_results};

pub async fn inspect_run_core_prefix_body(store: &dyn RunStore, snapshot: &RunSnapshot) -> Result<String, String> {
  let input_actions = list_input_action_results(store, snapshot).await.map_err(|error| error.to_string())?;
  let detector_lineage = list_detector_recognition_lineage(store, snapshot).await.map_err(|error| error.to_string())?;
  let mut output = format!(
    "Run {}\nRevision: {}\n\nSpans: {}\nEvents: {}\nArtifacts: {}\n",
    snapshot.run_id(),
    snapshot.through_revision().get(),
    snapshot.spans().len(),
    snapshot.events().len(),
    snapshot.artifacts().len()
  );
  output.push_str("\nInput actions:\n");
  if input_actions.is_empty() {
    output.push_str("- none\n");
  } else {
    for action in input_actions {
      output.push_str(&format!("- selected={:?} attempts={}\n", action.selected_path, action.attempts.len()));
    }
  }
  output.push_str("\nDetector recognition lineage:\n");
  if detector_lineage.is_empty() {
    output.push_str("- none\n");
  } else {
    for lineage in detector_lineage {
      output.push_str(&format!(
        "- artifact={} recognition={} source={} items={}/{} best={}\n",
        lineage.artifact_uri,
        lineage.recognition_id,
        recognition_source(lineage.source),
        lineage.filtered_count,
        lineage.all_count,
        lineage.best_item_id.as_deref().unwrap_or("n/a")
      ));
    }
  }
  Ok(output)
}

pub async fn inspect_run_core_suffix_body(store: &dyn RunStore, snapshot: &RunSnapshot) -> Result<String, String> {
  let scene = crate::scene_state_read::build_scene_state_inspect_for_run(store, snapshot).await.map_err(|error| error.to_string())?;
  Ok(crate::scene_state_read::format_scene_state_read_text(&scene))
}

fn recognition_source(source: RecognitionSource) -> &'static str {
  match source {
    RecognitionSource::OcrText => "ocr_text",
    RecognitionSource::OcrRow => "ocr_row",
    RecognitionSource::VisualRow => "visual_row",
    RecognitionSource::SegmentedRegion => "segmented_region",
    RecognitionSource::IconMatch => "icon_match",
    RecognitionSource::Custom => "custom",
  }
}
