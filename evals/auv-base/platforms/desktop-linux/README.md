# Linux desktop evaluations

- `cases/window_management.rs`: window mutations (`set_frame`, `set_state`,
  `focus`) are refused as `Unsupported` with the pull-requests-welcome
  recovery, and the window does not move. It needs a desktop session with
  AT-SPI and uses a window already on the desktop
  (`NOTICE` in the case). Run it with `just eval linux-window`.

NOTICE: There are no Linux input or focus evaluations yet. Add `tasks/` and
native `test-objects/` when such a Linux evaluation slice is approved; shared
desktop receivers belong in `../desktop-universal/test-objects/`.
