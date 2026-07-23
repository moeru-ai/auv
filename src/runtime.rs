//! Transitional project configuration used by the remaining scroll-scan API.
//!
//! Run creation, storage, and dispatch belong to the calling frontend. This
//! type deliberately carries no execution or recording behavior.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Runtime {
  project_root: PathBuf,
}

impl Runtime {
  pub fn new(project_root: PathBuf) -> Self {
    Self { project_root }
  }

  pub fn project_root(&self) -> &Path {
    &self.project_root
  }

  pub fn open_session(&self, options: crate::session::SessionOptions) -> crate::session::SessionRuntime {
    crate::session::SessionRuntime::new(options)
  }
}
