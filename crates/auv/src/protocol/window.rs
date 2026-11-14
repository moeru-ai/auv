//! Window, window state and window mutation encoding shared by Runner clients
//! and servers.

use auv_api_proto::auv::api::driver::v1 as proto;
use auv_driver::{
  CoordinateSpace, DisturbanceLevel, Rect, Window, WindowDesktop, WindowMutationAttempt, WindowMutationPath, WindowMutationResult,
  WindowRef, WindowState, WindowStateTarget,
};

/// A wire window or mutation result that does not carry complete typed
/// evidence.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
  #[error("Window omitted its reference")]
  MissingReference,
  #[error("Window omitted its screen frame")]
  MissingFrame,
  #[error("WindowMutationResult has an unspecified or unknown {0}")]
  UnknownEnum(&'static str),
}

/// Encodes a driver window without validation.
pub fn encode(window: Window) -> proto::Window {
  proto::Window {
    r#ref: Some(proto::WindowRef {
      window_id: window.reference.id,
    }),
    title: window.title,
    application_name: window.app_name,
    application_id: window.application_id,
    process_id: window.process_id,
    frame: Some(encode_rect(window.frame)),
    is_main: window.is_main,
    is_visible: window.is_visible,
    state: Some(encode_state(window.state)),
  }
}

/// Decodes a wire window. Windows are always in screen space. A missing
/// `state` (an older Runner) decodes as all facts unknown.
pub fn decode(window: proto::Window) -> Result<Window, DecodeError> {
  let reference = window.r#ref.ok_or(DecodeError::MissingReference)?;
  let frame = window.frame.ok_or(DecodeError::MissingFrame)?;
  Ok(Window {
    reference: WindowRef {
      id: reference.window_id,
    },
    title: window.title,
    app_name: window.application_name,
    application_id: window.application_id,
    process_id: window.process_id,
    frame: decode_rect(frame),
    coordinate_space: CoordinateSpace::Screen,
    is_main: window.is_main,
    is_visible: window.is_visible,
    state: window.state.map(decode_state).unwrap_or_default(),
  })
}

pub fn encode_state(state: WindowState) -> proto::WindowState {
  proto::WindowState {
    is_minimized: state.is_minimized,
    is_maximized: state.is_maximized,
    is_fullscreen: state.is_fullscreen,
    is_visible: state.is_visible,
    is_focused: state.is_focused,
    is_always_on_top: state.is_always_on_top,
    stack_index: state.stack_index,
    desktop: state.desktop.map(|desktop| proto::WindowDesktop {
      id: desktop.id,
      is_current: desktop.is_current,
    }),
  }
}

pub fn decode_state(state: proto::WindowState) -> WindowState {
  WindowState {
    is_minimized: state.is_minimized,
    is_maximized: state.is_maximized,
    is_fullscreen: state.is_fullscreen,
    is_visible: state.is_visible,
    is_focused: state.is_focused,
    is_always_on_top: state.is_always_on_top,
    stack_index: state.stack_index,
    desktop: state.desktop.map(|desktop| WindowDesktop {
      id: desktop.id,
      is_current: desktop.is_current,
    }),
  }
}

pub fn encode_state_target(state: WindowStateTarget) -> proto::WindowStateTarget {
  match state {
    WindowStateTarget::Normal => proto::WindowStateTarget::Normal,
    WindowStateTarget::Minimized => proto::WindowStateTarget::Minimized,
    WindowStateTarget::Maximized => proto::WindowStateTarget::Maximized,
  }
}

/// Decodes a requested target state; `None` for unspecified or unknown values.
pub fn decode_state_target(value: i32) -> Option<WindowStateTarget> {
  match proto::WindowStateTarget::try_from(value).ok()? {
    proto::WindowStateTarget::Unspecified => None,
    proto::WindowStateTarget::Normal => Some(WindowStateTarget::Normal),
    proto::WindowStateTarget::Minimized => Some(WindowStateTarget::Minimized),
    proto::WindowStateTarget::Maximized => Some(WindowStateTarget::Maximized),
  }
}

/// Encodes a driver mutation result without validation.
pub fn encode_mutation_result(result: WindowMutationResult) -> proto::WindowMutationResult {
  proto::WindowMutationResult {
    selected_path: encode_path(result.selected_path) as i32,
    attempts: result
      .attempts
      .into_iter()
      .map(|attempt| proto::WindowMutationAttempt {
        path: encode_path(attempt.path) as i32,
        succeeded: attempt.succeeded,
        message: attempt.message,
      })
      .collect(),
    before_frame: result.before_frame.map(encode_rect),
    after_frame: result.after_frame.map(encode_rect),
    before_state: result.before_state.map(encode_state),
    after_state: result.after_state.map(encode_state),
    adjusted_by_app: result.adjusted_by_app,
    focus_disturbance: encode_disturbance(result.focus_disturbance) as i32,
    mouse_disturbance: encode_disturbance(result.mouse_disturbance) as i32,
  }
}

