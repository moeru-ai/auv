pub mod components;
pub mod layers;
mod lifecycle;
mod overlay;
pub mod style;
mod theme;

pub use theme::OverlayTheme;

pub use components::IntoOverlayLayers;
pub use layers::Layer;
pub use lifecycle::{LifecycleOptions, Removal};
pub use overlay::{Easing, MotionOptions, Overlay, ShowOptions};

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
