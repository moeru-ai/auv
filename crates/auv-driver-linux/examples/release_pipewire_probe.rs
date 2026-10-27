//! Connects the release bundle's PipeWire client to a running host daemon.
//!
//! Release verification copies this probe beside `auv` in an extracted bundle,
//! so it prepares the same environment as `auv` and loads the bundled client
//! modules, plugins and configuration. The host must run a PipeWire daemon.

#[cfg(target_os = "linux")]
fn main() {
  use pipewire as pw;

  // SAFETY: this is the first statement of `main`; no other thread exists yet.
  unsafe { auv_driver_linux::bundle::prepare_pipewire_environment() };
  let module_dir = std::env::var_os("PIPEWIRE_MODULE_DIR").unwrap_or_else(|| panic!("the probe is not running from a release bundle"));
  pw::init();
  let mainloop = pw::main_loop::MainLoop::new(None).unwrap_or_else(|error| panic!("failed to create PipeWire mainloop: {error}"));
  let context = pw::context::Context::new(&mainloop).unwrap_or_else(|error| panic!("failed to create PipeWire context: {error}"));
  let _core = context.connect(None).unwrap_or_else(|error| panic!("failed to connect to the host PipeWire daemon: {error}"));
  println!("connected to PipeWire with client modules from {}", module_dir.to_string_lossy());
}

#[cfg(not(target_os = "linux"))]
fn main() {}
