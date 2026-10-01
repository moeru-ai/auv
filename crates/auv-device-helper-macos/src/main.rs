#[cfg(target_os = "macos")]
mod service_management;

#[cfg(target_os = "macos")]
#[tokio::main(flavor = "current_thread")]
async fn main() {
  if let Some(command) = std::env::args().nth(1) {
    let result = match command.as_str() {
      "--service-management-register" => service_management::register().map(|status| status.as_str()),
      "--service-management-unregister" => service_management::unregister().map(|status| status.as_str()),
      "--service-management-status" => Ok(service_management::status().as_str()),
      "--service-management-open-settings" => {
        service_management::open_settings();
        Ok("opened")
      }
      _ => Err("unknown service-management command".to_string()),
    };
    match result {
      Ok(status) => println!("{status}"),
      Err(error) => {
        eprintln!("{error}");
        std::process::exit(1);
      }
    }
    return;
  }

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
