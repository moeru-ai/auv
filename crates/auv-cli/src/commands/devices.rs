use auv_cli_common::{
  TableRow,
  outputs::formats::table::{self, TableOptions},
};
use clap::{ArgGroup, Args, Subcommand};

#[derive(Clone, Debug, Args)]
#[command(
  after_long_help = "Examples:\n  # List local and paired Devices\n  auv devices list\n\n  # Inspect one Device by its stable ID\n  auv devices get <DEVICE_ID>\n\n  # List current OS login sessions on a paired Device\n  auv --device <NAME> devices sessions\n\n  # Lock and unlock an existing OS login session\n  auv --device <NAME> devices lock --user neko\n  auv --device <NAME> devices unlock --user neko\n\n  # On the Device to unlock: enroll its login credential once\n  auv devices credentials enroll --user neko --kind os-password\n\n  # Learn the two-machine enrollment flow\n  auv devices pair --help\n\n  # Run the same typed operation on a paired Device\n  auv --device <NAME> invoke display.list"
)]
pub struct DevicesArgs {
  #[command(subcommand)]
  pub command: DevicesCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub enum DevicesCommand {
  /// List Devices visible through the selected daemon.
  #[command(alias = "ls")]
  List(DeviceListArgs),
  /// Get one Device by stable ID.
  Get(DeviceGetArgs),
  /// List current OS login sessions (requires a validated native entry host).
  Sessions(DeviceSessionsArgs),
  /// Unlock an existing OS login session (requires a validated native host).
  Unlock(DeviceSessionTargetArgs),
  /// Lock an existing usable OS login session and verify its final state.
  Lock(DeviceSessionTargetArgs),
  /// Establish or administer paired Device trust.
  #[command(
    long_about = "Pairing is a two-machine enrollment flow. The daemon host creates a short, one-time bootstrap token locally. The client consumes that token once, receives an opaque Device credential, and saves it in a named local profile.\n\nRegistration is deliberately local to the daemon host: a remote unauthenticated caller cannot create pairing tokens. After enrollment, normal AUV commands select the paired Device and reuse the saved credential automatically."
  )]
  Pair(PairingArgs),
  /// Remove a paired Device trust relationship.
  Unpair(DeviceTrustArgs),
  /// Enable a paired Device trust relationship.
  Enable(DeviceTrustArgs),
  /// Disable a paired Device trust relationship without deleting its history.
  Disable(DeviceTrustArgs),
  /// Manage this Device's login credentials for remote unlock (this Device only).
  Credentials(crate::commands::device_local::CredentialsArgs),
  /// Read or change whether paired Devices may unlock this Device (this Device only).
  UnlockPolicy(crate::commands::device_local::UnlockPolicyArgs),
  /// Read this Device's lock and unlock audit records (this Device only).
  Audit(crate::commands::device_local::AuditArgs),
  /// Manage configured paired Device profiles (provisional).
  Profiles(DeviceProfilesArgs),
}

#[derive(Clone, Debug, Args)]
pub struct DeviceTrustArgs {
  /// Stable Device ID or human-facing Device name.
  pub device: String,
}

#[derive(Clone, Debug, Args)]
pub struct DeviceProfilesArgs {
  #[command(subcommand)]
  pub command: DeviceProfilesCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub enum DeviceProfilesCommand {
  List(ProfileOutputArgs),
  Get(ProfileGetArgs),
  Create(ProfileWriteArgs),
  Update(ProfileWriteArgs),
  Delete(ProfileDeleteArgs),
}

#[derive(Clone, Debug, Args)]
pub struct ProfileOutputArgs {
  #[arg(long)]
  pub json: bool,
}

#[derive(Clone, Debug, Args)]
pub struct ProfileGetArgs {
  pub name: String,
  #[arg(long)]
  pub json: bool,
}

#[derive(Clone, Debug, Args)]
pub struct ProfileDeleteArgs {
  pub name: String,
}

#[derive(Clone, Args)]
pub struct ProfileWriteArgs {
  pub name: String,
  #[arg(long)]
  pub device_id: String,
  #[arg(long)]
  pub device_name: String,
  #[arg(long)]
  pub endpoint: String,
  /// Opaque credential saved by `auv devices pair connect`.
  #[arg(long)]
  pub device_credential: String,
}

impl std::fmt::Debug for ProfileWriteArgs {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter
      .debug_struct("ProfileWriteArgs")
      .field("name", &self.name)
      .field("device_id", &self.device_id)
      .field("device_name", &self.device_name)
      .field("endpoint", &self.endpoint)
      .field("device_credential", &"[REDACTED]")
      .finish()
  }
}

