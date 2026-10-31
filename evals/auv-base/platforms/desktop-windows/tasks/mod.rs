//! Windows evaluation tasks: the driver acting on windows the user is not looking at.
//!
//! The evaluations run native Win32 receivers (`win32-receiver`) in processes of their own and
//! drive them through the public `WindowsDriverSession`. A [`watch::DesktopWatch`] witnesses the
//! user's desktop independently, so a pass means the foreground window, the real cursor and the
//! global input stream were untouched, not merely that the driver said so. The `apps` survey
//! does the same against real applications of each toolkit.

pub mod apps;
pub mod background;
pub mod foreground_commands;
pub mod probe;
pub mod receiver_process;
pub mod records;
pub mod screen_commands;
pub mod sustained;
pub mod watch;
pub mod window_commands;
pub mod window_keyboard;
