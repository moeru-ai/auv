use std::any::Any;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use auv_tracing::ArtifactMetadata;
use clap::{Arg, ArgAction, Command, FromArgMatches, error::ErrorKind};
use serde::Serialize;
use serde::de::{DeserializeOwned, IntoDeserializer, Visitor, value::MapDeserializer};

use crate::InvokeReport;

pub type InvokeCommandFuture = std::pin::Pin<Box<dyn std::future::Future<Output = InvokeExecutionResult> + Send + 'static>>;
pub type InvokeCommandHandler = fn(InvokeCommandInput) -> InvokeCommandFuture;
type InvokeCommandParser = fn(&'static str, &'static str, TargetPolicy, &[String]) -> Result<InvokeCommandCliParse, String>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InvokeCommandCliParse {
  Help,
  Invoke {
    target: Option<crate::ExecutionTarget>,
    inputs: BTreeMap<String, String>,
    typed_args: TypedInvokeArgs,
    store_root: Option<PathBuf>,
    dry_run: bool,
    json: bool,
    detail: bool,
    wide: bool,
    overlay_enabled: bool,
  },
}

/// Type-erased command-local clap arguments carried from CLI parsing to the
/// registered handler without converting them back through protocol strings.
#[derive(Clone)]
pub struct TypedInvokeArgs(Arc<dyn Any + Send + Sync>);

impl std::fmt::Debug for TypedInvokeArgs {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.write_str("TypedInvokeArgs(..)")
  }
}

impl PartialEq for TypedInvokeArgs {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.0, &other.0)
  }
}

impl Eq for TypedInvokeArgs {}

impl TypedInvokeArgs {
  fn new<T: Any + Send + Sync>(args: T) -> Self {
    Self(Arc::new(args))
  }

  pub(crate) fn get<T: Any>(&self) -> Option<&T> {
    self.0.downcast_ref()
  }
}

/// Cloneable cancellation shared by one frontend dispatch and its typed command.
#[derive(Clone, Debug)]
pub struct InvokeCancellation {
  token: Arc<tokio_util::sync::CancellationToken>,
}

impl InvokeCancellation {
  pub fn new() -> Self {
    Self {
      token: Arc::new(tokio_util::sync::CancellationToken::new()),
    }
  }

  pub fn from_token(token: tokio_util::sync::CancellationToken) -> Self {
    Self {
      token: Arc::new(token),
    }
  }

  pub fn cancel(&self) {
    self.token.cancel();
  }

  pub fn is_cancelled(&self) -> bool {
    self.token.is_cancelled()
  }

  pub fn check(&self) -> Result<(), InvokeCancelled> {
    if self.is_cancelled() {
      Err(InvokeCancelled)
    } else {
      Ok(())
    }
  }

  pub async fn cancelled(&self) {
    self.token.cancelled().await;
  }
}

impl Default for InvokeCancellation {
  fn default() -> Self {
    Self::new()
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("invoke cancelled")]
pub struct InvokeCancelled;

#[derive(Clone, Debug)]
pub struct InvokeCommandInput {
  pub command_id: String,
  pub target: Option<crate::ExecutionTarget>,
  pub inputs: BTreeMap<String, String>,
  pub typed_args: Option<TypedInvokeArgs>,
  pub dry_run: bool,
  pub cancellation: InvokeCancellation,
}

impl InvokeCommandInput {
  pub fn required_input(&self, name: &str) -> Result<&str, String> {
    self
      .inputs
      .get(name)
      .map(String::as_str)
      .filter(|value| !value.trim().is_empty())
      .ok_or_else(|| format!("{} requires --{name}", self.command_id))
  }

  pub fn application_target(&self) -> Result<Option<&str>, String> {
    match self.target.as_ref() {
      Some(crate::ExecutionTarget::Application { id }) => Ok(Some(id)),
      Some(crate::ExecutionTarget::Window { .. }) => Err(format!("{} requires an app: target, not window:", self.command_id)),
      Some(crate::ExecutionTarget::Display { .. }) => Err(format!("{} requires an app: target, not display:", self.command_id)),
      None => Ok(self.inputs.get("target").map(String::as_str).filter(|value| !value.trim().is_empty())),
    }
  }

