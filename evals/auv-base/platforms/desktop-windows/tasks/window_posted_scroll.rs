//! Posted-wheel refusal using a real, bounded application message queue.

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Result, ensure};
use auv_driver_common::{
  ClickOptions, Driver, DriverError, InputActionProgress, InputDeliveryPath, InputPolicy, Point, Scroll, ScrollDeliveryCandidate,
  ScrollDeliveryStrategy, ScrollOptions, WindowInput,
};
use auv_driver_windows::WindowsDriver;
use serde_json::json;
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, WAIT_OBJECT_0, WPARAM};
use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForSingleObject};
use windows::Win32::UI::WindowsAndMessaging::{IsWindow, PostMessageW, WM_APP};
use windows::core::HSTRING;

use super::background::{discover_by_hwnd, point_in_window, run_id};
use super::receiver_process::{Receiver, ReceiverSpec, receiver_binary};
use super::records::Records;
use super::watch::DesktopWatch;

/// Owns the independent controller's three queue events. Drop releases every
/// fixture wait even if setup, the actor, or evidence validation returns early.
struct QueuePause {
  events: [OwnedHandle; 3],
  filled: u64,
  drained: bool,
}

impl QueuePause {
  fn arm(receiver: &mut Receiver, drain_one: bool) -> Result<Self> {
    let unique = format!("Local\\auv-posted-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos());
    let mut handles = Vec::new();
    let names = [
      format!("{unique}-ready"),
      format!("{unique}-step"),
      format!("{unique}-release"),
    ];
    for name in &names {
      // SAFETY: each fresh auto-reset event is transferred exactly once to
      // OwnedHandle. The controller and its owned child share this session.
      let handle = unsafe { CreateEventW(None, false, false, &HSTRING::from(name)) }?;
      handles.push(unsafe { OwnedHandle::from_raw_handle(handle.0) });
    }
    let mut pause = Self {
      events: handles.try_into().expect("three event handles"),
      filled: 0,
      drained: drain_one,
    };
    ensure!(
      receiver.request(json!({"cmd":"pause_posted_queue", "ready":names[0], "step":names[1],
      "release":names[2], "drain_one":drain_one}))?["armed"]
        == true,
      "receiver did not arm queue pause"
    );
    pause.wait_ready()?;
    // Fill only our owned GUI's queue. A real PostMessageW rejection supplies
    // ERROR_NOT_ENOUGH_QUOTA; no Driver fault switch is used. See
    // `https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-postmessagew`.
    loop {
      // SAFETY: a pointer-free filler goes only to this owned receiver HWND.
      match unsafe { PostMessageW(HWND(receiver.hwnd() as _), WM_APP + 6, WPARAM(0), LPARAM(0)) } {
        Ok(()) => {
          pause.filled += 1;
          ensure!(pause.filled < 100_000, "unexpected posted queue limit; stop bounded fixture setup");
        }
        Err(error) => {
          ensure!(error.code().0 as u32 & 0xFFFF == 1816, "queue did not fail with quota: {error}");
          break;
        }
      }
    }
    // SAFETY: live owned event; the GUI removes precisely one filler if asked,
    // then acknowledges its second pause before the actor is allowed to run.
    unsafe { SetEvent(HANDLE(pause.events[1].as_raw_handle())) }?;
    pause.wait_ready()?;
    Ok(pause)
  }

  fn wait_ready(&self) -> Result<()> {
    // SAFETY: retained ready event; the bounded wait does not operate the UI.
    ensure!(unsafe { WaitForSingleObject(HANDLE(self.events[0].as_raw_handle()), 5000) } == WAIT_OBJECT_0, "GUI queue gate not ready");
    Ok(())
  }

  fn release(&self) -> Result<()> {
    // SAFETY: retained release event; no input or window message is synthesized.
    unsafe { SetEvent(HANDLE(self.events[2].as_raw_handle())) }?;
    self.wait_ready()
  }
}

impl Drop for QueuePause {
  fn drop(&mut self) {
    // SAFETY: unblock both phases while these owned handles are still live.
    unsafe {
      let _ = SetEvent(HANDLE(self.events[1].as_raw_handle()));
      let _ = SetEvent(HANDLE(self.events[2].as_raw_handle()));
    }
  }
}

pub fn run(output: &Path) -> Result<bool> {
  ensure!(std::env::var("AUV_ISOLATED_DESKTOP_EVAL").as_deref() == Ok("1"), "requires an isolated desktop");
  ensure!(!output.exists(), "posted-scroll evidence directory must be fresh");
  std::fs::create_dir_all(output)?;
  let session = WindowsDriver::new().open_local()?;
  let binary = receiver_binary()?;
  let watch = DesktopWatch::start()?;
  let original = watch.snapshot();
  let mut records = Records::create(&output.join("window-posted-scroll.jsonl"), &run_id(), "window-posted-scroll")?;
  let cases = [
    "success_background",
    "partial_background_only",
    "partial_background_preferred",
    "none_background_only",
    "none_background_preferred",
    "none_without_foreground_candidate",
  ];
  let mut all_passed = true;
  for case in cases {
    ensure!(watch.modifiers_released(), "modifier already held; no input allowed");
    let spec = |index, title: &str, color: &str| ReceiverSpec {
      index,
      title: title.into(),
      left: 40,
      top: 100,
      width: 430,
      height: 360,
      color: color.into(),
      sink: false,
    };
    let mut a = Receiver::spawn_detached_with(&binary, &spec(0, "AUV posted recipient", "2E6F9E"), &["--foreground-input"])?;
    let mut b = Receiver::spawn_detached_with(&binary, &spec(1, "AUV posted alternate", "9E412E"), &["--foreground-input"])?;
    a.request(json!({"cmd":"keep_foreground"}))?;
    b.request(json!({"cmd":"keep_foreground"}))?;
    let (window, _) = discover_by_hwnd(&session, a.hwnd())?;
    let (other, _) = discover_by_hwnd(&session, b.hwnd())?;
    let point = point_in_window(&window, a.layout.edit_center);
    session.window().activate(&window)?;
    let setup = session.window().click(
      &window,
      point,
      ClickOptions {
        policy: InputPolicy::ForegroundPreferred,
        ..Default::default()
      },
    )?;
    records.write("setup", json!({"case":case,"action":setup}))?;
    let fallback = case == "none_background_preferred";
    if !fallback {
      session.window().activate(&other)?;
    }
    let before_a = a.request(json!({"cmd":"state"}))?;
    let before_b = b.request(json!({"cmd":"state"}))?;
    let partial = case.starts_with("partial_");
    let gated = case != "success_background";
    let pause = if gated {
      Some(QueuePause::arm(&mut a, partial)?)
    } else {
      None
    };
    let before = watch.snapshot();
    ensure!(before.physical_keys == 0 && before.physical_mouse == 0, "physical input: actor refused");
    let policy = if case.ends_with("_only") {
      InputPolicy::BackgroundOnly
    } else {
      InputPolicy::BackgroundPreferred
    };
    let mut candidates = vec![ScrollDeliveryCandidate::WindowTargetedWheel];
    if case != "none_without_foreground_candidate" {
      candidates.push(ScrollDeliveryCandidate::ForegroundHid);
    }
    let started = Instant::now();
    let operation = session.window().scroll(
      &window,
      point,
      Scroll::new(100.0, 100.0),
      ScrollOptions {
        policy,
        delivery_strategy: ScrollDeliveryStrategy { candidates },
        settle: Duration::ZERO,
        ..Default::default()
      },
    );
    let elapsed_ms = started.elapsed().as_millis();
    if let Some(pause) = &pause {
      pause.release()?;
    }
    // Posted replies can precede hardware-input translation. Wait for the
    // application's wheel acknowledgements; never retry an actor input.
    let native_wheels = operation.as_ref().is_ok_and(|action| action.selected_path == InputDeliveryPath::ForegroundSystemEvents);
    let minimum_y = u64::from(partial || !gated) + u64::from(native_wheels);
    let minimum_x = u64::from(!gated || native_wheels);
    let acknowledgement_deadline = Instant::now() + Duration::from_secs(5);
    let (after_a, acknowledged) = loop {
      let state = a.request(json!({"cmd":"state"}))?;
      if state["raw"]["edit_wheel"].as_u64().unwrap_or(0) >= minimum_y && state["raw"]["edit_hwheel"].as_u64().unwrap_or(0) >= minimum_x {
        break (state, true);
      }
      if Instant::now() >= acknowledgement_deadline {
        break (state, false);
      }
      std::thread::sleep(Duration::from_millis(10));
    };
    let after_b = b.request(json!({"cmd":"state"}))?;
    let after = watch.snapshot();
    let progress = operation.as_ref().err().and_then(|error| match error {
      DriverError::InputAction { progress, .. } => Some(progress),
      _ => None,
    });
    let queue = pause.as_ref().map(|pause| json!({"filled":pause.filled,"drained_one":pause.drained}));
    let queue_verified = !gated
      || (after_a["queue_pause"]["stepped"] == true
        && after_a["queue_pause"]["resumed"] == true
        && after_a["queue_pause"]["auto_released"] == false
        && after_a["queue_pause"]["drained"] == partial
        && after_a["queue_fillers_processed"].as_u64() == pause.as_ref().map(|pause| pause.filled - u64::from(partial)));
    let expected_y = u64::from(partial || fallback || !gated);
    let expected_x = u64::from(fallback || !gated);
    let effect = after_a["raw"]["edit_wheel"] == expected_y
      && after_a["raw"]["edit_hwheel"] == expected_x
      && (expected_y == 0 || after_a["first_line"].as_i64() > before_a["first_line"].as_i64())
      && after_b["raw"]["edit_wheel"] == before_b["raw"]["edit_wheel"]
      && after_b["raw"]["edit_hwheel"] == before_b["raw"]["edit_hwheel"];
    let stable_desktop = after.foreground == before.foreground
      && after.cursor == before.cursor
      && after.injected_mouse == before.injected_mouse
      && after.injected_keys == before.injected_keys;
    let passed = if !gated || fallback {
      operation.as_ref().is_ok_and(|action| {
        !action.verified
          && action.selected_path
            == if fallback {
              InputDeliveryPath::ForegroundSystemEvents
            } else {
              InputDeliveryPath::WindowTargetedWheel
            }
      }) && (fallback || stable_desktop)
    } else if partial {
      operation.is_err()
        && matches!(progress,Some(InputActionProgress::Submitted {action})
        if action.selected_path == InputDeliveryPath::WindowTargetedWheel)
        && stable_desktop
    } else {
      operation.is_err() && matches!(progress, Some(InputActionProgress::NotDelivered)) && stable_desktop
    } && acknowledged
      && queue_verified
      && effect
      && after.physical_keys == 0
      && after.physical_mouse == 0
      && watch.modifiers_released();
    let directory = output.join(case);
    std::fs::create_dir(&directory)?;
    for (name, target) in [("recipient", &window), ("alternate", &other)] {
      match session.window().capture(target) {
        Ok(capture) => capture.image.save(directory.join(format!("{name}.png")))?,
        Err(error) => records.write("capture_error", json!({"case":case,"receiver":name,"error":error.to_string()}))?,
      }
    }
    records.write("case",json!({"case":case,"passed":passed,"native_ok":operation.is_ok(),"action":operation.as_ref().ok(),
      "error":operation.as_ref().err().map(ToString::to_string),"input_progress":progress,"before_a":before_a,"before_b":before_b,
      "after_a":after_a,"after_b":after_b,"queue":queue,"queue_verified":queue_verified,"acknowledged":acknowledged,"elapsed_ms":elapsed_ms,
      "cursor_before":before.cursor,"cursor_after":after.cursor,"foreground_before":before.foreground,"foreground_after":after.foreground,
      "physical_mouse":after.physical_mouse,"physical_keys":after.physical_keys,"modifiers_released":watch.modifiers_released(),
      "injected_mouse":after.injected_mouse.saturating_sub(before.injected_mouse),"injected_keys":after.injected_keys.saturating_sub(before.injected_keys),"input_retries":0}))?;
    all_passed &= passed;
    ensure!(after.physical_keys == 0 && after.physical_mouse == 0, "physical input: teardown input refused");
    if after.foreground == a.hwnd() {
      let _ = a.yield_foreground(original.foreground);
    }
    if after.foreground == b.hwnd() {
      let _ = b.yield_foreground(original.foreground);
    }
    session.input().move_to(Point::new(original.cursor.0.into(), original.cursor.1.into()))?;
    drop(pause);
    for receiver in [&mut a, &mut b] {
      let hwnd = receiver.hwnd();
      let exited = receiver.close().is_ok();
      // SAFETY: read-only HWND query after the owned process exit receipt.
      let destroyed = unsafe { !IsWindow(HWND(hwnd as _)).as_bool() };
      records.write("cleanup", json!({"case":case,"window":hwnd,"process_exited":exited,"window_destroyed":destroyed}))?;
      ensure!(exited && destroyed, "owned posted-scroll receiver cleanup incomplete");
    }
  }
  let final_state = watch.snapshot();
  let restored = final_state.cursor == original.cursor && final_state.foreground == original.foreground && watch.modifiers_released();
  records.write(
    "summary",
    json!({"passed":all_passed&&restored,"cases":cases.len(),"pointer_restored":final_state.cursor==original.cursor,
    "foreground_restored":final_state.foreground==original.foreground,"modifiers_released":watch.modifiers_released(),"input_retries":0}),
  )?;
  Ok(all_passed && restored)
}
