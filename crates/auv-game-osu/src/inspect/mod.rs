//! Osu inspect composition (ordinary readers/renderers).

mod render_a;
mod render_b;
mod sections;

pub use render_a::append_sections_a;
pub use render_b::append_sections_b;
pub use sections::{
  OsuDetectionEvalSection, OsuVisualTruthPrimarySection, inspect_sections_detection_eval, inspect_sections_primary,
  render_osu_detection_eval_text, render_osu_primary_text,
};

#[cfg(test)]
mod tests_smoke;
