//! Root `auv` core command frontend and first-party Runner process host.

fn main() {
  // SAFETY: this is the first statement of `main`; no other thread exists yet.
  #[cfg(target_os = "linux")]
  unsafe {
    auv_driver_linux::bundle::prepare_pipewire_environment()
  };

  // Before any window exists: the Windows driver works in physical pixels, see
  // NOTICE(windows-dpi-awareness) in `auv_driver_windows::dpi`.
  #[cfg(windows)]
  if !auv_driver_windows::dpi::set_process_per_monitor_aware() {
    eprintln!(
      "warning: auv could not make this process per-monitor DPI aware (a manifest or compatibility setting overrides it); on a scaled display, window frames, captures and clicks may disagree"
    );
  }

  let arguments = std::env::args().collect::<Vec<_>>();
  if let Some(exit) = auv_cli::runner::run_if_internal(&arguments) {
    std::process::exit(exit);
  }

  let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("build AUV CLI runtime");
  let exit = runtime.block_on(auv_cli::cli::run_root());
  std::process::exit(auv_cli::cli::exit_status(exit));
}
