//! Minecraft inspect composition (ordinary readers/renderers).

mod helpers;
mod render;
mod sections;

pub use sections::{
  MinecraftPrimarySection, MinecraftQualitySpatialSection, inspect_sections_primary, inspect_sections_quality_spatial,
  render_minecraft_primary_text, render_minecraft_quality_spatial_text,
};

#[cfg(test)]
mod tests_smoke;
