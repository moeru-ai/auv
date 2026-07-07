use crate::AuvResult;

#[derive(Clone, Debug, Default)]
pub struct Overlay;

impl Overlay {
  pub fn new() -> AuvResult<Self> {
    Ok(Self)
  }

  pub fn show_cursor(&self, x: f64, y: f64, label: &str) -> AuvResult<()> {
    show_cursor(x, y, label)
  }

  pub fn show_dual_cursor(&self, x: f64, y: f64, label: &str, user_label: &str) -> AuvResult<()> {
    show_dual_cursor(x, y, label, user_label)
  }

  pub fn set_cursor(&self, cursor_id: &str, x: f64, y: f64, label: &str, variant: &str) -> AuvResult<()> {
    set_cursor(cursor_id, x, y, label, variant)
  }

  pub fn move_cursor(&self, cursor_id: &str, x: f64, y: f64, label: &str, variant: &str, duration_ms: u64) -> AuvResult<()> {
    move_cursor(cursor_id, x, y, label, variant, duration_ms)
  }

  pub fn move_dual_cursor(&self, x: f64, y: f64, label: &str, user_label: &str, duration_ms: u64) -> AuvResult<()> {
    move_dual_cursor(x, y, label, user_label, duration_ms)
  }

  pub fn flash_cursor(&self, x: f64, y: f64, label: &str, duration_ms: u64) -> AuvResult<()> {
    flash_cursor(x, y, label, duration_ms)
  }

  pub fn flash_cursor_id(&self, cursor_id: &str, x: f64, y: f64, label: &str, duration_ms: u64) -> AuvResult<()> {
    flash_cursor_id(cursor_id, x, y, label, duration_ms)
  }

  pub fn hide_cursor_id(&self, cursor_id: &str) -> AuvResult<()> {
    hide_cursor_id(cursor_id)
  }

  pub fn hide_cursor(&self) -> AuvResult<()> {
    hide_cursor()
  }

  pub fn pump_events(&self, duration_ms: u64) -> AuvResult<()> {
    pump_events(duration_ms)
  }

  pub fn shutdown(&self) -> AuvResult<()> {
    shutdown()
  }
}

pub fn show_cursor(x: f64, y: f64, label: &str) -> AuvResult<()> {
  crate::native::overlay::show_cursor(x, y, label)
}

pub fn show_dual_cursor(x: f64, y: f64, label: &str, user_label: &str) -> AuvResult<()> {
  crate::native::overlay::show_dual_cursor(x, y, label, user_label)
}

pub fn set_cursor(cursor_id: &str, x: f64, y: f64, label: &str, variant: &str) -> AuvResult<()> {
  crate::native::overlay::set_cursor(cursor_id, x, y, label, variant)
}

pub fn move_cursor(cursor_id: &str, x: f64, y: f64, label: &str, variant: &str, duration_ms: u64) -> AuvResult<()> {
  crate::native::overlay::move_cursor(cursor_id, x, y, label, variant, duration_ms)
}

pub fn move_dual_cursor(x: f64, y: f64, label: &str, user_label: &str, duration_ms: u64) -> AuvResult<()> {
  crate::native::overlay::move_dual_cursor(x, y, label, user_label, duration_ms)
}

pub fn flash_cursor(x: f64, y: f64, label: &str, duration_ms: u64) -> AuvResult<()> {
  crate::native::overlay::flash_cursor(x, y, label, duration_ms)
}

pub fn flash_cursor_id(cursor_id: &str, x: f64, y: f64, label: &str, duration_ms: u64) -> AuvResult<()> {
  crate::native::overlay::flash_cursor_id(cursor_id, x, y, label, duration_ms)
}

pub fn hide_cursor_id(cursor_id: &str) -> AuvResult<()> {
  crate::native::overlay::hide_cursor_id(cursor_id)
}

pub fn hide_cursor() -> AuvResult<()> {
  crate::native::overlay::hide_cursor()
}

pub fn pump_events(duration_ms: u64) -> AuvResult<()> {
  crate::native::overlay::pump_events(duration_ms)
}

pub fn shutdown() -> AuvResult<()> {
  crate::native::overlay::shutdown()
}
