//! Stateful in-process session substrate.
//!
//! This module owns resource lifecycle for live-ish observation state:
//! provider reuse, observation handles, node lookup, action invalidation, and
//! event emission. It is deliberately not a daemon transport and not an agent.
//! Callers must still decide when to observe, act, and verify.

use std::collections::BTreeMap;
use std::sync::Arc;

use auv_driver::InputActionResult;
use auv_tracing_driver::now_millis;
use auv_tracing_driver::trace::{DeviceId, SessionId};

use crate::contract::{ObservationSnapshot, SurfaceNode, VerificationResult};
use crate::model::AuvResult;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderId(String);

impl ProviderId {
  pub fn new(value: impl Into<String>) -> Self {
    Self(value.into())
  }

  pub fn as_str(&self) -> &str {
    &self.0
  }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObservationHandle {
  id: String,
}

impl ObservationHandle {
  pub fn as_str(&self) -> &str {
    &self.id
  }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeHandle {
  observation_id: String,
  node_id: String,
}

impl NodeHandle {
  pub fn observation_id(&self) -> &str {
    &self.observation_id
  }

  pub fn node_id(&self) -> &str {
    &self.node_id
  }
}

#[derive(Clone, Debug)]
pub struct SessionOptions {
  pub device_id: DeviceId,
  pub session_id: SessionId,
}

impl Default for SessionOptions {
  fn default() -> Self {
    Self {
      device_id: DeviceId::default_local(),
      session_id: SessionId::default_session(),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StaleReason {
  ActionInvalidated { action_label: String },
}

impl StaleReason {
  pub fn as_str(&self) -> &str {
    match self {
      Self::ActionInvalidated { .. } => "action_invalidated",
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObservationResource {
  pub handle: ObservationHandle,
  pub provider_id: ProviderId,
  pub version: u64,
  pub snapshot: ObservationSnapshot,
  pub stale_reason: Option<StaleReason>,
}

impl ObservationResource {
  pub fn is_stale(&self) -> bool {
    self.stale_reason.is_some()
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NodeResource {
  pub handle: NodeHandle,
  pub observation_version: u64,
  pub node: SurfaceNode,
  pub stale_reason: Option<StaleReason>,
}

impl NodeResource {
  pub fn is_stale(&self) -> bool {
    self.stale_reason.is_some()
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct VerificationResource {
  pub verification_id: String,
  pub result: VerificationResult,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SessionEvent {
  SessionOpened {
    session_id: SessionId,
    device_id: DeviceId,
  },
  ProviderInitialized {
    provider_id: ProviderId,
  },
  ObservationCaptured {
    observation_id: String,
    provider_id: ProviderId,
    version: u64,
    node_count: usize,
  },
  ResourceInvalidated {
    observation_id: String,
    reason: StaleReason,
  },
  ActionFinished {
    action_label: String,
    result: InputActionResult,
  },
  VerificationRecorded {
    verification_id: String,
  },
  SessionClosed {
    session_id: SessionId,
  },
}

pub trait SessionObservationProvider {
  fn provider_id(&self) -> ProviderId;

  fn observe(&mut self, request: &ObserveRequest) -> AuvResult<ObservationSnapshot>;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObserveRequest {
  pub label_filter: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActRequest {
  pub label: String,
  pub invalidate_observations: bool,
}

impl ActRequest {
  pub fn new(label: impl Into<String>) -> Self {
    Self {
      label: label.into(),
      invalidate_observations: true,
    }
  }
}

pub struct SessionRuntime {
  options: SessionOptions,
  next_observation_sequence: u64,
  providers: BTreeMap<ProviderId, Box<dyn SessionObservationProvider>>,
  observations: BTreeMap<String, ObservationResource>,
  observation_order: Vec<String>,
  verifications: BTreeMap<String, VerificationResource>,
  events: Vec<SessionEvent>,
}

impl SessionRuntime {
  pub fn new(options: SessionOptions) -> Self {
    let events = vec![SessionEvent::SessionOpened {
      session_id: options.session_id.clone(),
      device_id: options.device_id.clone(),
    }];
    Self {
      options,
      next_observation_sequence: 0,
      providers: BTreeMap::new(),
      observations: BTreeMap::new(),
      observation_order: Vec::new(),
      verifications: BTreeMap::new(),
      events,
    }
  }

  pub fn session_id(&self) -> &SessionId {
    &self.options.session_id
  }

  pub fn device_id(&self) -> &DeviceId {
    &self.options.device_id
  }

  pub fn events(&self) -> &[SessionEvent] {
    &self.events
  }

  pub fn provider_count(&self) -> usize {
    self.providers.len()
  }

  pub fn register_provider<P>(&mut self, provider: P) -> ProviderId
  where
    P: SessionObservationProvider + 'static,
  {
    let provider_id = provider.provider_id();
    if !self.providers.contains_key(&provider_id) {
      self.events.push(SessionEvent::ProviderInitialized {
        provider_id: provider_id.clone(),
      });
    }
    self
      .providers
      .insert(provider_id.clone(), Box::new(provider));
    provider_id
  }

  pub fn observe(
    &mut self,
    provider_id: &ProviderId,
    request: ObserveRequest,
  ) -> AuvResult<ObservationResource> {
    let provider = self.providers.get_mut(provider_id).ok_or_else(|| {
      format!(
        "unknown session observation provider {}",
        provider_id.as_str()
      )
    })?;
    let mut snapshot = provider.observe(&request)?;
    let sequence = self.next_observation_sequence;
    self.next_observation_sequence += 1;
    let observation_id = format!(
      "obs_{}_{}",
      self
        .options
        .session_id
        .as_str()
        .replace(|c: char| !c.is_ascii_alphanumeric(), "_"),
      sequence
    );
    snapshot.snapshot_id = observation_id.clone();
    if snapshot.captured_at_millis == 0 {
      snapshot.captured_at_millis = now_millis();
    }
    let version = 1;
    let handle = ObservationHandle {
      id: observation_id.clone(),
    };
    let resource = ObservationResource {
      handle,
      provider_id: provider_id.clone(),
      version,
      snapshot,
      stale_reason: None,
    };
    self.events.push(SessionEvent::ObservationCaptured {
      observation_id: observation_id.clone(),
      provider_id: provider_id.clone(),
      version,
      node_count: resource.snapshot.nodes.len(),
    });
    self.observation_order.push(observation_id.clone());
    self.observations.insert(observation_id, resource.clone());
    Ok(resource)
  }

  pub fn observation(&self, handle: &ObservationHandle) -> Option<&ObservationResource> {
    self.observations.get(handle.as_str())
  }

  pub fn find_node_by_label(&self, label: &str) -> Option<NodeResource> {
    self
      .observation_order
      .iter()
      .rev()
      .filter_map(|observation_id| self.observations.get(observation_id))
      .find_map(|observation| {
        observation.snapshot.nodes.iter().find_map(move |node| {
          (node.label.as_deref()? == label).then(|| NodeResource {
            handle: NodeHandle {
              observation_id: observation.handle.id.clone(),
              node_id: node.node_ref.node_id.clone(),
            },
            observation_version: observation.version,
            node: node.clone(),
            stale_reason: observation.stale_reason.clone(),
          })
        })
      })
  }

  pub fn act_with_result(
    &mut self,
    request: ActRequest,
    result: InputActionResult,
  ) -> InputActionResult {
    if request.invalidate_observations {
      let reason = StaleReason::ActionInvalidated {
        action_label: request.label.clone(),
      };
      for (observation_id, observation) in &mut self.observations {
        observation.version += 1;
        observation.stale_reason = Some(reason.clone());
        self.events.push(SessionEvent::ResourceInvalidated {
          observation_id: observation_id.clone(),
          reason: reason.clone(),
        });
      }
    }
    self.events.push(SessionEvent::ActionFinished {
      action_label: request.label,
      result: result.clone(),
    });
    result
  }

  pub fn verify_with_result(&mut self, result: VerificationResult) -> VerificationResource {
    let verification_id = format!(
      "verify_{}_{}",
      self
        .options
        .session_id
        .as_str()
        .replace(|c: char| !c.is_ascii_alphanumeric(), "_"),
      self.verifications.len()
    );
    let resource = VerificationResource {
      verification_id: verification_id.clone(),
      result,
    };
    self
      .verifications
      .insert(verification_id.clone(), resource.clone());
    self
      .events
      .push(SessionEvent::VerificationRecorded { verification_id });
    resource
  }

  pub fn verification(&self, verification_id: &str) -> Option<&VerificationResource> {
    self.verifications.get(verification_id)
  }

  pub fn close(&mut self) {
    self.events.push(SessionEvent::SessionClosed {
      session_id: self.options.session_id.clone(),
    });
  }
}

#[cfg(test)]
fn synthetic_snapshot(nodes: Vec<SurfaceNode>) -> ObservationSnapshot {
  ObservationSnapshot {
    api_version: crate::contract::OBSERVATION_SNAPSHOT_API_VERSION.to_string(),
    snapshot_id: "pending_session_observation".to_string(),
    run_id: auv_tracing_driver::trace::new_run_id(),
    span_id: auv_tracing_driver::trace::new_span_id(),
    captured_at_millis: now_millis(),
    source: crate::contract::ObservationSource::Visual,
    scope: crate::contract::RecognitionScope {
      surface: crate::contract::RecognitionSurface::Window,
      display_ref: None,
      native_display_id: None,
      app_bundle_id: None,
      window_title: None,
      window_number: None,
      region_hint: None,
      capture_artifact: None,
      capture_contract_artifact: None,
    },
    capture_contract_ref: None,
    evidence: Vec::new(),
    nodes,
    detail: serde_json::json!({ "producer": "session.synthetic_snapshot" }),
    known_limits: vec![
      "session synthetic snapshot has no durable capture artifact in v0".to_string(),
    ],
  }
}

#[cfg(test)]
fn synthetic_surface_node(
  node_id: impl Into<String>,
  label: impl Into<String>,
  box_: crate::contract::RecognitionBox,
) -> SurfaceNode {
  let node_id = node_id.into();
  SurfaceNode {
    node_ref: crate::contract::NodeRef {
      run_id: auv_tracing_driver::trace::RunId::new("run_session_synthetic"),
      span_id: auv_tracing_driver::trace::SpanId::new("span_session_synthetic"),
      node_id,
    },
    kind: "session_fixture_node".to_string(),
    label: Some(label.into()),
    box_,
    source_artifacts: Vec::new(),
    recognition_id: None,
    recognition_source: Some(crate::contract::RecognitionSource::VisualRow),
    recognition_surface: Some(crate::contract::RecognitionSurface::Window),
    recognized_item_id: None,
    recognized_item_kind: None,
    provider_score: None,
    detail: serde_json::json!({ "producer": "session.synthetic_surface_node" }),
  }
}

// TODO(session-daemon-transport): expose this resource table through a local
// daemon only after the in-process semantics have another real consumer.
// TODO(session-agent-boundary): this module must stay request-driven. Do not add
// a scheduler that observes, decides, and acts without an external caller.

#[derive(Clone)]
pub(crate) struct FixtureObservationProvider {
  provider_id: ProviderId,
  snapshots: Arc<Vec<ObservationSnapshot>>,
  observe_count: usize,
}

impl FixtureObservationProvider {
  pub(crate) fn new(provider_id: impl Into<String>, snapshots: Vec<ObservationSnapshot>) -> Self {
    Self {
      provider_id: ProviderId::new(provider_id),
      snapshots: Arc::new(snapshots),
      observe_count: 0,
    }
  }
}

impl SessionObservationProvider for FixtureObservationProvider {
  fn provider_id(&self) -> ProviderId {
    self.provider_id.clone()
  }

  fn observe(&mut self, _request: &ObserveRequest) -> AuvResult<ObservationSnapshot> {
    let index = self
      .observe_count
      .min(self.snapshots.len().saturating_sub(1));
    self.observe_count += 1;
    self
      .snapshots
      .get(index)
      .cloned()
      .ok_or_else(|| "fixture observation provider has no snapshots".to_string())
  }
}

#[cfg(test)]
mod tests {
  use auv_driver::InputDeliveryPath;

  use super::*;

  #[test]
  fn session_reuses_provider_and_answers_lookup() {
    let mut session = SessionRuntime::new(SessionOptions::default());
    let provider = FixtureObservationProvider::new(
      "fixture.visual",
      vec![synthetic_snapshot(vec![synthetic_surface_node(
        "node_hit_circle",
        "hit_circle",
        crate::contract::RecognitionBox {
          x: 10,
          y: 20,
          width: 30,
          height: 40,
        },
      )])],
    );
    let provider_id = session.register_provider(provider);

    let first = session
      .observe(&provider_id, ObserveRequest::default())
      .expect("first observe should succeed");
    let second = session
      .observe(&provider_id, ObserveRequest::default())
      .expect("second observe should reuse provider");

    assert_eq!(session.provider_count(), 1);
    assert_eq!(first.snapshot.nodes.len(), 1);
    assert_eq!(second.snapshot.nodes.len(), 1);
    assert!(matches!(
      session.events()[1],
      SessionEvent::ProviderInitialized { .. }
    ));

    let node = session
      .find_node_by_label("hit_circle")
      .expect("node should be lookup-addressable");
    assert_eq!(node.node.label.as_deref(), Some("hit_circle"));
    assert!(!node.is_stale());
  }

  #[test]
  fn action_result_invalidates_observations_without_new_result_schema() {
    let mut session = SessionRuntime::new(SessionOptions::default());
    let provider_id = session.register_provider(FixtureObservationProvider::new(
      "fixture.visual",
      vec![synthetic_snapshot(vec![synthetic_surface_node(
        "node_play",
        "Play",
        crate::contract::RecognitionBox {
          x: 0,
          y: 0,
          width: 10,
          height: 10,
        },
      )])],
    ));
    let observation = session
      .observe(&provider_id, ObserveRequest::default())
      .expect("observe should succeed");

    let result = session.act_with_result(
      ActRequest::new("click Play"),
      InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
    );

    assert_eq!(result.selected_path, InputDeliveryPath::WindowTargetedMouse);
    let stale_observation = session
      .observation(&observation.handle)
      .expect("observation should still be retained");
    assert!(stale_observation.is_stale());
    assert_eq!(stale_observation.version, 2);

    let stale_node = session
      .find_node_by_label("Play")
      .expect("stale node should still be lookup-addressable");
    assert!(stale_node.is_stale());
    assert!(session.events().iter().any(|event| {
      matches!(
        event,
        SessionEvent::ActionFinished { result, .. }
          if result.selected_path == InputDeliveryPath::WindowTargetedMouse
      )
    }));
  }

  #[test]
  fn verify_records_existing_verification_result_contract() {
    let mut session = SessionRuntime::new(SessionOptions::default());
    let verification = crate::contract::VerificationResult {
      api_version: crate::contract::VERIFICATION_RESULT_API_VERSION.to_string(),
      method: crate::contract::VerificationMethod::SemanticMatch,
      executed: true,
      state_changed: true,
      semantic_matched: Some(true),
      failure_layer: None,
      evidence: Vec::new(),
      consumed_candidate_ref: None,
      consumed_node_ref: None,
      consumed_recognition_artifact_ref: None,
      consumed_recognition_id: None,
      consumed_recognized_item_id: None,
      observed_label: Some("hit_circle".to_string()),
    };

    let resource = session.verify_with_result(verification);

    assert_eq!(resource.result.semantic_matched, Some(true));
    assert_eq!(
      session
        .verification(&resource.verification_id)
        .expect("verification should be retained")
        .result
        .observed_label
        .as_deref(),
      Some("hit_circle")
    );
    assert!(matches!(
      session.events().last(),
      Some(SessionEvent::VerificationRecorded { .. })
    ));
  }

  #[test]
  fn close_emits_session_closed_event() {
    let mut session = SessionRuntime::new(SessionOptions::default());
    session.close();

    assert!(matches!(
      session.events().last(),
      Some(SessionEvent::SessionClosed { .. })
    ));
  }
}
