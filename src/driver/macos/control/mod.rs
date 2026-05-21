mod app;
mod ax;
pub(crate) mod common;
mod pointer;
mod screen;
mod text;
mod window;
mod window_ocr;

pub(crate) use self::app::activate_app;
pub(crate) use self::ax::{
  ax_click_window_text, ax_focus_text_input, ax_press_button, focus_text_input, press_button,
  smart_press,
};
pub(crate) use self::pointer::{click_point, scroll_point};
pub(crate) use self::screen::{click_screen_row, click_screen_text};
pub(crate) use self::text::{paste_text_preserve_clipboard, press_key, type_text};
pub(crate) use self::window::click_window_point;
pub(crate) use self::window_ocr::{
  click_window_row, click_window_text, find_window_rows, find_window_text, wait_for_window_rows,
  wait_for_window_text,
};
