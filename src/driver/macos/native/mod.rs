// File: src/driver/macos/native/mod.rs
pub(crate) mod ax_tree;
pub(crate) mod clipboard;
pub(crate) mod error;
#[cfg(target_os = "macos")]
mod ffi;
pub(crate) mod ocr;
pub(crate) mod overlay;
pub(crate) mod permission;
pub(crate) mod pointer;
pub(crate) mod window;
