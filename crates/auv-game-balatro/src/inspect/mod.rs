//! Balatro inspect composition (ordinary readers/renderers).

mod render;
mod sections;

pub use render::append_sections;
pub use sections::{BalatroCardDetectionSection, inspect_sections, render_balatro_card_detection_text};

#[cfg(test)]
mod tests_smoke;