/// Decodes a wire mutation result; unspecified paths or disturbance levels
/// are incomplete evidence.
pub fn decode_mutation_result(result: proto::WindowMutationResult) -> Result<WindowMutationResult, DecodeError> {
  Ok(WindowMutationResult {
    selected_path: decode_path(result.selected_path)?,
    attempts: result
      .attempts
      .into_iter()
      .map(|attempt| {
        Ok(WindowMutationAttempt {
          path: decode_path(attempt.path)?,
          succeeded: attempt.succeeded,
          message: attempt.message,
        })
      })
      .collect::<Result<_, DecodeError>>()?,
    before_frame: result.before_frame.map(decode_rect),
    after_frame: result.after_frame.map(decode_rect),
    before_state: result.before_state.map(decode_state),
    after_state: result.after_state.map(decode_state),
    adjusted_by_app: result.adjusted_by_app,
    focus_disturbance: decode_disturbance(result.focus_disturbance)?,
    mouse_disturbance: decode_disturbance(result.mouse_disturbance)?,
  })
}

fn encode_rect(rect: Rect) -> proto::ScreenRect {
  proto::ScreenRect {
    x: rect.origin.x,
    y: rect.origin.y,
    width: rect.size.width,
    height: rect.size.height,
  }
}

fn decode_rect(rect: proto::ScreenRect) -> Rect {
  Rect::new(rect.x, rect.y, rect.width, rect.height)
}

fn encode_path(path: WindowMutationPath) -> proto::WindowMutationPath {
  match path {
    WindowMutationPath::AxWindowAttribute => proto::WindowMutationPath::AxWindowAttribute,
    WindowMutationPath::AxWindowAction => proto::WindowMutationPath::AxWindowAction,
    WindowMutationPath::PlatformNative => proto::WindowMutationPath::PlatformNative,
    WindowMutationPath::ForegroundSystemEvents => proto::WindowMutationPath::ForegroundSystemEvents,
    WindowMutationPath::Unsupported => proto::WindowMutationPath::Unsupported,
  }
}

fn decode_path(value: i32) -> Result<WindowMutationPath, DecodeError> {
  Ok(match proto::WindowMutationPath::try_from(value).map_err(|_| DecodeError::UnknownEnum("WindowMutationPath"))? {
    proto::WindowMutationPath::Unspecified => return Err(DecodeError::UnknownEnum("WindowMutationPath")),
    proto::WindowMutationPath::AxWindowAttribute => WindowMutationPath::AxWindowAttribute,
    proto::WindowMutationPath::AxWindowAction => WindowMutationPath::AxWindowAction,
    proto::WindowMutationPath::PlatformNative => WindowMutationPath::PlatformNative,
    proto::WindowMutationPath::ForegroundSystemEvents => WindowMutationPath::ForegroundSystemEvents,
    proto::WindowMutationPath::Unsupported => WindowMutationPath::Unsupported,
  })
}

fn encode_disturbance(level: DisturbanceLevel) -> proto::DisturbanceLevel {
  match level {
    DisturbanceLevel::None => proto::DisturbanceLevel::None,
    DisturbanceLevel::Temporary => proto::DisturbanceLevel::Temporary,
    DisturbanceLevel::Foreground => proto::DisturbanceLevel::Foreground,
    DisturbanceLevel::Unknown => proto::DisturbanceLevel::Unknown,
  }
}

fn decode_disturbance(value: i32) -> Result<DisturbanceLevel, DecodeError> {
  Ok(match proto::DisturbanceLevel::try_from(value).map_err(|_| DecodeError::UnknownEnum("DisturbanceLevel"))? {
    proto::DisturbanceLevel::Unspecified => return Err(DecodeError::UnknownEnum("DisturbanceLevel")),
    proto::DisturbanceLevel::None => DisturbanceLevel::None,
    proto::DisturbanceLevel::Temporary => DisturbanceLevel::Temporary,
    proto::DisturbanceLevel::Foreground => DisturbanceLevel::Foreground,
    proto::DisturbanceLevel::Unknown => DisturbanceLevel::Unknown,
  })
}

#[cfg(test)]
#[path = "window_test.rs"]
mod tests;
