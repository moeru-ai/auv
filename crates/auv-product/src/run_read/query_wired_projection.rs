//! Shared query-wired readiness / source-ref projection helpers (S3b).
//!
//! NOTICE(inspect-composition / S3b): Source-ref projection stays product-local
//! because it sits on the OperationResult adapter boundary. The neutral
//! eligibility-to-readiness mapping is owned by `auv-query-readiness` and shared
//! with ordinary game readers. Full file graduation into `auv-game-*` remains
//! blocked until the OperationResult types' ownership move is owner-approved.

pub(crate) use auv_query_readiness::map_action_eligibility_to_readiness_class;

/// NOTICE(core-c2-d2): reader-side provenance only — Core-C1 source_readiness_ref.
pub(crate) fn format_source_readiness_ref(parts: &[(&str, &str)]) -> String {
  parts.iter().filter(|(_, value)| !value.is_empty()).map(|(key, value)| format!("{key}={value}")).collect::<Vec<_>>().join(" ")
}

pub(crate) fn format_query_manifest_source_readiness_ref(artifact_id: &str, run_id: &str) -> String {
  format_source_readiness_ref(&[
    ("kind", "query_manifest"),
    ("artifact_id", artifact_id),
    ("run_id", run_id),
  ])
}

pub(crate) fn format_derived_readiness_source_readiness_ref(query_artifact_id: &str, run_id: &str) -> String {
  format_source_readiness_ref(&[
    ("kind", "derived_readiness"),
    ("query_artifact_id", query_artifact_id),
    ("run_id", run_id),
  ])
}

pub(crate) fn format_outcome_event_source_readiness_ref(event_name: &str, operation_result_artifact_id: Option<&str>) -> String {
  let mut parts = vec![("kind", "outcome_event"), ("event", event_name)];
  if let Some(operation_result_artifact_id) = operation_result_artifact_id.filter(|artifact_id| !artifact_id.is_empty()) {
    parts.push(("operation_result_artifact_id", operation_result_artifact_id));
  }
  format_source_readiness_ref(&parts)
}

pub(crate) enum SourceReadinessManifestLookup {
  MatchedValidManifest { artifact_id: String },
  CleanMiss,
  MatchedParseFailure,
}

/// Shared minecraft/osu manifest lookup classification for source_readiness_ref.
///
/// Donor-specific extract/list types stay at call sites; only the readiness
/// projection decision is shared here.
pub(crate) fn classify_manifest_source_readiness_lookup<T, E>(
  query_id: &str,
  extract_result: &Result<Vec<T>, E>,
  artifact_id: impl Fn(&T) -> &str,
  has_parsed_manifest: impl Fn(&T) -> bool,
) -> Option<SourceReadinessManifestLookup> {
  match extract_result {
    Err(_) => None,
    Ok(manifests) => {
      let matching = manifests.iter().find(|manifest| artifact_id(manifest) == query_id);
      Some(match matching {
        None => SourceReadinessManifestLookup::CleanMiss,
        Some(lineage) if has_parsed_manifest(lineage) => SourceReadinessManifestLookup::MatchedValidManifest {
          artifact_id: artifact_id(lineage).to_string(),
        },
        Some(_) => SourceReadinessManifestLookup::MatchedParseFailure,
      })
    }
  }
}

pub(crate) fn resolve_query_wired_live_action_source_readiness_ref(
  run_id: &str,
  query_artifact_id: Option<&str>,
  operation_result_artifact_id: Option<&str>,
  outcome_event_name: &str,
  has_outcome_event: bool,
  manifest_lookup: Option<SourceReadinessManifestLookup>,
) -> Option<String> {
  if let Some(query_id) = query_artifact_id {
    return match manifest_lookup? {
      SourceReadinessManifestLookup::MatchedValidManifest { artifact_id } => {
        Some(format_query_manifest_source_readiness_ref(artifact_id.as_str(), run_id))
      }
      SourceReadinessManifestLookup::CleanMiss => Some(format_derived_readiness_source_readiness_ref(query_id, run_id)),
      SourceReadinessManifestLookup::MatchedParseFailure => None,
    };
  }
  if has_outcome_event {
    return Some(format_outcome_event_source_readiness_ref(outcome_event_name, operation_result_artifact_id));
  }
  None
}
