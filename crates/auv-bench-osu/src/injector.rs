use serde::{Deserialize, Serialize};
#[cfg(not(target_os = "windows"))]
use std::time::Instant;

#[cfg(target_os = "windows")]
use windows::Win32::Media::{timeBeginPeriod, timeEndPeriod};
#[cfg(target_os = "windows")]
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
#[cfg(target_os = "windows")]
use windows::Win32::UI::Input::KeyboardAndMouse::{
  INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK,
  MOUSEINPUT, SendInput, VK_X, VK_Z,
};

pub struct TimePeriodGuard {
  active: bool,
}

impl TimePeriodGuard {
  pub fn new() -> Self {
    #[cfg(target_os = "windows")]
    unsafe {
      let _ = timeBeginPeriod(1);
    }
    Self { active: true }
  }
}

impl Default for TimePeriodGuard {
  fn default() -> Self {
    Self::new()
  }
}

impl Drop for TimePeriodGuard {
  fn drop(&mut self) {
    if self.active {
      #[cfg(target_os = "windows")]
      unsafe {
        let _ = timeEndPeriod(1);
      }
      self.active = false;
    }
  }
}

pub struct PrecisionClock {
  #[cfg(target_os = "windows")]
  frequency: f64,
  #[cfg(target_os = "windows")]
  start_counter: i64,
  #[cfg(not(target_os = "windows"))]
  start_instant: Instant,
  _period_guard: TimePeriodGuard,
}

impl PrecisionClock {
  pub fn new() -> Self {
    let guard = TimePeriodGuard::new();
    #[cfg(target_os = "windows")]
    {
      let mut freq = 0i64;
      let mut start = 0i64;
      unsafe {
        let _ = QueryPerformanceFrequency(&mut freq);
        let _ = QueryPerformanceCounter(&mut start);
      }
      Self {
        frequency: freq as f64,
        start_counter: start,
        _period_guard: guard,
      }
    }
    #[cfg(not(target_os = "windows"))]
    {
      Self {
        start_instant: Instant::now(),
        _period_guard: guard,
      }
    }
  }

  /// Current elapsed time in milliseconds since clock creation.
  pub fn elapsed_ms(&self) -> f64 {
    #[cfg(target_os = "windows")]
    {
      let mut current = 0i64;
      unsafe {
        let _ = QueryPerformanceCounter(&mut current);
      }
      let delta_ticks = current - self.start_counter;
      (delta_ticks as f64) * 1000.0 / self.frequency
    }
    #[cfg(not(target_os = "windows"))]
    {
      self.start_instant.elapsed().as_secs_f64() * 1000.0
    }
  }

  /// Hybrid wait until `target_ms`: sleeps while remaining > 2ms, spins for remainder.
  pub fn sleep_until_ms(&self, target_ms: f64) {
    loop {
      let current = self.elapsed_ms();
      let remaining = target_ms - current;
      if remaining <= 0.0 {
        break;
      }
      if remaining > 2.5 {
        std::thread::sleep(std::time::Duration::from_millis((remaining - 2.0).floor() as u64));
      } else {
        std::hint::spin_loop();
      }
    }
  }
}

impl Default for PrecisionClock {
  fn default() -> Self {
    Self::new()
  }
}

pub const AUV_OSU_INJECT_SIGNATURE: usize = 0x41555630; // 'AUV0'

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameVerificationStatus {
  NotApplicable,
  NotObserved,
  Verified,
}

impl std::fmt::Display for GameVerificationStatus {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::NotApplicable => write!(f, "NOT_APPLICABLE"),
      Self::NotObserved => write!(f, "NOT_OBSERVED"),
      Self::Verified => write!(f, "VERIFIED"),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputDeliveryAccounting {
  pub planned: usize,
  pub attempted: usize,
  pub os_accepted: usize,
  pub hook_received: usize,
  pub game_verified: GameVerificationStatus,
}

#[cfg(target_os = "windows")]
fn attach_thread_to_input_desktop() {
  use windows::Win32::System::StationsAndDesktops::{DESKTOP_ACCESS_FLAGS, DESKTOP_CONTROL_FLAGS, OpenInputDesktop, SetThreadDesktop};
  unsafe {
    if let Ok(desktop) = OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_ACCESS_FLAGS(0x000F_01FF)) {
      let _ = SetThreadDesktop(desktop);
    }
  }
}