#[derive(Clone, Debug, Args)]
pub struct DeviceListArgs {
  /// Override daemon discovery with an explicit endpoint.
  #[arg(long, value_name = "URI")]
  pub endpoint: Option<String>,
  /// Render machine-readable JSON.
  #[arg(long)]
  pub json: bool,
}

#[derive(Clone, Debug, Args)]
pub struct DeviceGetArgs {
  /// Stable Device ID.
  pub device_id: String,
  /// Override daemon discovery with an explicit endpoint.
  #[arg(long, value_name = "URI")]
  pub endpoint: Option<String>,
  /// Render machine-readable JSON.
  #[arg(long)]
  pub json: bool,
}

#[derive(Clone, Debug, Args)]
pub struct DeviceSessionsArgs {
  /// Override daemon discovery with an explicit endpoint.
  #[arg(long, value_name = "URI")]
  pub endpoint: Option<String>,
  /// Render machine-readable JSON.
  #[arg(long)]
  pub json: bool,
}

#[derive(Clone, Debug, Args)]
pub struct DeviceSessionTargetArgs {
  /// OS account with one existing session on the selected Device.
  #[arg(long, required_unless_present = "session", conflicts_with = "session")]
  pub user: Option<String>,
  /// Opaque selector from `auv devices sessions`.
  #[arg(long, required_unless_present = "user")]
  pub session: Option<String>,
  /// Override daemon discovery with an explicit endpoint.
  #[arg(long, value_name = "URI")]
  pub endpoint: Option<String>,
  /// Render machine-readable JSON.
  #[arg(long)]
  pub json: bool,
}

#[derive(TableRow)]
struct DeviceTableRow {
  #[table(header = "DEVICE ID")]
  device_id: String,
  name: Option<String>,
  platform: Option<String>,
  local: bool,
  status: String,
  profile: Option<String>,
}

#[derive(TableRow)]
struct DeviceProfileTableRow {
  #[table(header = "PROFILE")]
  config_profile: String,
  #[table(header = "DEVICE ID")]
  device_id: String,
  name: String,
  endpoint: String,
}

#[derive(TableRow)]
struct UserSessionTableRow {
  #[table(header = "SESSION")]
  selector: String,
  user: String,
  lock_state: String,
  connection_kind: String,
  seat: Option<String>,
}

pub async fn run(args: DevicesArgs, selection: &auv::selection::RootSelection, project_root: &std::path::Path) -> Result<i32, String> {
  use crate::commands::device_local::LocalRequest;

  match args.command {
    DevicesCommand::List(args) => list(args, selection).await,
    DevicesCommand::Get(args) => get(args, selection).await,
    DevicesCommand::Sessions(args) => sessions(args, selection).await,
    DevicesCommand::Unlock(args) => change_lock_state(args, selection, false).await,
    DevicesCommand::Lock(args) => change_lock_state(args, selection, true).await,
    DevicesCommand::Pair(args) => pairing(args, selection).await,
    DevicesCommand::Unpair(args) => trust(args, selection, TrustAction::Unpair).await,
    DevicesCommand::Enable(args) => trust(args, selection, TrustAction::Enable).await,
    DevicesCommand::Disable(args) => trust(args, selection, TrustAction::Disable).await,
    DevicesCommand::Profiles(args) => profiles(args),
    DevicesCommand::Credentials(args) => {
      local(selection, args.store.store_root, LocalRequest::Credentials(args.command), project_root).await
    }
    DevicesCommand::UnlockPolicy(args) => local(selection, args.store.store_root, LocalRequest::Policy(args.command), project_root).await,
    DevicesCommand::Audit(args) => local(selection, args.store.store_root, LocalRequest::Audit(args.command), project_root).await,
  }
}

