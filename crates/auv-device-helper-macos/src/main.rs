#[cfg(target_os = "macos")]
#[tokio::main(flavor = "current_thread")]
async fn main() {
  if auv_device_helper_macos::host::serve().await.is_err() {
    // The service manager can observe an unsuccessful exit. Native error
    // strings and credential-bearing values never reach stderr or syslog.
    std::process::exit(1);
  }
}

#[cfg(not(target_os = "macos"))]
fn main() {
  std::process::exit(1);
}
