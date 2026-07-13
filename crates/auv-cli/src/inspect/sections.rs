//! Product inspect sections assembled from app-owned and product-owned readers.
//!
//! Query-wired sections remain product-owned because they depend on
//! `OperationResult` adapters. Ordinary app-specific sections are supplied by
//! `auv-game-*` factories.
//!
//! Product CLI, product MCP, and the product inspect-server projection inject
//! the same composer from `build_product_inspect_composer`. Viewer app-specific
//! cards still fetch named JSON extensions (e.g. quality baseline) by key, not
//! first-class Minecraft routes.

use std::sync::Arc;

use auv_inspect_model::{InspectComposer, InspectError, InspectSection, InspectSectionOutput};
use auv_runtime::inspect::{CorePrefixSection, CoreSuffixSection};
use auv_tracing_driver::store::{CanonicalRun, LocalStore};

use super::query_wired_minecraft::append_minecraft_query_wired_section;
use super::query_wired_osu::append_osu_query_wired_section;
use crate::run_read::{list_minecraft_query_wired_live_action_summaries, list_osu_query_wired_live_action_summaries};

pub struct OsuQueryWiredLiveActionSection;

impl InspectSection for OsuQueryWiredLiveActionSection {
  fn id(&self) -> &'static str {
    "osu_query_wired_live_action"
  }

  fn collect(&self, store: &LocalStore, run: &CanonicalRun) -> Result<Option<InspectSectionOutput>, InspectError> {
    let summaries = list_osu_query_wired_live_action_summaries(store, run.run.run_id.as_str())?;
    let mut text = String::new();
    append_osu_query_wired_section(&mut text, &summaries);
    Ok(Some(InspectSectionOutput {
      id: self.id(),
      text,
      json: None,
    }))
  }
}

pub struct MinecraftQueryWiredLiveActionSection;

impl InspectSection for MinecraftQueryWiredLiveActionSection {
  fn id(&self) -> &'static str {
    "minecraft_query_wired_live_action"
  }

  fn collect(&self, store: &LocalStore, run: &CanonicalRun) -> Result<Option<InspectSectionOutput>, InspectError> {
    let summaries = list_minecraft_query_wired_live_action_summaries(store, run.run.run_id.as_str())?;
    let mut text = String::new();
    append_minecraft_query_wired_section(&mut text, &summaries);
    Ok(Some(InspectSectionOutput {
      id: self.id(),
      text,
      json: None,
    }))
  }
}

/// LOCKED golden render order (do not invent another).
pub fn build_product_inspect_composer() -> Result<Arc<InspectComposer>, InspectError> {
  let mut sections: Vec<Arc<dyn InspectSection>> = Vec::new();
  // 1. core_prefix
  sections.push(Arc::new(CorePrefixSection));
  // 2. minecraft primary
  sections.extend(auv_game_minecraft::inspect_sections_primary());
  // 3. balatro
  sections.extend(auv_game_balatro::inspect_sections());
  // 4. minecraft quality+spatial
  sections.extend(auv_game_minecraft::inspect_sections_quality_spatial());
  // 5. osu A
  sections.extend(auv_game_osu::inspect_sections_primary());
  // 6. osu query-wired (PRODUCT)
  sections.push(Arc::new(OsuQueryWiredLiveActionSection));
  // 7. osu B
  sections.extend(auv_game_osu::inspect_sections_detection_eval());
  // 8. minecraft query-wired (PRODUCT)
  sections.push(Arc::new(MinecraftQueryWiredLiveActionSection));
  // 9. core_suffix
  sections.push(Arc::new(CoreSuffixSection));
  InspectComposer::try_new(sections).map(Arc::new)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn product_composer_keeps_locked_section_order() {
    let composer = build_product_inspect_composer().expect("product composer");
    let ids = composer.sections().iter().map(|section| section.id()).collect::<Vec<_>>();
    assert_eq!(
      ids,
      [
        "core_prefix",
        "minecraft_primary",
        "balatro_card_detection",
        "minecraft_quality_spatial",
        "osu_visual_truth_primary",
        "osu_query_wired_live_action",
        "osu_detection_eval",
        "minecraft_query_wired_live_action",
        "core_suffix",
      ]
    );
  }
}
