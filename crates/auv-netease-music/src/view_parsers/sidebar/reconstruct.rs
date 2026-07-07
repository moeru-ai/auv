use crate::*;

pub(crate) fn reconstruct_playlist_sidebar(
  app: ScanAppContext,
  window: ScanWindowContext,
  sidebar_region: ViewRegionRecord,
  observations: Vec<SidebarViewportObservation>,
) -> PlaylistSidebarScan {
  let sidebar_bounds = sidebar_region.bounds.unwrap_or_default();
  let ReconstructionOutput {
    root,
    anchor_index,
    landmark_index,
    sections,
    diagnostics,
    boundary,
  } = reconstruct(&NeteasePolicy, &observations, sidebar_bounds);

  PlaylistSidebarScan {
    schema_version: VIEW_IR_SCHEMA_VERSION.to_string(),
    app,
    window,
    sidebar_region,
    observations,
    reconstruction: ViewReconstructionRecord {
      root,
      anchor_index,
      landmark_index,
    },
    projection: PlaylistSidebarProjection { sections },
    boundary,
    interaction_events: Vec::new(),
    diagnostics,
    known_limits: Vec::new(),
  }
}

/// `ReconstructionPolicy` impl that injects NetEase's classification +
/// node/projection construction into the framework's generic reconstruct
/// loop. All node id formats, anchor id formats, the section-header /
/// item / unknown classification, the root container's domain_kind, and
/// the dedup diagnostic wording live here — `auv-view` knows none of it.
pub(crate) struct NeteasePolicy;

impl ReconstructionPolicy for NeteasePolicy {
  type Candidate = SidebarViewportCandidate;
  type SectionKey = (SidebarSectionKind, String);
  type SectionProjection = SidebarSection;
  type ItemProjection = PlaylistSidebarItem;
  type Observation = SidebarViewportObservation;

  fn candidates<'a>(&self, observation: &'a Self::Observation) -> impl Iterator<Item = &'a Self::Candidate> + 'a
  where
    Self::Candidate: 'a,
  {
    observation.candidates.iter()
  }

  fn classify(&self, candidate: &Self::Candidate) -> CandidateRole<Self::SectionKey> {
    let Some(label) = candidate.label.as_deref().map(str::trim) else {
      return CandidateRole::Unknown;
    };
    match candidate.kind {
      SidebarCandidateKind::SectionHeader => {
        let kind = SidebarSectionKind::from_label(label);
        CandidateRole::Header {
          section_key: (kind, normalize_identity(label)),
        }
      }
      SidebarCandidateKind::PlaylistItem | SidebarCandidateKind::NavigationItem => CandidateRole::Item {
        dedupe_key: normalize_identity(label),
      },
      SidebarCandidateKind::Unknown => CandidateRole::Unknown,
    }
  }

  fn build_section(&self, observation: &Self::Observation, candidate: &Self::Candidate) -> (ViewNodeRecord, Self::SectionProjection) {
    let label = candidate.label.as_deref().map(str::trim).unwrap_or_default();
    let kind = SidebarSectionKind::from_label(label);
    let section_id = format!("section.obs{}.{}.{}", observation.observation_index, candidate.id, slug(label));
    let node = section_node(&section_id, kind, label, candidate, observation);
    let projection = SidebarSection {
      id: section_id,
      kind,
      label: Some(label.to_string()),
      items: Vec::new(),
    };
    (node, projection)
  }

  fn build_unassigned_section(&self) -> (ViewNodeRecord, Self::SectionProjection) {
    let section_id = "section.unassigned".to_string();
    let node = ViewNodeRecord {
      id: section_id.clone(),
      kind: ViewNodeKind::Section,
      domain_kind: Some(SidebarSectionKind::Unknown.domain_kind().to_string()),
      layout: Some(ViewLayout::VStack),
      label: None,
      bounds: ViewBounds::default(),
      scrollable: None,
      anchors: Vec::new(),
      landmarks: Vec::new(),
      actions: vec![ViewAction::ObserveOnly],
      evidence: Vec::new(),
      children: Vec::new(),
    };
    let projection = SidebarSection {
      id: section_id,
      kind: SidebarSectionKind::Unknown,
      label: None,
      items: Vec::new(),
    };
    (node, projection)
  }

  fn build_item(
    &self,
    observation: &Self::Observation,
    candidate: &Self::Candidate,
    section: &Self::SectionProjection,
  ) -> (ViewNodeRecord, Self::ItemProjection) {
    let label = candidate.label.as_deref().map(str::trim).unwrap_or_default();
    let item_id = format!("item.obs{}.{}.{}", observation.observation_index, candidate.id, slug(label));
    let anchor_id = format!("anchor.{item_id}");
    let node = item_node(&item_id, &anchor_id, label, candidate, observation);
    let projection = PlaylistSidebarItem {
      id: item_id,
      label: label.to_string(),
      section_hint: Some(section.kind),
      confidence: candidate.confidence,
      candidate_id: Some(candidate.id.clone()),
      anchor_id: Some(anchor_id),
    };
    (node, projection)
  }

  fn append_item_to_section_projection(&self, section: &mut Self::SectionProjection, item: Self::ItemProjection) {
    section.items.push(item);
  }

  fn build_root(
    &self,
    sidebar_bounds: ViewBounds,
    boundary: ScrollBoundarySummary,
    section_children: Vec<ViewNodeRecord>,
  ) -> ViewNodeRecord {
    ViewNodeRecord {
      id: "root.sidebar".to_string(),
      kind: ViewNodeKind::Collection,
      domain_kind: Some("netease.sidebar_playlist_collection".to_string()),
      layout: Some(ViewLayout::VStack),
      label: None,
      bounds: sidebar_bounds,
      scrollable: Some(ViewScrollable {
        axis: ViewAxis::Vertical,
        boundary,
      }),
      anchors: Vec::new(),
      landmarks: Vec::new(),
      actions: vec![ViewAction::Scroll],
      evidence: Vec::new(),
      children: section_children,
    }
  }

  fn emit_dedup_diagnostic(&self, candidate: &Self::Candidate, section: &Self::SectionProjection) -> ParserDiagnostic {
    let label = candidate.label.as_deref().unwrap_or("");
    ParserDiagnostic {
      code: "deduplicated_item".to_string(),
      message: format!("deduplicated repeated sidebar item {label:?} in section {:?}", section.kind),
      node_id: Some(candidate.id.clone()),
    }
  }
}