  /// Resolves the shared invoke presentation policy. Overlay presentation is
  /// enabled unless a frontend explicitly supplies `--no-overlay` or
  /// `--overlay false`.
  pub fn overlay_enabled(&self) -> Result<bool, String> {
    self.inputs.get("overlay").map_or(Ok(true), |value| {
      value.parse::<bool>().map_err(|error| format!("{} received invalid --overlay value {value:?}: {error}", self.command_id))
    })
  }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InvokeCommandOutput {
  pub report: Option<InvokeReport>,
  result: Option<serde_json::Value>,
  artifacts: Vec<ArtifactMetadata>,
}

impl InvokeCommandOutput {
  pub fn completed() -> Self {
    Self::default()
  }

  pub fn from_result<T>(result: &T) -> Result<Self, String>
  where
    T: serde::Serialize + ?Sized,
  {
    Ok(Self {
      report: None,
      result: Some(serde_json::to_value(result).map_err(|error| format!("failed to serialize invoke result: {error}"))?),
      artifacts: Vec::new(),
    })
  }

  pub fn with_report(mut self, report: InvokeReport) -> Self {
    self.report = Some(report);
    self
  }

  /// Attaches artifacts that are part of the direct command result.
  pub fn with_artifacts(mut self, artifacts: impl IntoIterator<Item = ArtifactMetadata>) -> Self {
    self.artifacts.extend(artifacts);
    self
  }

  pub fn artifacts(&self) -> &[ArtifactMetadata] {
    &self.artifacts
  }

