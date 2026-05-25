mod error;
mod native;
mod overlay;

pub use error::{AuvResult, NativeOverlayError, native_error_to_auv};
pub use overlay::{
  Overlay, flash_cursor, flash_cursor_id, hide_cursor, hide_cursor_id, move_cursor,
  move_dual_cursor, pump_events, set_cursor, show_cursor, show_dual_cursor, shutdown,
};

#[cfg(test)]
mod tests {
  #[test]
  fn overlay_exports_expected_operations() {
    let _show: fn(f64, f64, &str) -> crate::AuvResult<()> = crate::show_cursor;
    let _show_dual: fn(f64, f64, &str, &str) -> crate::AuvResult<()> = crate::show_dual_cursor;
    let _set: fn(&str, f64, f64, &str, &str) -> crate::AuvResult<()> = crate::set_cursor;
    let _move: fn(&str, f64, f64, &str, &str, u64) -> crate::AuvResult<()> = crate::move_cursor;
    let _move_dual: fn(f64, f64, &str, &str, u64) -> crate::AuvResult<()> = crate::move_dual_cursor;
    let _flash: fn(f64, f64, &str, u64) -> crate::AuvResult<()> = crate::flash_cursor;
    let _flash_id: fn(&str, f64, f64, &str, u64) -> crate::AuvResult<()> = crate::flash_cursor_id;
    let _hide_id: fn(&str) -> crate::AuvResult<()> = crate::hide_cursor_id;
    let _hide: fn() -> crate::AuvResult<()> = crate::hide_cursor;
    let _pump: fn(u64) -> crate::AuvResult<()> = crate::pump_events;
    let _shutdown: fn() -> crate::AuvResult<()> = crate::shutdown;
  }
}