/// Credentials, the unlock switch and audit stay with the Device they protect:
/// a credential is typed on that Device's terminal and never crosses the
/// network, so these commands do not follow Device or Run selection.
async fn local(
  selection: &auv::selection::RootSelection,
  store_root: Option<std::path::PathBuf>,
  request: crate::commands::device_local::LocalRequest,
  project_root: &std::path::Path,
) -> Result<i32, String> {
  if selection.device_name.is_some() || selection.device_id.is_some() || selection.run_id.is_some() {
    return Err("this command runs only on this Device; it cannot use --device, --device-id, or --run".to_string());
  }
  crate::commands::device_local::run(store_root.as_deref(), request, project_root).await
}

async fn list(args: DeviceListArgs, selection: &auv::selection::RootSelection) -> Result<i32, String> {
  let selector = selection.device_selector().map_err(|error| error.to_string())?;
  let mut devices = match auv::Client::discover(args.endpoint.as_deref()).await {
    Ok(Some(client)) => client.devices().list().await.map_err(|error| error.to_string())?,
    Ok(None) => Vec::new(),
    Err(error) if args.endpoint.is_none() => {
      eprintln!("warning: local AUV daemon is unavailable: {error}");
      Vec::new()
    }
    Err(error) => return Err(error.to_string()),
  };
  if let Some(selector) = &selector {
    devices.retain(|device| selector.matches(&device.id, &device.name));
  }
  let profile_store = auv::profile::ProfileStore::from_env().map_err(|error| error.to_string())?;
  let mut statuses = auv::devices::Devices::probe_configured(&profile_store).await.map_err(|error| error.to_string())?;
  if let Some(selector) = &selector {
    statuses.retain(|status| status.matches(selector));
  }

  if args.json {
    let mut values = devices
      .iter()
      .map(|device| {
        let mut value = device_json(device);
        value["source"] = serde_json::json!("daemon");
        value["status"] = serde_json::json!("online");
        value
      })
      .collect::<Vec<_>>();
    for status in &statuses {
      if devices.iter().any(|device| device.id.as_str() == status.profile.device_id()) {
        continue;
      }
      values.push(configured_device_json(status));
    }
    println!("{}", serde_json::to_string_pretty(&values).map_err(|error| format!("failed to encode Device list: {error}"))?);
  } else {
    let mut rows = devices.iter().map(|device| device_table_row(device, "online")).collect::<Vec<_>>();
    for status in &statuses {
      if devices.iter().any(|device| device.id.as_str() == status.profile.device_id()) {
        continue;
      }
      rows.push(configured_device_table_row(status));
    }
    print_table(&rows, "(no devices)");
  }
  Ok(0)
}

async fn get(args: DeviceGetArgs, selection: &auv::selection::RootSelection) -> Result<i32, String> {
  let (client, resolved) = auv::Client::selected(args.endpoint.as_deref(), selection)
    .await
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "no AUV daemon was discovered".to_string())?;
  let requested = auv::resource::DeviceSelector::parse_id(&args.device_id).map_err(|error| error.to_string())?;
  let device = client.devices().get(&requested).await.map_err(|error| error.to_string())?;
  device.validate_selection(resolved.device.as_ref()).map_err(|error| error.to_string())?;
  if args.json {
    println!("{}", serde_json::to_string_pretty(&device_json(&device)).map_err(|error| format!("failed to encode Device: {error}"))?);
  } else {
    print_table(&[device_table_row(&device, "online")], "(no device)");
  }
  Ok(0)
}

