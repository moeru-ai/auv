#![deny(clippy::all)]

use napi::{Task, bindgen_prelude::AsyncTask};
use napi_derive::napi;

/// Return the version embedded into the native binding.
///
/// The JavaScript entrypoint compares this value with its package manifest so
/// a partially published release fails before it starts an AUV sidecar.
#[napi]
pub fn native_package_version() -> &'static str {
  env!("CARGO_PKG_VERSION")
}

#[napi(object)]
pub struct MacosHelperStatus {
  pub state: String,
  pub detail: Option<String>,
  /// Whether this binding can install a helper: one embedded by a release
  /// build, or the app named by `AUV_MACOS_HELPER_APP`.
  pub helper_embedded: bool,
}

/// Inspect the AUV Helper installation used for locked macOS sessions.
///
/// Signature validation and ServiceManagement inspection run off the
/// JavaScript thread because they perform blocking platform calls.
#[napi(ts_return_type = "Promise<MacosHelperStatus>")]
pub fn macos_helper_status() -> AsyncTask<MacosHelperStatusTask> {
  AsyncTask::new(MacosHelperStatusTask)
}

/// Install and register the signed AUV Helper embedded in this build, or the
/// app named by `AUV_MACOS_HELPER_APP` when the embedding application ships
/// its own helper.
///
/// The work runs off the JavaScript thread because archive verification and
/// ServiceManagement registration perform blocking platform calls.
#[napi(ts_return_type = "Promise<MacosHelperStatus>")]
pub fn install_macos_helper() -> AsyncTask<InstallMacosHelper> {
  AsyncTask::new(InstallMacosHelper)
}

/// Unregister the helper, reset its Accessibility decision, and remove its app.
///
/// Enrollment remains in the current user's login Keychain. The work runs off
/// the JavaScript thread because it waits for ServiceManagement termination and
/// performs filesystem and TCC operations.
#[napi(ts_return_type = "Promise<MacosHelperStatus>")]
pub fn uninstall_macos_helper() -> AsyncTask<UninstallMacosHelper> {
  AsyncTask::new(UninstallMacosHelper)
}

/// Open System Settings at Privacy & Security > Accessibility.
#[napi]
pub fn open_macos_helper_accessibility_settings() -> napi::Result<()> {
  #[cfg(target_os = "macos")]
  {
    auv_device_helper_macos::setup::open_accessibility_settings().map_err(|error| napi::Error::from_reason(error.to_string()))
  }

  #[cfg(not(target_os = "macos"))]
  {
    Err(napi::Error::from_reason("macOS helper setup is available only on macOS"))
  }
}

/// Open System Settings at General > Login Items & Extensions.
#[napi]
pub fn open_macos_helper_background_items_settings() -> napi::Result<()> {
  #[cfg(target_os = "macos")]
  {
    auv_device_helper_macos::setup::open_background_items_settings().map_err(|error| napi::Error::from_reason(error.to_string()))
  }

  #[cfg(not(target_os = "macos"))]
  {
    Err(napi::Error::from_reason("macOS helper setup is available only on macOS"))
  }
}

pub struct InstallMacosHelper;

pub struct MacosHelperStatusTask;

pub struct UninstallMacosHelper;

impl Task for MacosHelperStatusTask {
  type Output = MacosHelperStatus;
  type JsValue = MacosHelperStatus;

  fn compute(&mut self) -> napi::Result<Self::Output> {
    #[cfg(target_os = "macos")]
    {
      Ok(status_value(auv_device_helper_macos::setup::status()))
    }

    #[cfg(not(target_os = "macos"))]
    {
      Err(napi::Error::from_reason("macOS helper setup is available only on macOS"))
    }
  }

  fn resolve(&mut self, _env: napi::Env, output: Self::Output) -> napi::Result<Self::JsValue> {
    Ok(output)
  }
}

impl Task for InstallMacosHelper {
  type Output = MacosHelperStatus;
  type JsValue = MacosHelperStatus;

  fn compute(&mut self) -> napi::Result<Self::Output> {
    #[cfg(target_os = "macos")]
    {
      auv_device_helper_macos::setup::install()
        .map(status_value)
        .map_err(|error| napi::Error::from_reason(error.to_string()))
    }

    #[cfg(not(target_os = "macos"))]
    {
      Err(napi::Error::from_reason("macOS helper setup is available only on macOS"))
    }
  }

  fn resolve(&mut self, _env: napi::Env, output: Self::Output) -> napi::Result<Self::JsValue> {
    Ok(output)
  }
}

impl Task for UninstallMacosHelper {
  type Output = MacosHelperStatus;
  type JsValue = MacosHelperStatus;

  fn compute(&mut self) -> napi::Result<Self::Output> {
    #[cfg(target_os = "macos")]
    {
      auv_device_helper_macos::setup::uninstall()
        .map(status_value)
        .map_err(|error| napi::Error::from_reason(error.to_string()))
    }

    #[cfg(not(target_os = "macos"))]
    {
      Err(napi::Error::from_reason("macOS helper setup is available only on macOS"))
    }
  }

  fn resolve(&mut self, _env: napi::Env, output: Self::Output) -> napi::Result<Self::JsValue> {
    Ok(output)
  }
}

#[cfg(target_os = "macos")]
fn status_value(status: auv_device_helper_macos::setup::Status) -> MacosHelperStatus {
  MacosHelperStatus {
    state: status.state.as_str().to_string(),
    detail: status.detail,
    helper_embedded: status.helper_embedded,
  }
}

// TODO(napi-operation-api): General operation bindings remain intentionally
// deferred; this owner-approved setup interface does not expose invoke/runtime
// execution through a second frontend.
