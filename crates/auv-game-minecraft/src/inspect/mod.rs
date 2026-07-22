//! Minecraft inspect composition (ordinary readers/renderers).

mod quality;
mod render;
mod sections;

pub use quality::{
  MinecraftInspectedArtifact, MinecraftQualityBaseline, MinecraftQualityVerdict, MinecraftQualityVerdicts, QualityEvidenceCoverage,
  QualityRenderEvidenceMode, QualityStage, QualityStageCheck, QualityStageOutcome, QualityVerdictOutcome,
};
pub use sections::{
  MinecraftInspectSection, MinecraftQualitySpatialInspection, MinecraftSpatialQueryInspection, inspect_sections_primary,
  inspect_sections_quality_spatial, read_minecraft_quality_spatial_inspection,
};

#[cfg(test)]
mod tests_smoke;
