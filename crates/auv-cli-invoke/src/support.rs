//! Per-platform support declared by every invoke command.
//!
//! `CommandSupport` cells describe one command's direct local path in the
//! current source on one platform. `RunnerSupport` declares how the same
//! command relates to the selected-Runner route. Neither is behavior evidence:
//! `Supported` means the path exists, not that anyone ran it. Evidence levels
//! stay in `docs/SUPPORT_MATRIX.md`.
//!
//! The Runner cells were established by the command-level source audit of
//! 2026-10-25 (adapter arm in `crate::runner`, runner-side handler, platform
//! driver capability). They back the invoke help and MCP metadata.

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

/// How one command relates to the selected-Runner route.
///
/// Every registered command must declare one; the `invoke_command` macro makes
/// `runner =` required, so a command without a declaration does not compile.
/// Like the direct cells, a declaration is a source fact: a routed variant means
/// the Runner adapter arm, the RPC, and the runner-side handler exist in the
/// current source, not that anyone ran them.
// NOTICE(invoke-support-runner-route): `crate::runner`'s pre-dispatch gate
// refuses a non-dry dispatch as `unsupported` when the selected Runner's
// daemon-reported platform is known and this declaration's cell for that
// platform is Unsupported. Unknown platforms never refuse; the backend's
// typed UNIMPLEMENTED answer stays the fallback there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunnerSupport {
  /// The command has no Runner route at all: dispatch refuses it before
  /// contacting a Runner. Fixture commands declare this, matching the refusal
  /// in `crate::runner`.
  NoRoute,
  /// The Runner adapter calls the same driver capability as the direct local
  /// path with no additional limits, so the route's cells are the direct
  /// cells. Allowed only where the source audit confirmed the equality; it
  /// must be re-confirmed when either side changes.
  Direct,
  /// The route's per-platform cells, used where they differ from the direct
  /// cells in state or note.
  Cells(CommandSupport),
}

/// The wording every surface uses for a command with no Runner route: command
/// help, MCP metadata, and the runtime refusal in `crate::runner` all point
/// the caller at the same fix, so the sentence is declared once here.
pub const NO_RUNNER_ROUTE_NOTE: &str = "no Runner route; run it without a selected Run or Device";

impl RunnerSupport {
  /// The route's per-platform cells, or `None` when the command has no Runner
  /// route. `Direct` resolves against the command's direct cells.
  pub fn cells(&self, direct: CommandSupport) -> Option<CommandSupport> {
    match self {
      Self::NoRoute => None,
      Self::Direct => Some(direct),
      Self::Cells(cells) => Some(*cells),
    }
  }
}
