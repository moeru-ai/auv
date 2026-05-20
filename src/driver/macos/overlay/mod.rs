use std::collections::BTreeMap;
use std::sync::{LazyLock, Mutex};
use std::thread;
use std::time::Duration;

use super::*;

mod daemon;
mod protocol;

use daemon::OverlayDaemon;
use protocol::OverlayDaemonCommand::{HideCursor, ShowCursor};

static OVERLAY_DAEMON: LazyLock<Mutex<Option<OverlayDaemon>>> = LazyLock::new(|| Mutex::new(None));

pub(crate) fn overlay_show_cursor(call: &DriverCall) -> AuvResult<DriverResponse> {
  let x = required_f64(call, "x")?;
  let y = required_f64(call, "y")?;
  let label = optional_non_empty_string(call, "label").unwrap_or_else(|| "AUV".to_string());
  let hold_ms = optional_positive_u64(call, "hold_ms")?.unwrap_or(0);
  let mut daemon = lock_overlay_daemon()?;
  let daemon = ensure_overlay_daemon(&mut daemon)?;
  let ack = daemon.send(ShowCursor {
    x,
    y,
    label: label.clone(),
  })?;
  let daemon_pid = daemon.pid();

  if hold_ms > 0 {
    thread::sleep(Duration::from_millis(hold_ms));
  }

  let report = overlay_report([
    ("operation", "show_cursor".to_string()),
    ("event", ack.event),
    ("daemonPid", daemon_pid.to_string()),
    ("globalLogicalPoint", format!("{x:.3},{y:.3}")),
    ("label", label.clone()),
    ("holdMs", hold_ms.to_string()),
    ("coordinateSpace", "global-logical".to_string()),
    ("visualOnly", "true".to_string()),
    ("windowShape", "small-floating".to_string()),
    ("lifecycle", "stdin-eof".to_string()),
    ("crossProcessPersistence", "false".to_string()),
  ]);
  let artifact = build_text_artifact(
    "overlay-cursor",
    "txt",
    "overlay-show-cursor",
    report,
    "Recorded an experimental macOS overlay cursor show command.",
  )?;

  Ok(DriverResponse {
    summary: format!(
      "Showed experimental AUV overlay cursor at global logical point ({x:.3}, {y:.3})."
    ),
    backend: Some("macos.swift.overlay-daemon-poc".to_string()),
    signals: overlay_signals(daemon_pid, "shown"),
    notes: vec![
      "experimental=true".to_string(),
      "visualOnly=true".to_string(),
      "coordinateSpace=global-logical".to_string(),
      "windowShape=small-floating".to_string(),
      "lifecycle=stdin-eof".to_string(),
      "crossProcessPersistence=false".to_string(),
      format!("daemonPid={daemon_pid}"),
      format!("label={label}"),
      format!("holdMs={hold_ms}"),
    ],
    artifacts: vec![artifact],
  })
}

pub(crate) fn overlay_hide_cursor(_call: &DriverCall) -> AuvResult<DriverResponse> {
  let mut daemon = lock_overlay_daemon()?;
  let Some(active_daemon) = daemon.as_mut() else {
    return Ok(no_active_daemon_response("hide_cursor"));
  };
  let daemon_pid = active_daemon.pid();
  let ack = active_daemon.send(HideCursor)?;
  let report = overlay_report([
    ("operation", "hide_cursor".to_string()),
    ("event", ack.event),
    ("daemonPid", daemon_pid.to_string()),
    ("lifecycle", "stdin-eof".to_string()),
    ("crossProcessPersistence", "false".to_string()),
  ]);
  let artifact = build_text_artifact(
    "overlay-cursor",
    "txt",
    "overlay-hide-cursor",
    report,
    "Recorded an experimental macOS overlay cursor hide command.",
  )?;

  Ok(DriverResponse {
    summary: "Hid the experimental AUV overlay cursor in the current process.".to_string(),
    backend: Some("macos.swift.overlay-daemon-poc".to_string()),
    signals: overlay_signals(daemon_pid, "hidden"),
    notes: vec![
      "experimental=true".to_string(),
      "visualOnly=true".to_string(),
      "lifecycle=stdin-eof".to_string(),
      "crossProcessPersistence=false".to_string(),
      format!("daemonPid={daemon_pid}"),
    ],
    artifacts: vec![artifact],
  })
}

pub(crate) fn overlay_shutdown(_call: &DriverCall) -> AuvResult<DriverResponse> {
  let mut daemon = lock_overlay_daemon()?;
  let Some(mut active_daemon) = daemon.take() else {
    return Ok(no_active_daemon_response("shutdown"));
  };
  let daemon_pid = active_daemon.pid();
  active_daemon.shutdown()?;

  Ok(DriverResponse {
    summary: "Shut down the experimental AUV overlay daemon in the current process.".to_string(),
    backend: Some("macos.swift.overlay-daemon-poc".to_string()),
    signals: overlay_signals(daemon_pid, "shutdown"),
    notes: vec![
      "experimental=true".to_string(),
      "visualOnly=true".to_string(),
      "lifecycle=stdin-eof".to_string(),
      "crossProcessPersistence=false".to_string(),
      format!("daemonPid={daemon_pid}"),
    ],
    artifacts: vec![],
  })
}

fn lock_overlay_daemon() -> AuvResult<std::sync::MutexGuard<'static, Option<OverlayDaemon>>> {
  OVERLAY_DAEMON
    .lock()
    .map_err(|error| format!("overlay daemon lock poisoned: {error}"))
}

fn ensure_overlay_daemon(daemon: &mut Option<OverlayDaemon>) -> AuvResult<&mut OverlayDaemon> {
  if daemon.is_none() {
    *daemon = Some(OverlayDaemon::spawn()?);
  }
  daemon
    .as_mut()
    .ok_or_else(|| "failed to initialize overlay daemon".to_string())
}

fn no_active_daemon_response(operation: &str) -> DriverResponse {
  DriverResponse {
    summary: format!(
      "No active experimental AUV overlay daemon exists in this process for {operation}."
    ),
    backend: Some("macos.swift.overlay-daemon-poc".to_string()),
    signals: BTreeMap::from([
      ("overlayEvent".to_string(), "no_active_daemon".to_string()),
      ("crossProcessPersistence".to_string(), "false".to_string()),
    ]),
    notes: vec![
      "experimental=true".to_string(),
      "visualOnly=true".to_string(),
      "lifecycle=stdin-eof".to_string(),
      "crossProcessPersistence=false".to_string(),
      "standalone invoke commands run in separate Rust processes".to_string(),
    ],
    artifacts: vec![],
  }
}

fn overlay_signals(daemon_pid: u32, event: &str) -> BTreeMap<String, String> {
  BTreeMap::from([
    ("overlayEvent".to_string(), event.to_string()),
    ("daemonPid".to_string(), daemon_pid.to_string()),
    ("visualOnly".to_string(), "true".to_string()),
    ("crossProcessPersistence".to_string(), "false".to_string()),
  ])
}

fn overlay_report<const N: usize>(entries: [(&str, String); N]) -> String {
  entries
    .into_iter()
    .map(|(key, value)| format!("{key}={value}"))
    .collect::<Vec<_>>()
    .join("\n")
    + "\n"
}
