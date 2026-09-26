//! Test-only persistent sender. Input/receipt assertions live in the independent
//! browser fixture; this process only invokes the real public driver methods.

#[cfg(target_os = "macos")]
fn main() {
  use std::io::{BufRead, Write};
  use std::time::Duration;

  use auv_driver_common::{Driver, InputPolicy, InputTarget, KeyboardHold, PressKeysOptions, TypeTextOptions};
  use auv_driver_macos::MacosDriver;
  use serde_json::{Value, json};

  let session = MacosDriver::new().open_local().unwrap();
  let mut held: Option<KeyboardHold> = None;

  for line in std::io::stdin().lock().lines() {
    let line = line.unwrap();
    let response = (|| -> Result<Value, String> {
      let request: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
      let command: Vec<String> = serde_json::from_value(request["command"].clone()).map_err(|e| e.to_string())?;

      if command[0] == "drop" {
        drop(held.take());
        return Ok(json!({"result": null}));
      }

      let window = session
        .window()
        .list()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|w| w.reference.id == request["window"].as_str().unwrap())
        .ok_or("receiver window missing")?;
      let target = InputTarget::Window(window);
      let policy = if request["mode"] == "foreground" {
        InputPolicy::ForegroundPreferred
      } else {
        InputPolicy::BackgroundOnly
      };

      let input = session.input();
      let keys = command[1..].to_vec();
      let result = match command[0].as_str() {
        "hold" => input.hold_keys(&target, keys, policy, Duration::from_millis(200)).map_err(|e| e.to_string())?,
        "down" | "timeout" => {
          if held.is_some() {
            return Err("fixture already owns a hold".into());
          }

          let timeout = if command[0] == "timeout" { 200 } else { 2000 };
          let hold = input.key_down(&target, keys, policy, Duration::from_millis(timeout)).map_err(|e| e.to_string())?;
          let result = hold.down_result().clone();
          held = Some(hold);

          result
        }
        "up" => held.as_mut().ok_or("fixture has no hold")?.release().map_err(|e| e.to_string())?,
        "input.keys" => input
          .press_keys(
            &target,
            PressKeysOptions {
              keys,
              ..Default::default()
            },
            policy,
            false,
          )
          .map_err(|e| e.to_string())?
          .ok_or("missing press result")?,
        "input.typeText" => session
          .window()
          .type_text(
            match &target {
              InputTarget::Window(window) => window,
              _ => unreachable!(),
            },
            &command[1],
            TypeTextOptions {
              policy,
              ..Default::default()
            },
          )
          .map_err(|e| e.to_string())?,
        _ => return Err("unknown fixture command".into()),
      };

      Ok(json!({"result": result}))
    })()
    .unwrap_or_else(|error| json!({"error": error}));

    println!("{response}");
    std::io::stdout().flush().unwrap();
  }

  drop(held);
  auv_driver_common::keyboard_coordinator().shutdown().unwrap();
}

#[cfg(not(target_os = "macos"))]
fn main() {
  eprintln!("This receiver sender requires macOS.");
}
