//! One-shot Windows console worker. Installed only with the privileged host.

#[cfg(target_os = "windows")]
fn main() {
  let mut args = std::env::args().skip(1);
  let Some(pipe) = args.next() else {
    std::process::exit(2)
  };

  let Some(session) = args.next().and_then(|value| value.parse::<u32>().ok()) else {
    std::process::exit(2)
  };

  let Some(logon) = args.next().and_then(|value| value.parse::<i64>().ok()) else {
    std::process::exit(2)
  };

  let Some(sid) = args.next() else {
    std::process::exit(2)
  };

  if args.next().is_some() {
    std::process::exit(2);
  }

  // Exit status is intentionally coarse. Native errors and credentials are
  // never formatted to stdout, stderr, a file, or the process command line.
  if auv_driver_windows::device_unlock_host::run_worker(&pipe, session, logon, &sid).is_err() {
    std::process::exit(1);
  }
}

#[cfg(not(target_os = "windows"))]
fn main() {
  std::process::exit(2);
}
