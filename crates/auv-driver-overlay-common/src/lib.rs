pub mod components;
pub mod layers;
mod lifecycle;
mod motion;
mod overlay;
mod stats;
pub mod style;
mod theme;

pub use theme::OverlayTheme;

pub use components::IntoOverlayLayers;
pub use layers::Layer;
pub use lifecycle::{LifecycleOptions, Removal};
pub use motion::{ActionEvent, MotionFrame, MotionScene, Travel, Wake};
pub use overlay::{Easing, MotionOptions, Overlay, ShowOptions};
pub use stats::{FrameStats, Percentiles};

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