async fn sessions(args: DeviceSessionsArgs, selection: &auv::selection::RootSelection) -> Result<i32, String> {
  let (client, _) = auv::Client::selected(args.endpoint.as_deref(), selection)
    .await
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "no AUV daemon was discovered".to_string())?;

  let sessions = client.devices().list_user_sessions().await.map_err(|error| error.to_string())?;

  if args.json {
    let values = sessions
      .iter()
      .map(|session| {
        serde_json::json!({
          "session_selector": session.selector,
          "user": session.user,
          "lock_state": session.lock_state.as_str(),
          "connection_kind": session.connection_kind.as_str(),
          "seat": session.seat,
        })
      })
      .collect::<Vec<_>>();
    println!("{}", serde_json::to_string_pretty(&values).map_err(|error| format!("failed to encode login sessions: {error}"))?);
  } else {
    let rows = sessions
      .iter()
      .map(|session| UserSessionTableRow {
        selector: session.selector.clone(),
        user: session.user.clone(),
        lock_state: session.lock_state.as_str().to_lowercase(),
        connection_kind: session.connection_kind.as_str().to_lowercase(),
        seat: session.seat.clone(),
      })
      .collect::<Vec<_>>();
    print_table(&rows, "(no login sessions)");
  }

  Ok(0)
}

async fn change_lock_state(args: DeviceSessionTargetArgs, selection: &auv::selection::RootSelection, lock: bool) -> Result<i32, String> {
  let (client, _) = auv::Client::selected(args.endpoint.as_deref(), selection)
    .await
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "no AUV daemon was discovered".to_string())?;

  let target = match (args.user, args.session) {
    (Some(user), None) => auv::devices::UserSessionTarget::User(user),
    (None, Some(session)) => auv::devices::UserSessionTarget::SessionSelector(session),
    _ => return Err("specify exactly one of --user or --session".to_string()),
  };
  let (kind, user, session_selector) = if lock {
    let effect = client.devices().ensure_user_session_locked(target).await.map_err(|error| error.to_string())?;
    (effect.kind.as_str(), effect.user, Some(effect.session_selector))
  } else {
    let effect = client.devices().ensure_user_session_unlocked(target).await.map_err(|error| error.to_string())?;
    (effect.kind.as_str(), effect.user, effect.session_selector)
  };

  if args.json {
    println!(
      "{}",
      serde_json::to_string_pretty(&serde_json::json!({
        "effect": kind,
        "user": user,
        "session_selector": session_selector,
      }))
      .map_err(|error| format!("failed to encode Device entry effect: {error}"))?
    );
  } else {
    match session_selector {
      Some(selector) => println!("{}: {} ({selector})", kind.to_lowercase(), user),
      None => println!("{}: {}", kind.to_lowercase(), user),
    }
  }

  Ok(0)
}

fn profiles(args: DeviceProfilesArgs) -> Result<i32, String> {
  let store = auv::profile::ProfileStore::from_env().map_err(|error| error.to_string())?;
  match args.command {
    DeviceProfilesCommand::List(args) => {
      let profiles = match store.list_devices() {
        Ok(profiles) => profiles,
        Err(auv::profile::ProfileError::Open { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error.to_string()),
      };
      if args.json {
        let values = profiles.iter().map(configured_profile_json).collect::<Vec<_>>();
        println!("{}", serde_json::to_string_pretty(&values).map_err(|error| error.to_string())?);
      } else {
        print_table(&profiles.iter().map(device_profile_table_row).collect::<Vec<_>>(), "(no device profiles)");
      }
    }
    DeviceProfilesCommand::Get(args) => {
      let profile = store.get_device(&args.name).map_err(|error| error.to_string())?;
      if args.json {
        println!("{}", serde_json::to_string_pretty(&configured_profile_json(&profile)).map_err(|error| error.to_string())?);
      } else {
        print_table(&[device_profile_table_row(&profile)], "(no device profile)");
      }
    }
    DeviceProfilesCommand::Create(args) => store.create(&args.name, profile_inputs(&args)).map_err(|error| error.to_string())?,
    DeviceProfilesCommand::Update(args) => store.update(&args.name, profile_inputs(&args)).map_err(|error| error.to_string())?,
    DeviceProfilesCommand::Delete(args) => store.delete(&args.name).map_err(|error| error.to_string())?,
  }
  Ok(0)
}

#[derive(Clone, Copy)]
enum TrustAction {
  Unpair,
  Enable,
  Disable,
}

async fn trust(args: DeviceTrustArgs, selection: &auv::selection::RootSelection, action: TrustAction) -> Result<i32, String> {
  let (client, _) = auv::Client::selected(None, selection)
    .await
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "no AUV daemon was discovered".to_string())?;
  let selector = auv::pairing::PairedDeviceSelector::parse(&args.device).map_err(|error| error.to_string())?;
  match action {
    TrustAction::Unpair => client.pairing().unpair(&selector).await,
    TrustAction::Enable => client.pairing().set_enabled(&selector, true).await,
    TrustAction::Disable => client.pairing().set_enabled(&selector, false).await,
  }
  .map_err(|error| error.to_string())?;
  Ok(0)
}

