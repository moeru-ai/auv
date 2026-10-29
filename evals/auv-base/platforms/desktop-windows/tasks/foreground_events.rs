//! Native event-coverage probe for the proposed Windows input-recipient contract.
//!
//! This records existing Driver activation and posted-input behavior on owned fixtures. It does
//! not implement a lease or treat a WinEvent callback as proof that input would be safe.

// TODO(windows-input-target-lease): production guards need a recipient/loss contract before they
// consume event history. See `docs/ai/references/driver/2026-10-30-foreground-event-coverage.md`.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use auv_driver_common::{Driver, input::WindowInput};
use auv_driver_windows::WindowsDriver;
use serde_json::json;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsWindow};

use super::background::{background_click, discover_by_hwnd, point_in_window, run_id};
use super::receiver_process::{Receiver, ReceiverSpec, receiver_binary};
use super::records::Records;
use super::watch::DesktopWatch;

pub fn run(output: &Path, rounds: u32) -> Result<bool> {
  ensure!((1..=50).contains(&rounds), "foreground event probe needs 1..=50 rounds");
  std::fs::create_dir_all(output)?;
  let mut records = Records::create(&output.join("foreground-events.jsonl"), &run_id(), "foreground-events")?;
  let session = WindowsDriver::default().open_local().context("open driver session")?;
  let watch = DesktopWatch::start()?;
  let original = watch.snapshot();
  let binary = receiver_binary()?;
  let spec = |index, title: &str, left| ReceiverSpec {
    index,
    title: title.into(),
    left,
    top: 130,
    width: 380,
    height: 300,
    color: "2E6F9E".into(),
    sink: false,
  };
  let mut anchor = Receiver::spawn_detached_with(&binary, &spec(0, "AUV event anchor", 80), &["--foreground-input"])?;
  let mut other = Receiver::spawn_detached_with(&binary, &spec(1, "AUV event alternate", 500), &["--foreground-input"])?;
  let mut background = Receiver::spawn_detached(&binary, &spec(2, "AUV event background", 300))?;
  anchor.request(json!({"cmd": "keep_foreground"}))?;
  other.request(json!({"cmd": "keep_foreground"}))?;
  let (anchor_window, _) = discover_by_hwnd(&session, anchor.hwnd())?;
  let (other_window, _) = discover_by_hwnd(&session, other.hwnd())?;
  let (background_window, _) = discover_by_hwnd(&session, background.hwnd())?;
  session.window().activate(&anchor_window).context("activate owned anchor")?;
  ensure!(anchor.state()?.foreground, "owned anchor did not confirm foreground");
  records.write("setup", json!({
    "evidence_level": "native_event_probe_only", "rounds": rounds,
    "anchor": {"hwnd": anchor.hwnd(), "pid": anchor.layout.pid},
    "alternate": {"hwnd": other.hwnd(), "pid": other.layout.pid},
    "background": {"hwnd": background.hwnd(), "pid": background.layout.pid},
    "original": original,
    "notice": "Callbacks are asynchronous; named windows and callback-time foreground are separate facts. This is not input-lease acceptance."
  }))?;
  let mut aba_named_alternate = 0;
  let mut aba_callback_only_anchor = 0;
  let mut background_named_without_takeover = 0;
  let mut background_takeovers = 0;
  let mut complete = true;
  for round in 0..rounds {
    for kind in ["stable", "aba", "background_click"] {
      let current = watch.snapshot();
      if current.physical_keys != 0 || current.physical_mouse != 0 || current.foreground != anchor.hwnd() {
        records.write("interrupted", json!({"round": round, "case": kind, "snapshot": current}))?;
        complete = false;
        break;
      }
      let app_before = background.state()?;
      let before = watch.snapshot();
      let mut middle = None;
      let mut delivery = None;
      match kind {
        "stable" => std::thread::sleep(Duration::from_millis(75)),
        "aba" => {
          session.window().activate(&other_window).context("activate owned alternate")?;
          let active_other = watch.snapshot();
          ensure!(active_other.foreground == other.hwnd(), "alternate did not become foreground");
          middle = Some(active_other);
          // No application-state barrier between these two activations: record the real B state,
          // return to A immediately, then see what the independent hook thread actually retained.
          session.window().activate(&anchor_window).context("return to owned anchor")?;
        }
        "background_click" => {
          delivery = Some(session.window().click(
            &background_window,
            point_in_window(&background_window, background.layout.button_center),
            background_click(),
          )?);
        }
        _ => unreachable!(),
      }
      let immediate = watch.snapshot();
      let app_after = background.state()?;
      let anchor_after = anchor.state()?;
      let alternate_after = other.state()?;
      // Read-only collection window for asynchronous receipts, not an input retry or a lease barrier.
      std::thread::sleep(Duration::from_millis(150));
      let after = watch.snapshot();
      let events = watch.timeline(before.taken_ms, after.taken_ms).foreground_events;
      let names_alternate = events.iter().any(|event| event.window == other.hwnd());
      let names_background = events.iter().any(|event| event.window == background.hwnd());
      let takeover = app_after.foreground_taken != app_before.foreground_taken;
      if kind == "aba" {
        ensure!(immediate.foreground == anchor.hwnd(), "ABA did not return to anchor");
        aba_named_alternate += u32::from(names_alternate);
        aba_callback_only_anchor += u32::from(names_alternate && events.iter().all(|event| event.actual == anchor.hwnd()));
      }
      if kind == "background_click" {
        ensure!(app_after.clicks == app_before.clicks + 1, "background receiver did not process its one click");
        background_takeovers += u32::from(takeover);
        background_named_without_takeover += u32::from(names_background && !takeover && after.foreground == anchor.hwnd());
      }
      records.write(
        "case",
        json!({
          "round": round, "case": kind, "before": before, "middle": middle,
          "immediate": immediate, "after": after, "events": events,
          "application_before": app_before, "application_after": app_after, "delivery": delivery,
          "anchor_application_after": anchor_after, "alternate_application_after": alternate_after,
          "named_alternate": names_alternate, "named_background": names_background,
          "application_reported_takeover": takeover,
          "event_count_delta": after.foreground_changes.saturating_sub(before.foreground_changes),
          "input_retries": 0
        }),
      )?;
      ensure!(
        after.injected_mouse == before.injected_mouse && after.injected_keys == before.injected_keys,
        "activation or background posting unexpectedly injected global input"
      );
      ensure!(after.cursor == before.cursor, "probe operation moved the shared pointer");
      if after.physical_keys != 0 || after.physical_mouse != 0 || after.foreground != anchor.hwnd() {
        records.write("interrupted", json!({"round": round, "case": kind, "snapshot": after}))?;
        complete = false;
        break;
      }
    }
    if !complete {
      break;
    }
  }
  // Restore only while an owned fixture still has foreground; never displace unrelated activity.
  // SAFETY: read-only query; no borrowed HWND is dereferenced.
  let foreground = unsafe { GetForegroundWindow() }.0 as isize;
  let restore = if [anchor.hwnd(), other.hwnd(), background.hwnd()].contains(&foreground) && original.foreground != 0 {
    anchor.yield_foreground(original.foreground)
  } else {
    Ok(false)
  };
  records
    .write("foreground_restore", json!({"handed_back": restore.as_ref().ok(), "error": restore.err().map(|error| error.to_string())}))?;
  for receiver in [&mut background, &mut other, &mut anchor] {
    let hwnd = receiver.hwnd();
    let closed = receiver.close().is_ok();
    // SAFETY: read-only check of the HWND held by the receiver whose owned process just exited.
    let destroyed = unsafe { !IsWindow(HWND(hwnd as _)).as_bool() };
    records.write("cleanup", json!({"window": hwnd, "process_exit_confirmed": closed, "window_destroyed": destroyed}))?;
    ensure!(closed && destroyed, "owned receiver cleanup was not confirmed");
  }
  let finish = watch.snapshot();
  records.write("summary", json!({
    "evidence_level": "native_event_probe_only", "complete": complete, "rounds": rounds,
    "aba_named_alternate": aba_named_alternate, "aba_callback_only_anchor": aba_callback_only_anchor,
    "background_named_without_application_takeover": background_named_without_takeover,
    "background_application_takeovers": background_takeovers, "original": original, "finish": finish,
    "pointer_restored": finish.cursor == original.cursor, "foreground_restored": finish.foreground == original.foreground,
    "notice": "Counts describe these fixtures only; an absent callback does not prove no transition. No input lease or click predicate is implemented."
  }))?;
  Ok(complete)
}
