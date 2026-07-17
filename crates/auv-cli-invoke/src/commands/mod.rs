pub mod app;
pub mod display;
pub mod input;
pub mod media_control;
#[cfg(target_os = "macos")]
mod ocr;
pub mod overlay;
pub mod scan;
pub mod screen;
pub mod window;