async fn pairing(args: PairingArgs, selection: &auv::selection::RootSelection) -> Result<i32, String> {
  match args.command {
    PairingCommand::CreateToken { ttl } => {
      let (client, _) = auv::Client::selected(args.endpoint.as_deref(), selection)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "no AUV daemon was discovered".to_string())?;
      println!("{}", client.pairing().create_token(ttl.map(std::time::Duration::from_secs)).await.map_err(|error| error.to_string())?);
    }
    PairingCommand::Connect {
      token,
      token_stdin,
      device_id,
      label,
      profile,
      json,
    } => {
      let endpoint = args.endpoint.ok_or_else(|| "pair connect requires --endpoint http://HOST:PORT".to_string())?;
      let token = if token_stdin {
        use std::io::Read as _;
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input).map_err(|error| format!("failed to read bootstrap token from stdin: {error}"))?;
        let token = input.trim().to_owned();
        if token.is_empty() {
          return Err("bootstrap token from stdin is empty".to_string());
        }
        token
      } else {
        token.ok_or("pair connect requires --token or --token-stdin")?
      };
      tracing::info!(endpoint, service = "auv.api.daemon.v1.PairingService", method = "PairDevice", "calling bootstrap RPC");
      let store = auv::profile::ProfileStore::from_env().map_err(|error| error.to_string())?;
      let enrollment = auv::pairing::Pairing::enroll(
        auv::pairing::EnrollmentRequest {
          endpoint,
          token,
          client_device_id: device_id,
          label,
          profile,
        },
        &store,
      )
      .await
      .map_err(|error| error.to_string())?;
      if json {
        println!(
          "{}",
          serde_json::to_string_pretty(&serde_json::json!({
            "device_id": enrollment.device_id.as_str(),
            "device_name": enrollment.device_name,
            "endpoint": enrollment.endpoint,
            "profile": enrollment.profile,
            "credentials_file": enrollment.credentials_file,
          }))
          .map_err(|error| error.to_string())?
        );
      } else {
        let display_name = if enrollment.device_name.is_empty() {
          &enrollment.profile
        } else {
          &enrollment.device_name
        };
        println!("Connected to {display_name} ({})", enrollment.device_id.short());
        println!("Profile: {}", enrollment.profile);
        println!("Credentials saved in {}", enrollment.credentials_file.display());
      }
    }
    PairingCommand::Enable { pair_id } => {
      let selector = auv::pairing::PairedDeviceSelector::parse(&pair_id).map_err(|error| error.to_string())?;
      auv::Client::selected(args.endpoint.as_deref(), selection)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "no AUV daemon was discovered".to_string())?
        .0
        .pairing()
        .set_enabled(&selector, true)
        .await
        .map_err(|error| error.to_string())?;
    }
    PairingCommand::Disable { pair_id } => {
      let selector = auv::pairing::PairedDeviceSelector::parse(&pair_id).map_err(|error| error.to_string())?;
      auv::Client::selected(args.endpoint.as_deref(), selection)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "no AUV daemon was discovered".to_string())?
        .0
        .pairing()
        .set_enabled(&selector, false)
        .await
        .map_err(|error| error.to_string())?;
    }
  }
  Ok(0)
}