  /// Returns the direct typed operation result encoded for frontend transport.
  pub fn result(&self) -> Option<&serde_json::Value> {
    self.result.as_ref()
  }
}

pub type InvokeCommandResult = Result<InvokeCommandOutput, String>;

/// Frontend execution failure. Existing handler helpers may still return text;
/// target and driver boundaries retain machine-readable categories here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, thiserror::Error)]
#[error("{message}")]
pub struct InvokeFailure {
  pub code: FailureCode,
  pub message: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub keyboard_progress: Option<auv_driver::KeyboardInputProgress>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCode {
  CommandFailed,
  InvalidTarget,
  NotFound,
  Unsupported,
  PermissionDenied,
  InvalidInput,
  StaleObservation,
  RoleMismatch,
  Backend,
}

impl InvokeFailure {
  pub fn new(code: FailureCode, message: impl Into<String>) -> Self {
    Self {
      code,
      message: message.into(),
      keyboard_progress: None,
    }
  }
}

// TODO(invoke-driver-failure-migration): legacy non-keyboard helpers retain
// command_failed until their owner approves preserving typed errors end to end.
impl From<String> for InvokeFailure {
  fn from(message: String) -> Self {
    Self::new(FailureCode::CommandFailed, message)
  }
}

impl From<auv_driver::DriverError> for InvokeFailure {
  fn from(error: auv_driver::DriverError) -> Self {
    use auv_driver::DriverError;
    let code = match &error {
      DriverError::Unsupported { .. } => FailureCode::Unsupported,
      DriverError::NotFound { .. } => FailureCode::NotFound,
      DriverError::PermissionDenied { .. } => FailureCode::PermissionDenied,
      DriverError::InvalidInput { .. } => FailureCode::InvalidInput,
      DriverError::StaleObservation { .. } => FailureCode::StaleObservation,
      DriverError::RoleMismatch { .. } => FailureCode::RoleMismatch,
      DriverError::Backend { .. } => FailureCode::Backend,
    };
    Self::new(code, error.to_string())
  }
}

impl From<auv_driver::KeyboardInputError> for InvokeFailure {
  fn from(error: auv_driver::KeyboardInputError) -> Self {
    let message = error.to_string();
    let mut failure = Self::from(error.cause);
    failure.message = message;
    failure.keyboard_progress = Some(error.progress);
    failure
  }
}

impl From<auv::client::runner::CapabilityError> for InvokeFailure {
  fn from(error: auv::client::runner::CapabilityError) -> Self {
    use auv::error::ClientErrorKind;
    let code = match error.client_kind() {
      Some(ClientErrorKind::NotFound) => FailureCode::NotFound,
      Some(ClientErrorKind::Unsupported) => FailureCode::Unsupported,
      Some(ClientErrorKind::Unauthorized) => FailureCode::PermissionDenied,
      Some(ClientErrorKind::InvalidRequest) => FailureCode::InvalidInput,
      Some(ClientErrorKind::Conflict | ClientErrorKind::Ambiguous) => FailureCode::StaleObservation,
      _ => FailureCode::Backend,
    };
    let mut failure = Self::new(code, error.to_string());
    if let auv::client::runner::CapabilityError::KeyboardInput { progress, .. } = error {
      failure.keyboard_progress = Some(progress);
    }
    failure
  }
}

pub type InvokeExecutionResult = Result<InvokeCommandOutput, InvokeFailure>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum InvokeNamespace {
  Display,
  Screen,
  Window,
  Input,
  App,
  Game,
  Overlay,
  MediaControl,
  Fixture,
  Scan,
}

impl InvokeNamespace {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Display => "display",
      Self::Screen => "screen",
      Self::Window => "window",
      Self::Input => "input",
      Self::App => "app",
      Self::Game => "game",
      Self::Overlay => "overlay",
      Self::MediaControl => "mediaControl",
      Self::Fixture => "fixture",
      Self::Scan => "scan",
    }
  }
}

#[derive(Clone, Debug)]
pub struct InvokeCommand {
  pub id: &'static str,
  pub namespace: InvokeNamespace,
  pub description: &'static str,
  pub target: TargetPolicy,
  typed_command: fn(&'static str, &'static str) -> Command,
  typed_parse: InvokeCommandParser,
  handler: InvokeCommandHandler,
}

/// Target constraints shared by handler validation, CLI help, and MCP metadata.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetPolicy {
  #[default]
  Forbidden,
  OptionalApplication,
  RequiredApplication,
  OptionalKeyboard,
  OptionalPoint,
}

impl TargetPolicy {
  pub fn accepted_types(self) -> &'static [&'static str] {
    match self {
      Self::Forbidden => &[],
      Self::OptionalApplication | Self::RequiredApplication => &["application"],
      Self::OptionalKeyboard => &["application", "window"],
      Self::OptionalPoint => &["application", "window", "display"],
    }
  }
  pub fn required(self) -> bool {
    self == Self::RequiredApplication
  }

  pub fn help(self) -> &'static str {
    match self {
      Self::Forbidden => "Target forbidden; this command operates without target selection.",
      Self::OptionalApplication => "Optional app:<bundle-id> (bare ids accepted); window and display targets are unsupported.",
      Self::RequiredApplication => "Required app:<bundle-id> (bare ids accepted); window and display targets are unsupported.",
      Self::OptionalKeyboard => {
        "Optional app:<bundle-id> (bare ids accepted) or window:<window-id>. Default foreground-preferred activates the app; a window target also requires exact window focus. Background policies do not activate or fall back to foreground. Does not establish text-control focus. Without target, uses current foreground focus. Display targets are unsupported."
      }
      Self::OptionalPoint => {
        "Optional app:<bundle-id>, window:<window-id>, or display:<display-id>; coordinate basis defaults from target. Bare values select an application."
      }
    }
  }

  pub fn validate(self, input: &InvokeCommandInput) -> Result<(), String> {
    use crate::ExecutionTarget;
    let target = input.target.as_ref();
    let valid = match (self, target) {
      (Self::RequiredApplication, None) => false,
      (_, None) => true,
      (Self::OptionalPoint, Some(_)) => true,
      (Self::OptionalKeyboard, Some(ExecutionTarget::Application { .. } | ExecutionTarget::Window { .. })) => true,
      (Self::OptionalApplication | Self::RequiredApplication, Some(ExecutionTarget::Application { .. })) => true,
      _ => false,
    };
    if !valid {
      return Err(match (self, target) {
        (Self::RequiredApplication, None) => format!("{} requires --target app:", input.command_id),
        (Self::Forbidden, _) => format!("{} forbids --target", input.command_id),
        (_, Some(target)) => {
          let kind = match target {
            ExecutionTarget::Application { .. } => "app",
            ExecutionTarget::Window { .. } => "window",
            ExecutionTarget::Display { .. } => "display",
          };
          format!("{} does not support {kind}: targets; accepted target types: {}", input.command_id, self.accepted_types().join(", "))
        }
        _ => unreachable!("only invalid combinations are reported"),
      });
    }
    if let Some(target) = target {
      let id = match target {
        ExecutionTarget::Application { id } | ExecutionTarget::Window { id } | ExecutionTarget::Display { id } => id,
      };
      if id.trim().is_empty() {
        return Err(format!("{}: target id cannot be empty", input.command_id));
      }
    }
    Ok(())
  }
}

