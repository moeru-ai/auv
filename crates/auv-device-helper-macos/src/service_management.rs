//! Native registration for the LaunchAgent embedded in `AUV Helper.app`.

use objc2::rc::Retained;
use objc2_foundation::{NSError, NSString};
use objc2_service_management::{SMAppService, SMAppServiceStatus};

use crate::ServiceStatus;

pub fn status() -> ServiceStatus {
  let service = service();
  // SAFETY: `service` is a retained SMAppService created by the framework;
  // `status` has no caller-owned pointer or lifetime requirements.
  match unsafe { service.status() } {
    SMAppServiceStatus::Enabled => ServiceStatus::Enabled,
    SMAppServiceStatus::RequiresApproval => ServiceStatus::RequiresApproval,
    SMAppServiceStatus::NotFound => ServiceStatus::NotFound,
    _ => ServiceStatus::NotRegistered,
  }
}

pub fn register() -> Result<ServiceStatus, String> {
  let service = service();
  // SAFETY: `service` is retained for the complete Objective-C call and the
  // generated binding owns the NSError out-parameter contract.
  let before = unsafe { service.status() };
  if before == SMAppServiceStatus::RequiresApproval {
    return Ok(ServiceStatus::RequiresApproval);
  }
  if before != SMAppServiceStatus::Enabled {
    // SAFETY: The generated binding translates the Objective-C NSError
    // convention into Result and retains the returned error when present.
    if let Err(error) = unsafe { service.registerAndReturnError() } {
      if status() == ServiceStatus::RequiresApproval {
        return Ok(ServiceStatus::RequiresApproval);
      }
      return Err(error.to_string());
    }
  }
  Ok(status())
}

pub fn unregister() -> Result<ServiceStatus, String> {
  let service = service();
  // SAFETY: `service` remains retained across the Objective-C message send.
  let before = unsafe { service.status() };
  if !matches!(before, SMAppServiceStatus::Enabled | SMAppServiceStatus::RequiresApproval) {
    return Ok(status());
  }

  let (sender, receiver) = std::sync::mpsc::sync_channel(1);
  let completion = block2::RcBlock::new(move |error: *mut NSError| {
    let result = if error.is_null() {
      Ok(())
    } else {
      // SAFETY: ServiceManagement owns this NSError for the duration of the
      // completion callback. Convert it to an owned string before returning.
      Err(unsafe { &*error }.to_string())
    };
    let _ = sender.send(result);
  });

  // SAFETY: `completion` is a heap block retained by ServiceManagement. Apple
  // invokes it only after the registered LaunchAgent has been terminated, so
  // callers may safely remove or replace the containing app after this returns.
  unsafe { service.unregisterWithCompletionHandler(&completion) };
  let result = receiver
    .recv_timeout(std::time::Duration::from_secs(10))
    .map_err(|_| "timed out waiting for ServiceManagement to stop AUV Helper".to_string())?;
  let after = status();
  match result {
    Ok(()) => Ok(after),
    // A concurrent user/system action may win the race after the initial
    // status read. The terminal states still mean that no registered job can
    // launch or keep an in-flight request alive.
    Err(_) if after.is_unregistered() => Ok(after),
    Err(error) => Err(error),
  }
}

pub fn open_settings() {
  // SAFETY: This class method takes no pointers and only asks the framework
  // to open its system-owned settings pane.
  unsafe { SMAppService::openSystemSettingsLoginItems() };
}

fn service() -> Retained<SMAppService> {
  let name = NSString::from_str("ai.moeru.auv.helper.plist");
  // SAFETY: `name` is a valid retained NSString for the duration of the call;
  // the returned SMAppService is retained by the generated binding.
  unsafe { SMAppService::agentServiceWithPlistName(&name) }
}
