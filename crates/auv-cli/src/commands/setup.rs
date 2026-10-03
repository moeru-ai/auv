use clap::{Args, Subcommand};

#[derive(Clone, Debug, Args)]
pub struct SetupArgs {
  #[command(subcommand)]
  command: SetupCommand,
}

#[derive(Clone, Debug, Subcommand)]
enum SetupCommand {
  /// Manage the signed helper used for locked macOS sessions.
  MacosHelper(MacosHelperArgs),
  /// Manage the Windows service and helper used for locked sessions.
  WindowsHelper(WindowsHelperArgs),
}

#[derive(Clone, Debug, Args)]
struct MacosHelperArgs {
  /// Manage this unpacked, notarized helper app shipped by an application
  /// embedding AUV instead of the official AUV Helper.
  #[arg(long, global = true, value_name = "PATH", env = "AUV_MACOS_HELPER_APP")]
  helper_app: Option<std::path::PathBuf>,
  #[command(subcommand)]
  command: MacosHelperCommand,
}

#[derive(Clone, Debug, Subcommand)]
enum MacosHelperCommand {
  /// Report the installed helper identity and current-user readiness.
  Status {
    /// Emit a stable machine-readable result.
    #[arg(long)]
    json: bool,
  },
  /// Install and register the signed helper embedded in this AUV build, or the
  /// app named by `--helper-app`.
  Install {
    /// Emit a stable machine-readable result after installation.
    #[arg(long)]
    json: bool,
  },
  /// Unregister the helper, reset Accessibility, and remove its app bundle.
  Uninstall {
    /// Emit a stable machine-readable result after removal.
    #[arg(long)]
    json: bool,
  },
  /// Open System Settings at Privacy & Security > Accessibility.
  OpenAccessibilitySettings,
  /// Open System Settings at General > Login Items & Extensions.
  OpenBackgroundItemsSettings,
}

#[derive(Clone, Debug, Args)]
struct WindowsHelperArgs {
  #[command(subcommand)]
  command: WindowsHelperCommand,
}

#[derive(Clone, Debug, Subcommand)]
enum WindowsHelperCommand {
  /// Report the installed service, binaries, and current readiness.
  Status {
    /// Emit a stable machine-readable result.
    #[arg(long)]
    json: bool,
  },
  /// Install both release binaries and register the LocalSystem service.
  Install {
    /// Emit a stable machine-readable result after installation.
    #[arg(long)]
    json: bool,
  },
  /// Stop and unregister the service, then remove its installed binaries.
  Uninstall {
    /// Emit a stable machine-readable result after removal.
    #[arg(long)]
    json: bool,
  },
  /// Remove the short-lived first-pairing token file after it is consumed.
  ClearBootstrapToken,
}

pub fn run(args: SetupArgs) -> Result<i32, String> {
  #[cfg(target_os = "macos")]
  {
    match args.command {
      SetupCommand::MacosHelper(args) => {
        let options = auv_device_helper_macos::setup::Options {
          helper_app: args.helper_app,
        };
        match args.command {
          MacosHelperCommand::Status { json } => {
            print_status(&auv_device_helper_macos::setup::status(&options), json)?;
            Ok(0)
          }
          MacosHelperCommand::Install { json } => {
            let status = auv_device_helper_macos::setup::install(&options).map_err(|error| error.to_string())?;
            print_status(&status, json)?;
            Ok(0)
          }
          MacosHelperCommand::Uninstall { json } => {
            let status = auv_device_helper_macos::setup::uninstall(&options).map_err(|error| error.to_string())?;
            print_status(&status, json)?;
            Ok(0)
          }
          MacosHelperCommand::OpenAccessibilitySettings => {
            auv_device_helper_macos::setup::open_accessibility_settings().map_err(|error| error.to_string())?;
            println!("opened macOS Accessibility settings for helper authorization");
            Ok(0)
          }
          MacosHelperCommand::OpenBackgroundItemsSettings => {
            auv_device_helper_macos::setup::open_background_items_settings(&options).map_err(|error| error.to_string())?;
            println!("opened macOS Login Items settings for helper authorization");
            Ok(0)
          }
        }
      }
      SetupCommand::WindowsHelper(_) => Err("Windows helper setup is available only on Windows".to_string()),
    }
  }

  #[cfg(target_os = "windows")]
  {
    match args.command {
      SetupCommand::WindowsHelper(args) => match args.command {
        WindowsHelperCommand::Status { json } => super::windows_helper_setup::status(json),
        WindowsHelperCommand::Install { json } => super::windows_helper_setup::install(json),
        WindowsHelperCommand::Uninstall { json } => super::windows_helper_setup::uninstall(json),
        WindowsHelperCommand::ClearBootstrapToken => super::windows_helper_setup::clear_bootstrap_token(),
      },
      SetupCommand::MacosHelper(_) => Err("macOS helper setup is available only on macOS".to_string()),
    }
  }

  #[cfg(not(any(target_os = "macos", target_os = "windows")))]
  {
    let _ = args;
    Err("helper setup is available only on macOS and Windows".to_string())
  }
}

#[cfg(target_os = "macos")]
fn print_status(status: &auv_device_helper_macos::setup::Status, json: bool) -> Result<(), String> {
  if json {
    println!(
      "{}",
      serde_json::to_string_pretty(&serde_json::json!({
        "state": status.state.as_str(),
        "detail": status.detail,
        "helper_embedded": status.helper_embedded,
      }))
      .map_err(|error| format!("failed to encode helper status: {error}"))?
    );
  } else {
    println!("state\t{}", status.state.as_str());
    println!("helper_embedded\t{}", status.helper_embedded);
    if let Some(detail) = &status.detail {
      println!("detail\t{detail}");
    }
  }

  Ok(())
}