impl InvokeCommand {
  pub fn with_target(mut self, target: TargetPolicy) -> Self {
    self.target = target;
    self
  }

  pub fn invoke(&self, input: InvokeCommandInput) -> InvokeCommandFuture {
    if let Err(error) = input.cancellation.check() {
      return Box::pin(async move { Err(error.to_string().into()) });
    }
    if let Err(error) = self.target.validate(&input) {
      return Box::pin(async move { Err(InvokeFailure::new(FailureCode::InvalidTarget, error)) });
    }
    (self.handler)(input)
  }

  pub fn clap_command(&self) -> Command {
    (self.typed_command)(self.id, self.description)
  }

  pub fn parse_cli_args(&self, arguments: &[String]) -> Result<InvokeCommandCliParse, String> {
    (self.typed_parse)(self.id, self.description, self.target, arguments)
  }
}

#[derive(Clone, Debug)]
pub struct CommandGroup {
  pub name: &'static str,
  pub heading: &'static str,
  pub children: Vec<CommandNode>,
}

impl CommandGroup {
  pub fn new(name: &'static str, heading: &'static str) -> Self {
    Self {
      name,
      heading,
      children: Vec::new(),
    }
  }

  pub fn command(mut self, command: InvokeCommand) -> Self {
    self.children.push(CommandNode::Command(command));
    self
  }

  pub fn group(mut self, group: CommandGroup) -> Self {
    self.children.push(CommandNode::Group(group));
    self
  }
}

#[derive(Clone, Debug)]
pub enum CommandNode {
  Command(InvokeCommand),
  Group(CommandGroup),
}

/// Build one handler-first invoke command from command-local clap arguments.
#[doc(hidden)]
pub fn typed_spec<T>(id: &'static str, namespace: InvokeNamespace, description: &'static str, handler: InvokeCommandHandler) -> InvokeCommand
where
  T: clap::Args + FromArgMatches + Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
{
  InvokeCommand {
    id,
    namespace,
    description,
    target: TargetPolicy::default(),
    typed_command: typed_command::<T>,
    typed_parse: parse_cli_args::<T>,
    handler,
  }
}

#[doc(hidden)]
pub fn decode_args<T>(input: &InvokeCommandInput) -> Result<T, String>
where
  T: clap::Args + Clone + DeserializeOwned + 'static,
{
  if let Some(args) = input.typed_args.as_ref().and_then(TypedInvokeArgs::get::<T>) {
    return Ok(args.clone());
  }
  let mut protocol_inputs = input.inputs.clone();
  let command = T::augment_args(Command::new("invoke-command").disable_help_flag(true));
  for argument in command.get_arguments() {
    let Some(long) = argument.get_long() else {
      continue;
    };
    let id = argument.get_id().as_str();
    if !protocol_inputs.contains_key(long)
      && let Some(value) = protocol_inputs.get(id).cloned()
    {
      protocol_inputs.insert(long.to_string(), value);
    }
  }
  let values = protocol_inputs.iter().map(|(name, value)| (name.as_str().into_deserializer(), ProtocolValue(value)));
  T::deserialize(MapDeserializer::<_, serde::de::value::Error>::new(values)).map_err(|error| error.to_string())
}