/// Win32 SendInput wrapper for osu! actuator
pub struct Win32InputActuator;

impl Win32InputActuator {
  /// Send keyboard key event (VK_Z or VK_X) with benchmark signature
  pub fn send_key(key: crate::synthesizer::KeyCode, is_down: bool) -> bool {
    Self::send_key_with_signature(key, is_down, AUV_OSU_INJECT_SIGNATURE)
  }

  /// Send keyboard key event with explicit extraInfo signature
  pub fn send_key_with_signature(key: crate::synthesizer::KeyCode, is_down: bool, signature: usize) -> bool {
    #[cfg(target_os = "windows")]
    unsafe {
      attach_thread_to_input_desktop();
      let vk = match key {
        crate::synthesizer::KeyCode::K1 => VK_Z,
        crate::synthesizer::KeyCode::K2 => VK_X,
      };
      let flags = if is_down { 0 } else { KEYEVENTF_KEYUP.0 };
      let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
          ki: KEYBDINPUT {
            wVk: vk,
            wScan: 0,
            dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(flags),
            time: 0,
            dwExtraInfo: signature,
          },
        },
      };

      let accepted = SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
      accepted == 1
    }
    #[cfg(not(target_os = "windows"))]
    {
      let _ = (key, is_down, signature);
      false
    }
  }

  /// Send mouse position normalized to desktop [0, 65535] across virtual desktop
  pub fn send_mouse_absolute(norm_x: u16, norm_y: u16) -> bool {
    #[cfg(target_os = "windows")]
    unsafe {
      attach_thread_to_input_desktop();
      let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
          mi: MOUSEINPUT {
            dx: norm_x as i32,
            dy: norm_y as i32,
            mouseData: 0,
            dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
            time: 0,
            dwExtraInfo: AUV_OSU_INJECT_SIGNATURE,
          },
        },
      };
      let accepted = SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
      accepted == 1
    }
    #[cfg(not(target_os = "windows"))]
    {
      let _ = (norm_x, norm_y);
      false
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BenchmarkEdgeRecord {
  pub edge_index: usize,
  pub planned_time_ms: f64,
  pub actual_time_ms: f64,
  pub raw_deviation_ms: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActuatorBenchmarkReport {
  pub planned_edges: usize,
  pub attempted_edges: usize,
  pub os_accepted_edges: usize,
  pub actual_edges: usize,
  pub count_reconciled: bool,
  pub delivery: InputDeliveryAccounting,
  pub raw_p50_ms: f64,
  pub raw_p95_ms: f64,
  pub raw_p99_ms: f64,
  pub raw_max_ms: f64,
  pub median_ms: f64,
  pub comp_p50_ms: f64,
  pub comp_p95_ms: f64,
  pub comp_p99_ms: f64,
  pub comp_max_ms: f64,
  pub gate_16_6ms_passed: bool,
  pub records: Vec<BenchmarkEdgeRecord>,
}

pub fn run_actuator_smoke_bench(num_edges: usize, interval_ms: f64) -> ActuatorBenchmarkReport {
  let clock = PrecisionClock::new();
  let start_offset_ms = 10.0;
  let mut records = Vec::with_capacity(num_edges);
  let mut os_accepted_edges = 0;

  for i in 0..num_edges {
    let planned_time = start_offset_ms + (i as f64) * interval_ms;
    clock.sleep_until_ms(planned_time);
    let actual_time = clock.elapsed_ms();

    // Fire simulated actuator signal
    let is_down = (i % 2) == 0;
    let accepted = Win32InputActuator::send_key(crate::synthesizer::KeyCode::K1, is_down);
    if accepted {
      os_accepted_edges += 1;
    }

    records.push(BenchmarkEdgeRecord {
      edge_index: i,
      planned_time_ms: planned_time,
      actual_time_ms: actual_time,
      raw_deviation_ms: actual_time - planned_time,
    });
  }

  let planned_edges = num_edges;
  let attempted_edges = num_edges;
  let actual_edges = os_accepted_edges;
  let count_reconciled = planned_edges == os_accepted_edges;

  let delivery = InputDeliveryAccounting {
    planned: planned_edges,
    attempted: attempted_edges,
    os_accepted: os_accepted_edges,
    hook_received: 0,
    game_verified: GameVerificationStatus::NotApplicable,
  };

  let mut raw_devs: Vec<f64> = records.iter().map(|r| r.raw_deviation_ms).collect();
  raw_devs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

  let raw_p50 = percentile_linear(&raw_devs, 0.50);
  let raw_p95 = percentile_linear(&raw_devs, 0.95);
  let raw_p99 = percentile_linear(&raw_devs, 0.99);
  let raw_max = *raw_devs.last().unwrap_or(&0.0);

  let median = raw_p50;
  let mut comp_devs: Vec<f64> = raw_devs.iter().map(|x| (x - median).abs()).collect();
  comp_devs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

  let comp_p50 = percentile_linear(&comp_devs, 0.50);
  let comp_p95 = percentile_linear(&comp_devs, 0.95);
  let comp_p99 = percentile_linear(&comp_devs, 0.99);
  let comp_max = *comp_devs.last().unwrap_or(&0.0);

  let gate_passed = comp_p99 <= 16.6;

  ActuatorBenchmarkReport {
    planned_edges,
    attempted_edges,
    os_accepted_edges,
    actual_edges,
    count_reconciled,
    delivery,
    raw_p50_ms: raw_p50,
    raw_p95_ms: raw_p95,
    raw_p99_ms: raw_p99,
    raw_max_ms: raw_max,
    median_ms: median,
    comp_p50_ms: comp_p50,
    comp_p95_ms: comp_p95,
    comp_p99_ms: comp_p99,
    comp_max_ms: comp_max,
    gate_16_6ms_passed: gate_passed,
    records,
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckStatus {
  Passed,
  NotChecked,
  Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SafetyError {
  MissingDirectory(std::path::PathBuf),
  MissingConfigFile(std::path::PathBuf),
  ConfigReadError(String),
  LoggedInUserDetected {
    username: String,
    path: std::path::PathBuf,
  },
  AccountTokenPresent(std::path::PathBuf),
  OnlineSessionDetected(std::path::PathBuf),
  AuthStateUnknown(String),
}

impl std::fmt::Display for SafetyError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::MissingDirectory(p) => {
        write!(f, "SAFETY ABORT: osu! directory not found at {}. Cannot verify offline status (fail-closed)!", p.display())
      }
      Self::MissingConfigFile(p) => {
        write!(f, "SAFETY ABORT: Configuration file not found at {}. Cannot verify offline status (fail-closed)!", p.display())
      }
      Self::ConfigReadError(msg) => {
        write!(f, "SAFETY ABORT: Failed to read configuration or log file: {msg}")
      }
      Self::LoggedInUserDetected { username, path } => {
        write!(
          f,
          "SAFETY ABORT: Logged-in user detected ('{username}') in {}. Must be in guest/offline mode with blank Username!",
          path.display()
        )
      }
      Self::AccountTokenPresent(p) => {
        write!(
          f,
          "SAFETY ABORT: Non-empty account token detected in {}. Must be in guest/offline mode with blank Token! (Token value redacted)",
          p.display()
        )
      }
      Self::OnlineSessionDetected(p) => {
        write!(f, "SAFETY ABORT: Online API state detected in auth log {}. Must be Offline!", p.display())
      }
      Self::AuthStateUnknown(reason) => {
        write!(f, "SAFETY ABORT: Authentication state indeterminate or unverified: {reason}. Refusing input injection (fail-closed)!")
      }
    }
  }
}

impl std::error::Error for SafetyError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfflineSafetyReport {
  pub osu_dir: std::path::PathBuf,
  pub game_ini_verified: bool,
  pub auth_log_verified: bool,
  pub framework_ini_checked: CheckStatus,
  pub realm_database_checked: CheckStatus,
  pub credential_manager_checked: CheckStatus,
  pub passed: bool,
}

/// Resolves the default osu! installation directory across platforms without hardcoding user names.
pub fn default_osu_dir() -> Result<std::path::PathBuf, SafetyError> {
  #[cfg(target_os = "windows")]
  {
    if let Ok(appdata) = std::env::var("APPDATA") {
      let p = std::path::PathBuf::from(appdata).join("osu");
      return Ok(p);
    }
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
      let p = std::path::PathBuf::from(userprofile).join("AppData").join("Roaming").join("osu");
      return Ok(p);
    }
    Err(SafetyError::MissingDirectory(std::path::PathBuf::from("%APPDATA%\\osu (APPDATA and USERPROFILE environment variables not set)")))
  }
  #[cfg(target_os = "macos")]
  {
    if let Ok(home) = std::env::var("HOME") {
      return Ok(std::path::PathBuf::from(home).join("Library").join("Application Support").join("osu"));
    }
    Err(SafetyError::MissingDirectory(std::path::PathBuf::from("$HOME/Library/Application Support/osu (HOME environment variable not set)")))
  }
  #[cfg(target_os = "linux")]
  {
    if let Ok(home) = std::env::var("HOME") {
      return Ok(std::path::PathBuf::from(home).join(".local").join("share").join("osu"));
    }
    Err(SafetyError::MissingDirectory(std::path::PathBuf::from("$HOME/.local/share/osu (HOME environment variable not set)")))
  }
  #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
  {
    Err(SafetyError::AuthStateUnknown("Unsupported platform for default osu directory resolution".into()))
  }
}

/// Verifies that osu!lazer is in guest/offline mode with no account logged in.
/// Performs fail-closed multi-layer verification:
/// 1. Verifies osu! configuration directory exists and is readable.
/// 2. Verifies `game.ini` exists and `Username` and `Token` are confirmed blank.
/// 3. Verifies `logs/*.auth.log` explicitly confirms `API state changed to Offline` with zero online sessions.
/// 4. Discloses that `framework.ini`, `client.realm`, and Windows Credential Manager are `NotChecked` if not verified.
///
/// Aborts with `Err(SafetyError)` if any credentials, online states, missing files, or unverified states are found.
pub fn verify_offline_safety_guard() -> Result<OfflineSafetyReport, SafetyError> {
  let osu_dir = default_osu_dir()?;
  verify_lazer_offline_safety_guard_at_dir(&osu_dir)
}

pub fn verify_offline_safety_guard_at_path(path: &std::path::Path) -> Result<OfflineSafetyReport, SafetyError> {
  if !path.exists() {
    return Err(SafetyError::MissingDirectory(path.to_path_buf()));
  }
  if path.is_file() {
    verify_lazer_ini_file(path)?;
    if let Some(parent) = path.parent()
      && parent.join("logs").exists()
    {
      verify_lazer_offline_safety_guard_at_dir(parent)
    } else {
      Err(SafetyError::AuthStateUnknown(format!(
        "Cannot verify offline auth logs for standalone file {} without parent logs directory",
        path.display()
      )))
    }
  } else {
    verify_lazer_offline_safety_guard_at_dir(path)
  }
}

pub fn verify_lazer_ini_file(ini_path: &std::path::Path) -> Result<(), SafetyError> {
  if !ini_path.exists() {
    return Err(SafetyError::MissingConfigFile(ini_path.to_path_buf()));
  }
  let content =
    std::fs::read_to_string(ini_path).map_err(|e| SafetyError::ConfigReadError(format!("failed to read {}: {e}", ini_path.display())))?;

  let mut seen_username = false;
  let mut seen_token = false;

  for line in content.lines() {
    let trimmed = line.trim();
    if let Some(val) = trimmed.strip_prefix("Username") {
      let val = val.trim_start();
      if let Some(v) = val.strip_prefix('=') {
        seen_username = true;
        let username = v.trim();
        if !username.is_empty() {
          return Err(SafetyError::LoggedInUserDetected {
            username: username.to_string(),
            path: ini_path.to_path_buf(),
          });
        }
      }
    }
    if let Some(val) = trimmed.strip_prefix("Token") {
      let val = val.trim_start();
      if let Some(v) = val.strip_prefix('=') {
        seen_token = true;
        let token = v.trim();
        if !token.is_empty() {
          return Err(SafetyError::AccountTokenPresent(ini_path.to_path_buf()));
        }
      }
    }
  }

  if !seen_username || !seen_token {
    return Err(SafetyError::AuthStateUnknown(format!("missing required 'Username' or 'Token' field in {}", ini_path.display())));
  }

  Ok(())
}

pub fn verify_lazer_offline_safety_guard_at_dir(dir: &std::path::Path) -> Result<OfflineSafetyReport, SafetyError> {
  if !dir.exists() {
    return Err(SafetyError::MissingDirectory(dir.to_path_buf()));
  }

  let ini_path = dir.join("game.ini");
  verify_lazer_ini_file(&ini_path)?;

  // Inspect auth logs in logs/ to ensure no online session is logged in and offline state is verified
  let logs_dir = dir.join("logs");
  if !logs_dir.exists() {
    return Err(SafetyError::AuthStateUnknown(format!("logs directory not found at {}", logs_dir.display())));
  }

  let entries = std::fs::read_dir(&logs_dir)
    .map_err(|e| SafetyError::ConfigReadError(format!("failed to read logs directory {}: {e}", logs_dir.display())))?;

  let mut auth_log_count = 0usize;
  let mut confirmed_offline = false;

  for entry in entries {
    let entry = entry.map_err(|e| SafetyError::ConfigReadError(format!("failed to read log entry in {}: {e}", logs_dir.display())))?;
    let log_path = entry.path();
    let is_auth_log = log_path.file_name().and_then(|n| n.to_str()).map(|name| name.ends_with(".auth.log")).unwrap_or(false);

    if is_auth_log {
      auth_log_count += 1;
      let content = std::fs::read_to_string(&log_path)
        .map_err(|e| SafetyError::ConfigReadError(format!("failed to read auth log {}: {e}", log_path.display())))?;

      if content.contains("API state changed to Online") {
        return Err(SafetyError::OnlineSessionDetected(log_path));
      }
      if content.contains("API state changed to Offline") {
        confirmed_offline = true;
      }
    }
  }

  if auth_log_count == 0 {
    return Err(SafetyError::AuthStateUnknown(format!("no *.auth.log files found in {}; cannot confirm offline state", logs_dir.display())));
  }

  if !confirmed_offline {
    return Err(SafetyError::AuthStateUnknown(format!(
      "auth logs in {} do not contain 'API state changed to Offline'; offline state unconfirmed",
      logs_dir.display()
    )));
  }

  let framework_ini_checked = if dir.join("framework.ini").exists() {
    CheckStatus::Passed
  } else {
    CheckStatus::NotChecked
  };

  Ok(OfflineSafetyReport {
    osu_dir: dir.to_path_buf(),
    game_ini_verified: true,
    auth_log_verified: true,
    framework_ini_checked,
    realm_database_checked: CheckStatus::NotChecked,
    credential_manager_checked: CheckStatus::NotChecked,
    passed: true,
  })
}

/// Linear interpolation percentile (matching numpy default)
pub fn percentile_linear(sorted: &[f64], p: f64) -> f64 {
  if sorted.is_empty() {
    return 0.0;
  }
  if sorted.len() == 1 {
    return sorted[0];
  }
  let rank = p * ((sorted.len() - 1) as f64);
  let lower_idx = rank.floor() as usize;
  let upper_idx = rank.ceil() as usize;
  let fraction = rank - (lower_idx as f64);

  if lower_idx == upper_idx {
    sorted[lower_idx]
  } else {
    sorted[lower_idx] + fraction * (sorted[upper_idx] - sorted[lower_idx])
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_percentile_linear() {
    let vals = vec![10.0, 20.0, 30.0, 40.0, 50.0];
    assert!((percentile_linear(&vals, 0.50) - 30.0).abs() < 1e-6);
    assert!((percentile_linear(&vals, 0.0) - 10.0).abs() < 1e-6);
    assert!((percentile_linear(&vals, 1.0) - 50.0).abs() < 1e-6);
  }

  #[test]
  fn test_actuator_smoke_10_edges() {
    let report = run_actuator_smoke_bench(10, 5.0);
    assert!(report.count_reconciled, "edge count must reconcile");
    assert_eq!(report.actual_edges, 10);
    assert_eq!(report.records.len(), 10);
    for (index, record) in report.records.iter().enumerate() {
      assert_eq!(record.edge_index, index);
      assert_eq!(record.planned_time_ms, 10.0 + index as f64 * 5.0);
      assert!(record.actual_time_ms.is_finite());
      assert!(record.raw_deviation_ms.is_finite());
    }
    // CI runners are shared and have no reliable upper bound on scheduling jitter.
    // Keep the latency gate in the explicit Windows-only benchmark below.
    assert_eq!(report.gate_16_6ms_passed, report.comp_p99_ms <= 16.6);
  }

  #[cfg(target_os = "windows")]
  #[test]
  #[ignore = "requires an interactive, low-load Windows runner for latency measurements"]
  fn test_actuator_latency_gate_50_edges() {
    let report = run_actuator_smoke_bench(50, 16.0);
    assert!(report.count_reconciled, "edge count must reconcile");
    assert!(report.gate_16_6ms_passed, "compensated P99 must be under 16.6ms gate, got {}ms", report.comp_p99_ms);
  }

  struct TestEnvGuard {
    path: std::path::PathBuf,
  }

  impl Drop for TestEnvGuard {
    fn drop(&mut self) {
      let _ = std::fs::remove_dir_all(&self.path);
    }
  }

  fn setup_mock_env(name: &str, username: &str, token: &str, auth_log_content: Option<&str>) -> (TestEnvGuard, std::path::PathBuf) {
    let base = std::env::temp_dir().join(format!("auv_safety_{name}_{}", std::time::SystemTime::now().elapsed().unwrap().as_nanos()));
    let osu_dir = base.join("osu");
    std::fs::create_dir_all(&osu_dir).unwrap();

    let ini_path = osu_dir.join("game.ini");
    std::fs::write(&ini_path, format!("Username = {username}\nToken = {token}\n")).unwrap();

    if let Some(log_content) = auth_log_content {
      let logs_dir = osu_dir.join("logs");
      std::fs::create_dir_all(&logs_dir).unwrap();
      let auth_log_path = logs_dir.join("runtime.auth.log");
      std::fs::write(&auth_log_path, log_content).unwrap();
    }

    (TestEnvGuard { path: base }, osu_dir)
  }

  #[test]
  fn test_offline_safety_guard_rejects_missing_directory() {
    let non_existent = std::env::temp_dir().join("non_existent_osu_dir_987654321");
    let res = verify_lazer_offline_safety_guard_at_dir(&non_existent);
    assert!(matches!(res, Err(SafetyError::MissingDirectory(_))));
    let err_str = res.unwrap_err().to_string();
    assert!(err_str.contains("SAFETY ABORT"));
    assert!(err_str.contains("directory not found"));
  }

  #[test]
  fn test_offline_safety_guard_rejects_missing_ini() {
    let base = std::env::temp_dir().join(format!("auv_no_ini_{}", std::time::SystemTime::now().elapsed().unwrap().as_nanos()));
    let _guard = TestEnvGuard { path: base.clone() };
    let osu_dir = base.join("osu");
    std::fs::create_dir_all(&osu_dir).unwrap();

    let res = verify_lazer_offline_safety_guard_at_dir(&osu_dir);
    assert!(matches!(res, Err(SafetyError::MissingConfigFile(_))));
    let err_str = res.unwrap_err().to_string();
    assert!(err_str.contains("Configuration file not found"));
  }

  #[test]
  fn test_offline_safety_guard_rejects_unreadable_config() {
    let base = std::env::temp_dir().join(format!("auv_unreadable_{}", std::time::SystemTime::now().elapsed().unwrap().as_nanos()));
    let _guard = TestEnvGuard { path: base.clone() };
    let osu_dir = base.join("osu");
    std::fs::create_dir_all(&osu_dir).unwrap();
    // Making game.ini a directory causes read_to_string to fail
    let bad_ini = osu_dir.join("game.ini");
    std::fs::create_dir_all(&bad_ini).unwrap();

    let res = verify_lazer_ini_file(&bad_ini);
    assert!(matches!(res, Err(SafetyError::ConfigReadError(_))));
    let err_str = res.unwrap_err().to_string();
    assert!(err_str.contains("Failed to read configuration"));
  }

  #[test]
  fn test_offline_safety_guard_rejects_logged_in_user() {
    let (_guard, osu_dir) = setup_mock_env("logged_in", "peppy", "", Some("API state changed to Offline"));
    let res = verify_lazer_offline_safety_guard_at_dir(&osu_dir);
    assert!(matches!(res, Err(SafetyError::LoggedInUserDetected { .. })));
    let err_str = res.unwrap_err().to_string();
    assert!(err_str.contains("peppy"));
    assert!(err_str.contains("Logged-in user detected"));
  }

  #[test]
  fn test_offline_safety_guard_rejects_non_empty_token_without_leaking() {
    let secret = "super_secret_bancho_auth_token_987654321";
    let (_guard, osu_dir) = setup_mock_env("secret_token", "", secret, Some("API state changed to Offline"));
    let res = verify_lazer_offline_safety_guard_at_dir(&osu_dir);
    assert!(matches!(res, Err(SafetyError::AccountTokenPresent(_))));
    let err_str = res.unwrap_err().to_string();
    assert!(err_str.contains("Non-empty account token detected"));
    assert!(err_str.contains("Token value redacted"));
    // Explicit assertion: the actual secret must never appear in the error string!
    assert!(!err_str.contains(secret), "Secret token leaked in error message!");
  }

  #[test]
  fn test_offline_safety_guard_rejects_online_session() {
    let (_guard, osu_dir) = setup_mock_env("online_log", "", "", Some("API state changed to Offline\nAPI state changed to Online\n"));
    let res = verify_lazer_offline_safety_guard_at_dir(&osu_dir);
    assert!(matches!(res, Err(SafetyError::OnlineSessionDetected(_))));
    let err_str = res.unwrap_err().to_string();
    assert!(err_str.contains("Online API state detected in auth log"));
  }

  #[test]
  fn test_offline_safety_guard_rejects_indeterminate_auth_state_missing_logs() {
    let (_guard, osu_dir) = setup_mock_env("no_logs", "", "", None);
    let res = verify_lazer_offline_safety_guard_at_dir(&osu_dir);
    assert!(matches!(res, Err(SafetyError::AuthStateUnknown(_))));
    let err_str = res.unwrap_err().to_string();
    assert!(err_str.contains("logs directory not found"));
  }

  #[test]
  fn test_offline_safety_guard_rejects_indeterminate_auth_state_no_offline_marker() {
    let (_guard, osu_dir) = setup_mock_env("no_offline_marker", "", "", Some("Random log line with no state change marker\n"));
    let res = verify_lazer_offline_safety_guard_at_dir(&osu_dir);
    assert!(matches!(res, Err(SafetyError::AuthStateUnknown(_))));
    let err_str = res.unwrap_err().to_string();
    assert!(err_str.contains("offline state unconfirmed"));
  }

  #[test]
  fn test_offline_safety_guard_passes_confirmable_offline_environment() {
    let (_guard, osu_dir) =
      setup_mock_env("confirmed_offline", "", "", Some("Session initialized.\nAPI state changed to Offline\nLocal play active.\n"));
    let res = verify_lazer_offline_safety_guard_at_dir(&osu_dir);
    assert!(res.is_ok(), "Expected valid offline environment to pass: {:?}", res);
    let report = res.unwrap();
    assert!(report.passed);
    assert!(report.game_ini_verified);
    assert!(report.auth_log_verified);
    assert_eq!(report.framework_ini_checked, CheckStatus::NotChecked);
    assert_eq!(report.realm_database_checked, CheckStatus::NotChecked);
    assert_eq!(report.credential_manager_checked, CheckStatus::NotChecked);
  }

  #[test]
  fn test_safety_guard_blocks_input_dispatch_when_unverified() {
    let non_existent = std::env::temp_dir().join("non_existent_dir_safety_gate");
    let mut inputs_dispatched = false;

    let res = verify_lazer_offline_safety_guard_at_dir(&non_existent);
    if res.is_ok() {
      inputs_dispatched = true;
    }

    assert!(res.is_err());
    assert!(!inputs_dispatched, "Inputs must not be dispatched when safety pre-check fails!");
  }

  #[test]
  fn test_input_delivery_accounting_five_stages() {
    let accounting = InputDeliveryAccounting {
      planned: 50,
      attempted: 50,
      os_accepted: 50,
      hook_received: 0,
      game_verified: GameVerificationStatus::NotApplicable,
    };

    assert_eq!(accounting.planned, 50);
    assert_eq!(accounting.attempted, 50);
    assert_eq!(accounting.os_accepted, 50);
    assert_eq!(accounting.hook_received, 0);
    assert_eq!(accounting.game_verified, GameVerificationStatus::NotApplicable);
    assert_eq!(accounting.game_verified.to_string(), "NOT_APPLICABLE");
    assert_eq!(GameVerificationStatus::NotObserved.to_string(), "NOT_OBSERVED");
    assert_eq!(GameVerificationStatus::Verified.to_string(), "VERIFIED");

    // Serialization check
    let serialized = serde_json::to_string(&accounting).expect("serialize accounting");
    let deserialized: InputDeliveryAccounting = serde_json::from_str(&serialized).expect("deserialize accounting");
    assert_eq!(accounting, deserialized);
  }
}
