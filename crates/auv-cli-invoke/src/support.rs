//! Per-platform support declared by every invoke command.
//!
//! A cell describes one command's direct local path in the current source on one
//! platform. It is not behavior evidence: `Supported` means the path exists, not
//! that anyone ran it. Evidence levels stay in `docs/SUPPORT_MATRIX.md`, and a
//! selected Runner's support is a separate fact (`TODO(invoke-support-runner-route)`).

use serde::Serialize;

/// Desktop platforms an invoke command can name a local path for.
///
/// TODO(invoke-support-other-platforms): a target outside these three has no cell.
/// Add a variant, and a cell on every command, when such a driver exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
  Macos,
  Linux,
  Windows,
}

impl Platform {
  pub const ALL: [Self; 3] = [Self::Macos, Self::Linux, Self::Windows];

  /// The name help output and documentation show.
  pub fn label(self) -> &'static str {
    match self {
      Self::Macos => "macOS",
      Self::Linux => "Linux",
      Self::Windows => "Windows",
    }
  }
}

/// How much of a command's contract one platform's direct local path covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportState {
  /// A direct, non-dry-run path exists for everything the command accepts.
  Supported,
  /// A path exists for some inputs or delivery modes. The cell's note names the limit.
  Partial,
  /// No path exists. The command refuses, or completes without doing the work and says so.
  Unsupported,
  /// Nobody has audited the path, so the cell makes no claim either way.
  NotEvidenced,
}

impl SupportState {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Supported => "supported",
      Self::Partial => "partial",
      Self::Unsupported => "unsupported",
      Self::NotEvidenced => "not_evidenced",
    }
  }
}

/// One command on one platform: a state and, where the state needs one, its reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct PlatformSupport {
  pub state: SupportState,
  /// The limit of a `Partial` cell, or why an `Unsupported` or `NotEvidenced` cell is that way.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub note: Option<&'static str>,
}

impl PlatformSupport {
  pub const fn new(state: SupportState, note: Option<&'static str>) -> Self {
    Self { state, note }
  }
}

/// A command's cells. Every platform is required, so a new command cannot omit one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct CommandSupport {
  pub macos: PlatformSupport,
  pub linux: PlatformSupport,
  pub windows: PlatformSupport,
}

impl CommandSupport {
  pub fn on(&self, platform: Platform) -> PlatformSupport {
    match platform {
      Platform::Macos => self.macos,
      Platform::Linux => self.linux,
      Platform::Windows => self.windows,
    }
  }
}

/// What a command is, which decides how its cells count.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandRole {
  /// Drives the desktop through a platform driver.
  Desktop,
  /// Another name for `canonical`. It shares that command's cells and counts once.
  Alias { canonical: &'static str },
  /// Processes caller-supplied fixture data and never touches a desktop, so its
  /// platform cells describe the build, not a driver.
  Fixture,
}