fn device_table_row(device: &auv::devices::Device, status: &str) -> DeviceTableRow {
  DeviceTableRow {
    device_id: device.id.short(),
    name: (!device.name.is_empty()).then(|| device.name.clone()),
    platform: Some(platform_name(device.platform, false)),
    local: device.local,
    status: status.to_string(),
    profile: None,
  }
}

fn configured_device_table_row(probe: &auv::devices::ConfiguredDeviceStatus) -> DeviceTableRow {
  DeviceTableRow {
    device_id: probe.profile.device_id().replace('-', "").chars().take(12).collect(),
    name: probe
      .remote
      .as_ref()
      .and_then(|device| (!device.name.is_empty()).then(|| device.name.clone()))
      .or_else(|| (!probe.profile.device_name().is_empty()).then(|| probe.profile.device_name().to_string())),
    platform: probe.remote.as_ref().map(|device| platform_name(device.platform, false)),
    local: false,
    status: availability_name(probe.availability).to_string(),
    profile: Some(probe.profile.config_profile().to_string()),
  }
}

fn device_json(device: &auv::devices::Device) -> serde_json::Value {
  serde_json::json!({
    "device_id": device.id.as_str(), "name": device.name, "platform": platform_name(device.platform, true),
    "local": device.local, "labels": device.labels,
  })
}

fn configured_device_json(probe: &auv::devices::ConfiguredDeviceStatus) -> serde_json::Value {
  serde_json::json!({
    "device_id": probe.profile.device_id(),
    "name": probe.remote.as_ref().map(|device| device.name.as_str()).filter(|name| !name.is_empty()).unwrap_or(probe.profile.device_name()),
    "platform": probe.remote.as_ref().map(|device| platform_name(device.platform, true)), "local": false,
    "source": "configured_profile", "status": availability_name(probe.availability),
    "config_profile": probe.profile.config_profile(), "endpoint": probe.profile.endpoint().to_string(),
    "labels": probe.remote.as_ref().map(|device| &device.labels),
  })
}

fn configured_profile_json(profile: &auv::profile::ConfiguredDevice) -> serde_json::Value {
  serde_json::json!({
    "device_id": profile.device_id(), "name": profile.device_name(), "platform": null, "local": false,
    "source": "configured_profile", "status": "configured", "config_profile": profile.config_profile(),
    "endpoint": profile.endpoint().to_string(), "labels": null,
  })
}

fn device_profile_table_row(device: &auv::profile::ConfiguredDevice) -> DeviceProfileTableRow {
  DeviceProfileTableRow {
    config_profile: device.config_profile().to_string(),
    device_id: device.device_id().replace('-', "").chars().take(12).collect(),
    name: device.device_name().to_string(),
    endpoint: device.endpoint().to_string(),
  }
}

fn profile_inputs(args: &ProfileWriteArgs) -> auv::profile::DeviceProfileInput {
  auv::profile::DeviceProfileInput {
    device_id: args.device_id.clone(),
    device_name: args.device_name.clone(),
    endpoint: args.endpoint.clone(),
    device_credential: args.device_credential.clone(),
  }
}

fn availability_name(value: auv::devices::DeviceAvailability) -> &'static str {
  match value {
    auv::devices::DeviceAvailability::Online => "online",
    auv::devices::DeviceAvailability::Offline => "offline",
    auv::devices::DeviceAvailability::Unauthorized => "unauthorized",
    auv::devices::DeviceAvailability::Invalid => "invalid",
    auv::devices::DeviceAvailability::Error => "error",
  }
}

