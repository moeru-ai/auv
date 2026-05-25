#![allow(dead_code)]

pub mod ax_tree;
#[cfg(target_os = "macos")]
mod binding;
pub mod clipboard;
pub mod error;
pub mod ocr;
pub mod permission;
pub mod pointer;
pub mod types;
pub mod window;
