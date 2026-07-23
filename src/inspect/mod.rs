//! Canonical root run inspection.

use auv_tracing::{RunSnapshot, RunStore};

use crate::contract::{ObservationSnapshot, ObservationSource, RecognitionSource};
use crate::run_read::{ScrollScanReadError, list_detector_recognition_lineage, list_input_action_results, read_scroll_scan};
use crate::scroll_scan::SCROLL_SCAN_PURPOSE;

pub async fn inspect_scroll_scan_observations_v1(store: &dyn RunStore, snapshot: &RunSnapshot) -> Result<String, ScrollScanReadError> {
  if snapshot.authority_id() != store.authority_id() {
    return Err(
      crate::run_read::RootArtifactReadError::SnapshotAuthorityMismatch {
        snapshot_authority: snapshot.authority_id(),
        store_authority: store.authority_id(),
      }
      .into(),
    );
  }
  let mut observations = Vec::new();
  for (uri, published) in snapshot.artifacts() {
    if published.metadata().purpose().as_str() == SCROLL_SCAN_PURPOSE {
      observations.extend(read_scroll_scan(store, snapshot, uri).await?.snapshots);
    }
  }
  Ok(render_observations_text(&observations))
}

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

fn render_observations_text(observation_snapshots: &[ObservationSnapshot]) -> String {
  let mut output = String::from("Observations:\n");
  if observation_snapshots.is_empty() {
    output.push_str("- none\n");
  } else {
    for snapshot in observation_snapshots {
      output.push_str(&format!(
        "- {} span={} source={} nodes={} evidence={} limits={}\n",
        snapshot.snapshot_id,
        snapshot.span_id,
        observation_source(snapshot.source),
        snapshot.nodes.len(),
        snapshot.evidence.len(),
        snapshot.known_limits.len()
      ));
    }
  }
  output
}

fn observation_source(source: ObservationSource) -> &'static str {
  match source {
    ObservationSource::Ax => "ax",
    ObservationSource::Ocr => "ocr",
    ObservationSource::Visual => "visual",
    ObservationSource::Merged => "merged",
  }
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