#[doc(hidden)]
pub fn encode_args<T: Serialize>(args: &T) -> Result<BTreeMap<String, String>, String> {
  let value = serde_json::to_value(args).map_err(|error| format!("failed to encode typed invoke arguments: {error}"))?;
  let object = value.as_object().ok_or_else(|| "typed invoke arguments must serialize as an object".to_string())?;
  let mut inputs = BTreeMap::new();
  for (name, value) in object {
    let text = match value {
      serde_json::Value::Null => continue,
      serde_json::Value::String(value) => value.clone(),
      serde_json::Value::Bool(value) => value.to_string(),
      serde_json::Value::Number(value) => value.to_string(),
      other => serde_json::to_string(other).map_err(|error| format!("failed to encode invoke argument {name}: {error}"))?,
    };
    inputs.insert(name.clone(), text);
  }
  Ok(inputs)
}

#[doc(hidden)]
pub fn deserialize_optional_nonzero_u8<'de, D>(deserializer: D) -> Result<Option<u8>, D::Error>
where
  D: serde::Deserializer<'de>,
{
  let value = <Option<u8> as serde::Deserialize>::deserialize(deserializer)?;
  match value {
    Some(0) => Err(serde::de::Error::custom("expected an integer within 1..=255")),
    value => Ok(value),
  }
}

/// Deserializes one protocol string directly into the field type requested by
/// the command input. This keeps MCP decoding independent from CLI argv while
/// preserving the existing string-valued invoke protocol.
struct ProtocolValue<'a>(&'a str);

impl<'de> IntoDeserializer<'de, serde::de::value::Error> for ProtocolValue<'de> {
  type Deserializer = Self;

  fn into_deserializer(self) -> Self::Deserializer {
    self
  }
}

macro_rules! deserialize_number {
  ($method:ident, $visit:ident, $ty:ty) => {
    fn $method<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
      V: Visitor<'de>,
    {
      let value = self.0.parse::<$ty>().map_err(serde::de::Error::custom)?;
      visitor.$visit(value)
    }
  };
}