fn platform_name(value: auv::devices::DevicePlatform, wire: bool) -> String {
  let name = match value {
    auv::devices::DevicePlatform::Unspecified => "UNSPECIFIED",
    auv::devices::DevicePlatform::Linux => "LINUX",
    auv::devices::DevicePlatform::Macos => "MACOS",
    auv::devices::DevicePlatform::Windows => "WINDOWS",
  };
  if wire {
    format!("DEVICE_PLATFORM_{name}")
  } else {
    name.to_ascii_lowercase()
  }
}

fn print_table<R: table::TableRow>(rows: &[R], empty_message: &'static str) {
  println!("{}", table::render(rows, TableOptions::default().empty_message(empty_message)));
}

/// Create and consume short paired-Device enrollment tokens.
#[derive(Clone, Debug, Args)]
#[command(
  after_long_help = "Examples:\n  # On the daemon host: create a one-time bootstrap token\n  auv devices pair create-token\n\n  # On the client machine: consume the token and save a profile\n  auv devices pair --endpoint http://HOST:9847 connect --token <TOKEN> --label \"MacBook\" --profile node1\n\n  # Confirm that the paired Device is online\n  auv devices list\n\n  # Invoke a typed operation through the paired daemon\n  auv --device <NAME> invoke display.list"
)]
pub struct PairingArgs {
  /// Daemon endpoint. Create-token defaults to local discovery; connect must
  /// name the remote http:// endpoint that issued the token.
  #[arg(long, global = true, value_name = "URI")]
  pub endpoint: Option<String>,

  #[command(subcommand)]
  pub command: PairingCommand,
}

#[derive(Clone, Subcommand)]
pub enum PairingCommand {
  /// Create a cryptographically random bootstrap token and display it once.
  CreateToken {
    /// Optional token lifetime in seconds. Omit for no expiry.
    #[arg(long, value_name = "SECONDS")]
    ttl: Option<u64>,
  },
  /// Connect to a Device and save its credential as a local profile.
  #[command(group(ArgGroup::new("bootstrap_token").required(true).multiple(false).args(["token", "token_stdin"])))]
  Connect {
    #[arg(long)]
    token: Option<String>,
    /// Read the bootstrap token from standard input so it never enters the
    /// process command line or shell history.
    #[arg(long)]
    token_stdin: bool,
    /// Stable caller identity. An opaque random ID is generated when omitted.
    #[arg(long)]
    device_id: Option<String>,
    /// Human-readable label; never used as identity.
    #[arg(long)]
    label: String,
    /// Local profile name. Defaults to the remote Device name or short ID.
    #[arg(long)]
    profile: Option<String>,
    /// Render non-secret connection metadata as JSON.
    #[arg(long)]
    json: bool,
  },
  /// Enable a paired device.
  // NOTICE(pairing-cli-compat): Keep the pair-ID form for existing scripts;
  // resource-oriented callers should use `auv devices enable <device>`.
  Enable { pair_id: String },
  /// Disable a paired device without deleting its history.
  // NOTICE(pairing-cli-compat): Keep the pair-ID form for existing scripts;
  // resource-oriented callers should use `auv devices disable <device>`.
  Disable { pair_id: String },
}

impl std::fmt::Debug for PairingCommand {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::CreateToken { ttl } => formatter.debug_struct("CreateToken").field("ttl", ttl).finish(),
      Self::Connect {
        device_id,
        label,
        profile,
        json,
        ..
      } => formatter
        .debug_struct("Connect")
        .field("token", &"[REDACTED]")
        .field("device_id", device_id)
        .field("label", label)
        .field("profile", profile)
        .field("json", json)
        .finish(),
      Self::Enable { pair_id } => formatter.debug_struct("Enable").field("pair_id", pair_id).finish(),
      Self::Disable { pair_id } => formatter.debug_struct("Disable").field("pair_id", pair_id).finish(),
    }
  }
}

#[cfg(test)]
#[path = "pairing_test.rs"]
mod tests;
