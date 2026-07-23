//! Product inspect sections assembled from canonical root and app readers.

use auv_tracing::{RunSnapshot, RunStore};

use super::query_wired_minecraft::{append_minecraft_query_wired_section, collect_minecraft_query_wired_live_action_summaries};
use super::query_wired_osu::{append_osu_query_wired_section, collect_osu_query_wired_live_action_summaries};

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ProductInspectSection {
  pub id: &'static str,
  pub text: String,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ProductInspectDocument {
  #[serde(flatten)]
  canonical: auv_inspect_model::InspectDocument,
  pub sections: Vec<ProductInspectSection>,
}

impl ProductInspectDocument {
  pub fn render_text(&self) -> String {
    let mut output = String::new();
    for section in &self.sections {
      output.push_str(&section.text);
      if !section.text.ends_with('\n') {
        output.push('\n');
      }
    }
    output
  }
}

#[derive(Debug)]
pub enum ProductInspectError {
  Root(String),
  Minecraft(auv_game_minecraft::MinecraftArtifactReadError),
  Balatro(auv_game_balatro::BalatroArtifactReadError),
  Osu(auv_game_osu::run_read::OsuArtifactReadError),
}

impl std::fmt::Display for ProductInspectError {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Root(error) => write!(formatter, "root inspection failed: {error}"),
      Self::Minecraft(error) => write!(formatter, "Minecraft inspection failed: {error}"),
      Self::Balatro(error) => write!(formatter, "Balatro inspection failed: {error}"),
      Self::Osu(error) => write!(formatter, "osu! inspection failed: {error}"),
    }
  }
}

impl std::error::Error for ProductInspectError {}

impl From<auv_game_minecraft::MinecraftArtifactReadError> for ProductInspectError {
  fn from(value: auv_game_minecraft::MinecraftArtifactReadError) -> Self {
    Self::Minecraft(value)
  }
}

impl From<auv_game_balatro::BalatroArtifactReadError> for ProductInspectError {
  fn from(value: auv_game_balatro::BalatroArtifactReadError) -> Self {
    Self::Balatro(value)
  }
}

impl From<auv_game_osu::run_read::OsuArtifactReadError> for ProductInspectError {
  fn from(value: auv_game_osu::run_read::OsuArtifactReadError) -> Self {
    Self::Osu(value)
  }
}

pub async fn build_product_inspect_document(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<ProductInspectDocument, ProductInspectError> {
  let sections = collect_sections(store, snapshot).await?;
  Ok(ProductInspectDocument {
    canonical: auv_inspect_model::InspectDocument::from(snapshot),
    sections,
  })
}

async fn collect_sections(store: &dyn RunStore, snapshot: &RunSnapshot) -> Result<Vec<ProductInspectSection>, ProductInspectError> {
  let mut sections = Vec::new();
  sections.push(ProductInspectSection {
    id: "core_prefix",
    text: auv_runtime::inspect::inspect_run_core_prefix_body(store, snapshot).await.map_err(ProductInspectError::Root)?,
  });

  sections.extend(auv_game_minecraft::inspect_sections_primary(store, snapshot).await?.into_iter().map(minecraft_section));
  sections.push(ProductInspectSection {
    id: auv_game_balatro::inspect::BalatroCardDetectionSection::ID,
    text: auv_game_balatro::inspect::render_balatro_card_detection_text(store, snapshot).await?,
  });
  sections.extend(auv_game_minecraft::inspect_sections_quality_spatial(store, snapshot).await?.into_iter().map(minecraft_section));
  sections.extend(auv_game_osu::inspect_sections_primary(store, snapshot).await?.into_iter().map(osu_section));

  let osu_query_wired = collect_osu_query_wired_live_action_summaries(store, snapshot).await?;
  let mut osu_query_wired_text = String::new();
  append_osu_query_wired_section(&mut osu_query_wired_text, &osu_query_wired);
  sections.push(ProductInspectSection {
    id: "osu_query_wired_live_action",
    text: osu_query_wired_text,
  });

  sections.extend(auv_game_osu::inspect_sections_detection_eval(store, snapshot).await?.into_iter().map(osu_section));

  let minecraft_query_wired = collect_minecraft_query_wired_live_action_summaries(store, snapshot).await?;
  let mut minecraft_query_wired_text = String::new();
  append_minecraft_query_wired_section(&mut minecraft_query_wired_text, &minecraft_query_wired);
  sections.push(ProductInspectSection {
    id: "minecraft_query_wired_live_action",
    text: minecraft_query_wired_text,
  });

  sections.push(ProductInspectSection {
    id: "core_suffix",
    text: auv_runtime::inspect::inspect_run_core_suffix_body(store, snapshot).await.map_err(ProductInspectError::Root)?,
  });
  Ok(sections)
}

fn minecraft_section(section: auv_game_minecraft::inspect::MinecraftInspectSection) -> ProductInspectSection {
  ProductInspectSection {
    id: section.id(),
    text: section.into_text(),
  }
}

fn osu_section(section: auv_game_osu::inspect::OsuInspectSection) -> ProductInspectSection {
  ProductInspectSection {
    id: section.id(),
    text: section.into_text(),
  }
}