impl<'de> serde::Deserializer<'de> for ProtocolValue<'de> {
  type Error = serde::de::value::Error;

  fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
  where
    V: Visitor<'de>,
  {
    visitor.visit_borrowed_str(self.0)
  }

  fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
  where
    V: Visitor<'de>,
  {
    visitor.visit_bool(self.0.parse::<bool>().map_err(serde::de::Error::custom)?)
  }

  deserialize_number!(deserialize_i8, visit_i8, i8);
  deserialize_number!(deserialize_i16, visit_i16, i16);
  deserialize_number!(deserialize_i32, visit_i32, i32);
  deserialize_number!(deserialize_i64, visit_i64, i64);
  deserialize_number!(deserialize_i128, visit_i128, i128);
  deserialize_number!(deserialize_u8, visit_u8, u8);
  deserialize_number!(deserialize_u16, visit_u16, u16);
  deserialize_number!(deserialize_u32, visit_u32, u32);
  deserialize_number!(deserialize_u64, visit_u64, u64);
  deserialize_number!(deserialize_u128, visit_u128, u128);
  deserialize_number!(deserialize_f32, visit_f32, f32);
  deserialize_number!(deserialize_f64, visit_f64, f64);

  fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
  where
    V: Visitor<'de>,
  {
    visitor.visit_some(self)
  }

  fn deserialize_enum<V>(self, name: &'static str, variants: &'static [&'static str], visitor: V) -> Result<V::Value, Self::Error>
  where
    V: Visitor<'de>,
  {
    self.0.into_deserializer().deserialize_enum(name, variants, visitor)
  }

  // Repeated CLI arguments are JSON arrays in the string-valued invoke wire
  // format. Only sequence fields decode JSON; numeric-looking text stays text.
  fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
  where
    V: Visitor<'de>,
  {
    let value: serde_json::Value = serde_json::from_str(self.0).map_err(serde::de::Error::custom)?;
    value.deserialize_seq(visitor).map_err(serde::de::Error::custom)
  }

  serde::forward_to_deserialize_any! {
    char str string bytes byte_buf unit unit_struct newtype_struct tuple
    tuple_struct map struct identifier ignored_any
  }
}

fn typed_command<T>(id: &'static str, description: &'static str) -> Command
where
  T: clap::Args,
{
  T::augment_args(Command::new(id).bin_name(format!("auv invoke {id}")).about(description))
}

fn parse_cli_args<T>(
  id: &'static str,
  description: &'static str,
  target: TargetPolicy,
  arguments: &[String],
) -> Result<InvokeCommandCliParse, String>
where
  T: clap::Args + FromArgMatches + Clone + Serialize + Send + Sync + 'static,
{
  let mut command = with_invoke_context(typed_command::<T>(id, description), target);
  let mut argv = Vec::with_capacity(arguments.len() + 1);
  argv.push(id.to_string());
  argv.extend(arguments.iter().cloned());
  let matches = match command.try_get_matches_from_mut(argv) {
    Ok(matches) => matches,
    Err(error) if error.kind() == ErrorKind::DisplayHelp => return Ok(InvokeCommandCliParse::Help),
    Err(error) => return Err(error.to_string()),
  };
  let args = T::from_arg_matches(&matches).map_err(|error| error.to_string())?;
  let inputs = encode_args(&args).map_err(|error| format!("failed to encode parsed {id} arguments: {error}"))?;
  Ok(InvokeCommandCliParse::Invoke {
    target: matches.get_one::<String>("auv_target").map(|value| crate::ExecutionTarget::parse(value)).transpose()?,
    inputs,
    typed_args: TypedInvokeArgs::new(args),
    store_root: matches.get_one::<PathBuf>("auv_store_root").cloned(),
    dry_run: matches.get_flag("auv_dry_run"),
    json: matches.get_flag("auv_json"),
    detail: matches.get_flag("auv_detail"),
    wide: matches.get_flag("auv_wide"),
    overlay_enabled: !matches.get_flag("auv_no_overlay"),
  })
}

pub(crate) fn with_invoke_context(command: Command, target: TargetPolicy) -> Command {
  let command = if target == TargetPolicy::Forbidden {
    let about = command.get_long_about().or_else(|| command.get_about()).map(ToString::to_string).unwrap_or_default();
    command.long_about(format!("{about}\n\n{}", target.help()))
  } else {
    command
  };
  command
    .arg(Arg::new("auv_target").long("target").value_name("TARGET").help(target.help()).hide(target == TargetPolicy::Forbidden))
    .arg(Arg::new("auv_dry_run").long("dry-run").action(ArgAction::SetTrue).help("Validate the operation without performing it."))
    .arg(Arg::new("auv_no_overlay").long("no-overlay").action(ArgAction::SetTrue).help("Disable live visual overlay presentation."))
    .arg(
      Arg::new("auv_store_root")
        .long("store-root")
        .value_name("PATH")
        .value_parser(clap::value_parser!(PathBuf))
        .help("Directory used to persist the recorded run and artifacts."),
    )
    .arg(Arg::new("auv_json").long("json").action(ArgAction::SetTrue).help("Render machine-readable JSON output."))
    .arg(Arg::new("auv_detail").long("detail").action(ArgAction::SetTrue).help("Include diagnostic detail in human output."))
    .arg(Arg::new("auv_wide").long("wide").action(ArgAction::SetTrue).help("Include extra columns in human table output."))
}
