//! EWMH window observation, selection, and activation for legacy X11.
//!
//! XTEST has no window-bound delivery: events reach whichever window holds
//! keyboard focus or lies under the pointer. Window-targeted operations
//! therefore resolve an EWMH client window, ask the window manager to activate
//! it, confirm that activation, and then deliver foreground XTEST events. The
//! results report that path as a foreground fallback, never as targeted input.
//!
//! Window references use `x11:0x<XID>` with the client window id from
//! `_NET_CLIENT_LIST_STACKING`, not the window manager's frame window.

use crate::{
  X11DriverSession,
  session::{backend, invalid},
};
use auv_driver_common::{
  Activation, ApplicationActivationMethod, ApplicationActivationResult, ApplicationActivationVerification, ApplicationIdKind, Capture,
  CaptureOptions, ClickOptions, CoordinateSpace, DisturbanceLevel, DriverError, DriverResult, InputActionResult, InputAttempt,
  InputDeliveryPath, InputPolicy, InputTarget, MoveMouseRequest, Point, Position, Rect, ScreenPoint, Scroll, ScrollDeliveryCandidate,
  ScrollOptions, Size, Window, WindowPoint, WindowRef, WindowSelector,
  input::MouseButton,
  selector::{AppSelector, TextMatcher},
};
use std::time::{Duration, Instant};
use x11rb::{
  connection::Connection,
  errors::ReplyError,
  protocol::{
    Event,
    xproto::{AtomEnum, ClientMessageEvent, ConnectionExt as _, CreateWindowAux, EventMask, InputFocus, MapState, PropMode, WindowClass},
  },
  rust_connection::RustConnection,
};

x11rb::atom_manager! {
  pub(crate) Atoms: AtomsCookie {
    _NET_CLIENT_LIST,
    _NET_CLIENT_LIST_STACKING,
    _NET_ACTIVE_WINDOW,
    _NET_WM_NAME,
    UTF8_STRING,
    _NET_FRAME_EXTENTS,
    _NET_WM_PID,
    _NET_WM_STATE,
    _NET_WM_STATE_HIDDEN,
    _NET_WM_WINDOW_TYPE,
    _NET_WM_WINDOW_TYPE_NORMAL,
    _GTK_APPLICATION_ID,
    _AUV_SERVER_TIME,
  }
}

const WINDOW_ID_PREFIX: &str = "x11:0x";

// NOTICE(x11-activation-wait): EWMH activation is a request to the window
// manager, which applies it asynchronously and may refuse it. Two seconds
// bounds a slow but responsive WM; the wait is a polled predicate, not a fixed
// delay. Revisit only with evidence of a WM that needs longer.
const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(2);
const ACTIVATION_POLL: Duration = Duration::from_millis(20);
// NOTICE(x11-focus-fallback): a WM normally moves focus together with
// `_NET_ACTIVE_WINDOW` within a few polls; a quarter second leaves it room
// before the driver focuses the client itself. Revisit with WM evidence.
const FOCUS_FALLBACK_AFTER: Duration = Duration::from_millis(250);
// The server answers our own zero-length property append immediately; one
// second only bounds a stalled connection.
const SERVER_TIME_TIMEOUT: Duration = Duration::from_secs(1);

/// EWMH client-window observation and activation for one X11 session.
///
/// Uses its own x11rb connection, separate from Enigo's XTEST connection.
/// Every confirmation is a server round trip, so XTEST requests issued after
/// a confirmation are processed after the observed focus state.
#[derive(Debug)]
pub(crate) struct Ewmh {
  connection: RustConnection,
  root: u32,
  atoms: Atoms,
  /// Unmapped InputOnly window owned by this connection; its PropertyNotify
  /// events supply real server timestamps for activation requests.
  clock: u32,
}

/// A window resolved for foreground keyboard or wheel delivery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Recipient {
  pub(crate) xid: u32,
  pub(crate) process_id: Option<u32>,
}

impl Recipient {
  pub(crate) fn describe(&self) -> String {
    match self.process_id {
      Some(pid) => format!("{} (pid {pid})", window_id(self.xid)),
      None => format!("{} (pid unknown)", window_id(self.xid)),
    }
  }
}

/// One observed EWMH client with the identity facts the shared `Window` lacks.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ClientWindow {
  pub(crate) xid: u32,
  pub(crate) window: Window,
  wm_class: Option<WmClass>,
  gtk_application_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WmClass {
  instance: String,
  class: String,
}

/// Raw per-window property values, decoded by [`client_window`].
#[derive(Clone, Debug, Default)]
struct WindowFacts {
  net_wm_name: Option<Vec<u8>>,
  wm_name: Option<Vec<u8>>,
  wm_class: Option<Vec<u8>>,
  gtk_application_id: Option<Vec<u8>>,
  process_id: Option<u32>,
  hidden: bool,
  /// `_NET_WM_WINDOW_TYPE` is absent or names `_NET_WM_WINDOW_TYPE_NORMAL`.
  normal_type: bool,
  transient: bool,
  viewable: bool,
  client_origin: (i32, i32),
  client_size: (u32, u32),
  /// `_NET_FRAME_EXTENTS`: left, right, top, bottom decoration widths.
  frame_extents: Option<[u32; 4]>,
}

impl Ewmh {
  pub(crate) fn connect(display: &str) -> DriverResult<Self> {
    let (connection, screen) = x11rb::connect(Some(display)).map_err(backend)?;
    let root = connection.setup().roots.get(screen).ok_or_else(|| backend("X11 screen is missing from the connection setup"))?.root;
    let atoms = Atoms::new(&connection).map_err(backend)?.reply().map_err(backend)?;
    let clock = connection.generate_id().map_err(backend)?;
    connection
      .create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        clock,
        root,
        -1,
        -1,
        1,
        1,
        0,
        WindowClass::INPUT_ONLY,
        x11rb::COPY_FROM_PARENT,
        &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
      )
      .map_err(backend)?
      .check()
      .map_err(backend)?;
    Ok(Self {
      connection,
      root,
      atoms,
      clock,
    })
  }

  fn property(&self, window: u32, property: u32, kind: impl Into<u32>) -> DriverResult<Option<x11rb::protocol::xproto::GetPropertyReply>> {
    let reply = vanished(self.connection.get_property(false, window, property, kind, 0, u32::MAX / 4).map_err(backend)?.reply())?;
    Ok(reply.filter(|reply| reply.format != 0))
  }

  fn active_window(&self) -> DriverResult<Option<u32>> {
    let active = self.property(self.root, self.atoms._NET_ACTIVE_WINDOW, AtomEnum::WINDOW)?;
    Ok(active.and_then(|reply| reply.value32().and_then(|mut values| values.next())).filter(|xid| *xid != x11rb::NONE))
  }

  /// Managed client XIDs, bottom-most first.
  fn client_ids(&self) -> DriverResult<Vec<u32>> {
    // Prefer stacking order; fall back to mapping order, which EWMH also
    // requires, when a WM publishes only `_NET_CLIENT_LIST`.
    let list = match self.property(self.root, self.atoms._NET_CLIENT_LIST_STACKING, AtomEnum::WINDOW)? {
      Some(list) => list,
      None => self
        .property(self.root, self.atoms._NET_CLIENT_LIST, AtomEnum::WINDOW)?
        .ok_or_else(|| DriverError::unsupported("X11 window observation without an EWMH window manager"))?,
    };
    Ok(list.value32().map(|values| values.collect()).unwrap_or_default())
  }

  /// Client windows ordered top-most first.
  fn clients(&self) -> DriverResult<Vec<ClientWindow>> {
    let mut clients = Vec::new();
    for xid in self.client_ids()?.into_iter().rev() {
      if let Some(client) = self.client(xid)? {
        clients.push(client);
      }
    }
    Ok(clients)
  }

  /// Reads one client, or `None` when it was destroyed during observation.
  fn client(&self, xid: u32) -> DriverResult<Option<ClientWindow>> {
    let atoms = &self.atoms;
    let text = |property: u32| -> DriverResult<Option<Vec<u8>>> {
      Ok(self.property(xid, property, AtomEnum::ANY)?.map(|reply| reply.value).filter(|value| !value.is_empty()))
    };
    let atoms_of = |property: u32| -> DriverResult<Option<Vec<u32>>> {
      Ok(self.property(xid, property, AtomEnum::ATOM)?.and_then(|reply| reply.value32().map(|values| values.collect())))
    };
    let cardinals = |property: u32| -> DriverResult<Option<Vec<u32>>> {
      Ok(self.property(xid, property, AtomEnum::CARDINAL)?.and_then(|reply| reply.value32().map(|values| values.collect())))
    };
    let Some(attributes) = vanished(self.connection.get_window_attributes(xid).map_err(backend)?.reply())? else {
      return Ok(None);
    };
    let Some(geometry) = vanished(self.connection.get_geometry(xid).map_err(backend)?.reply())? else {
      return Ok(None);
    };
    let Some(origin) = vanished(self.connection.translate_coordinates(xid, self.root, 0, 0).map_err(backend)?.reply())? else {
      return Ok(None);
    };
    let window_type = atoms_of(atoms._NET_WM_WINDOW_TYPE)?;
    let facts = WindowFacts {
      net_wm_name: text(atoms._NET_WM_NAME)?,
      wm_name: text(AtomEnum::WM_NAME.into())?,
      wm_class: text(AtomEnum::WM_CLASS.into())?,
      gtk_application_id: text(atoms._GTK_APPLICATION_ID)?,
      // NOTICE(x11-wm-pid-host): `_NET_WM_PID` is only meaningful together
      // with WM_CLIENT_MACHINE. Remote X clients are outside this driver's
      // supported environment, so the host is not compared.
      process_id: cardinals(atoms._NET_WM_PID)?.and_then(|values| values.first().copied()).filter(|pid| *pid > 0),
      hidden: atoms_of(atoms._NET_WM_STATE)?.is_some_and(|states| states.contains(&atoms._NET_WM_STATE_HIDDEN)),
      normal_type: window_type.is_none_or(|types| types.is_empty() || types.contains(&atoms._NET_WM_WINDOW_TYPE_NORMAL)),
      transient: self.property(xid, AtomEnum::WM_TRANSIENT_FOR.into(), AtomEnum::WINDOW)?.is_some(),
      viewable: attributes.map_state == MapState::VIEWABLE,
      client_origin: (i32::from(origin.dst_x), i32::from(origin.dst_y)),
      client_size: (u32::from(geometry.width), u32::from(geometry.height)),
      frame_extents: cardinals(atoms._NET_FRAME_EXTENTS)?.and_then(|values| <[u32; 4]>::try_from(values).ok()),
    };
    Ok(Some(client_window(xid, facts)))
  }

  /// A real server timestamp from a zero-length append to our own window.
  fn server_time(&self) -> DriverResult<u32> {
    self
      .connection
      .change_property(PropMode::APPEND, self.clock, self.atoms._AUV_SERVER_TIME, AtomEnum::STRING, 8, 0, &[])
      .map_err(backend)?;
    self.connection.flush().map_err(backend)?;
    let deadline = Instant::now() + SERVER_TIME_TIMEOUT;
    loop {
      match self.connection.poll_for_event().map_err(backend)? {
        Some(Event::PropertyNotify(event)) if event.window == self.clock => return Ok(event.time),
        Some(_) => {}
        None if Instant::now() >= deadline => return Err(backend("X11 server did not report a timestamp for the activation request")),
        None => std::thread::sleep(Duration::from_millis(2)),
      }
    }
  }

  /// Whether keyboard focus is the client window or one of its descendants.
  fn focus_within(&self, xid: u32) -> DriverResult<bool> {
    let mut window = self.connection.get_input_focus().map_err(backend)?.reply().map_err(backend)?.focus;
    // 0 is None and 1 is PointerRoot; neither names a window.
    while window > 1 && window != self.root {
      if window == xid {
        return Ok(true);
      }
      let Some(tree) = vanished(self.connection.query_tree(window).map_err(backend)?.reply())? else {
        return Ok(false);
      };
      window = tree.parent;
    }
    Ok(false)
  }

  fn is_active(&self, xid: u32) -> DriverResult<bool> {
    Ok(self.active_window()? == Some(xid) && self.focus_within(xid)?)
  }

  /// Requests EWMH activation as a pager and waits until the WM reports the
  /// window active and keyboard focus is inside it. No request is repeated.
  fn activate(&self, xid: u32) -> DriverResult<()> {
    if self.is_active(xid)? {
      return Ok(());
    }
    let time = self.server_time()?;
    let current = self.active_window()?.unwrap_or(x11rb::NONE);
    // Source indication 2 is a pager/tool request; WMs apply focus-stealing
    // prevention to application requests (1) but honor explicit user tools.
    let event = ClientMessageEvent::new(32, xid, self.atoms._NET_ACTIVE_WINDOW, [2, time, current, 0, 0]);
    self
      .connection
      .send_event(false, self.root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, event)
      .map_err(backend)?;
    self.connection.flush().map_err(backend)?;
    let started = Instant::now();
    let deadline = started + ACTIVATION_TIMEOUT;
    let mut focus_requested = false;
    loop {
      let active = self.active_window()? == Some(xid);
      if active && self.focus_within(xid)? {
        return Ok(());
      }
      // Some WMs mark the window active but leave keyboard focus elsewhere
      // (a lingering grab, a focus-follows-mouse policy). The window is then
      // mapped, so focus it directly once, as `xdotool windowactivate` and
      // cua-driver's `foreground.rs` do, with the same server timestamp.
      if active && !focus_requested && started.elapsed() >= FOCUS_FALLBACK_AFTER {
        focus_requested = true;
        self.connection.set_input_focus(InputFocus::PARENT, xid, time).map_err(backend)?;
        self.connection.flush().map_err(backend)?;
      }
      if Instant::now() >= deadline {
        if active {
          return Err(backend(format!(
            "X11 window manager activated {} but keyboard focus did not enter it within {}ms, even after a direct focus request; no input was delivered",
            window_id(xid),
            ACTIVATION_TIMEOUT.as_millis()
          )));
        }
        let active = self.active_window()?.map_or_else(|| "none".to_string(), window_id);
        return Err(backend(format!(
          "X11 window manager did not activate {} within {}ms (active window: {active}); no input was delivered",
          window_id(xid),
          ACTIVATION_TIMEOUT.as_millis()
        )));
      }
      std::thread::sleep(ACTIVATION_POLL);
    }
  }
}

/// Window observation and window-relative wheel input over EWMH.
#[derive(Clone, Copy, Debug)]
pub struct WindowApi<'a> {
  pub(crate) session: &'a X11DriverSession,
}

impl WindowApi<'_> {
  /// Lists EWMH client windows, top-most first, including hidden ones.
  ///
  /// `application_id` carries `_GTK_APPLICATION_ID`, else the WM_CLASS class;
  /// `app_name` carries the WM_CLASS class. Frames include WM decorations.
  pub fn list(&self) -> DriverResult<Vec<Window>> {
    Ok(self.session.lock_windows()?.clients()?.into_iter().map(|client| client.window).collect())
  }

  /// Resolves one visible window with the shared selector rules.
  ///
  /// Application id selectors match `_GTK_APPLICATION_ID` and both WM_CLASS
  /// strings case-insensitively, plus the WM_CLASS names a desktop entry of
  /// that id declares; `frontmost` selects the active window's app.
  pub fn resolve(&self, selector: WindowSelector) -> DriverResult<Window> {
    let aliases = match selector.app.as_ref().and_then(|app| app.id.as_ref()) {
      Some(TextMatcher::Exact(id)) => crate::desktop::wm_aliases(id),
      _ => Vec::new(),
    };
    let ewmh = self.session.lock_windows()?;
    let clients = ewmh.clients()?;
    resolve_from(&clients, &selector, ewmh.active_window()?, &aliases).map(|client| client.window.clone())
  }

  /// Captures the window's current frame as a crop of its monitor, after
  /// activating it when `options.activation` asks to.
  ///
  /// The crop is bound to the window: its origin is the window point of the
  /// crop's top-left (0, 0 unless the frame extends past the monitor). Without
  /// activation, overlapping windows appear in it, as with the Wayland crop.
  pub fn capture_with(&self, window: &Window, options: CaptureOptions) -> DriverResult<Capture> {
    if options.display.is_some() || options.region.is_some() || options.window.is_some() {
      return Err(invalid("window.capture_with does not accept display, region, or nested window capture options"));
    }
    let recipient = self.recipient(&InputTarget::Window(window.clone()))?.expect("window target resolves a recipient");
    let frame = {
      let ewmh = self.session.lock_windows()?;
      if let Activation::ActivateFirst { settle } = options.activation {
        ewmh.activate(recipient.xid)?;
        std::thread::sleep(settle);
      }
      ewmh.client(recipient.xid)?.ok_or_else(|| not_found(recipient.xid))?.window.frame
    };
    let mut capture = crate::capture::capture_frame(self.session, frame)?;
    capture.origin = Some(Position::in_window(
      &window.reference,
      WindowPoint::new(capture.bounds.origin.x - frame.origin.x, capture.bounds.origin.y - frame.origin.y),
    ));
    capture.backend = "ewmh.frame+xcap.x11.crop".into();
    capture.fallback_reason = Some("X11 window capture crops the window frame from its monitor; overlapping windows appear in it".into());
    Ok(capture.at_resolution(options.resolution))
  }

  /// [`WindowApi::capture_with`] without activation.
  pub fn capture(&self, window: &Window) -> DriverResult<Capture> {
    self.capture_with(window, CaptureOptions::default())
  }

  /// Maps a point relative to the window frame's top-left onto root pixels.
  pub fn to_screen_point(&self, window: &Window, point: WindowPoint) -> DriverResult<ScreenPoint> {
    let point = point.point();
    Ok(ScreenPoint::new(window.frame.origin.x + point.x, window.frame.origin.y + point.y))
  }

  /// Maps a root pixel into the window frame's coordinate space.
  pub fn to_window_point(&self, window: &Window, point: ScreenPoint) -> DriverResult<WindowPoint> {
    let point = point.point();
    Ok(WindowPoint::new(point.x - window.frame.origin.x, point.y - window.frame.origin.y))
  }

  /// Logical pixels per XTEST wheel detent, for timed scroll quantization.
  pub fn scroll_quantum(&self) -> f64 {
    crate::input::PIXELS_PER_WHEEL_NOTCH
  }

  /// Activates the window, then delivers foreground XTEST wheel events at the
  /// window-relative point mapped through the window's fresh frame.
  ///
  /// X11 has no window-bound wheel route, so the result records a failed
  /// `window_targeted_wheel` attempt before the foreground delivery.
  pub fn scroll(&self, window: &Window, point: WindowPoint, scroll: Scroll, options: ScrollOptions) -> DriverResult<InputActionResult> {
    match options.policy {
      InputPolicy::BackgroundOnly => return Err(invalid("X11 window.scroll cannot use background_only input policy")),
      InputPolicy::BackgroundPreferred if !options.delivery_strategy.candidates.contains(&ScrollDeliveryCandidate::ForegroundHid) => {
        return Err(invalid(
          "X11 window.scroll needs ForegroundHid in the delivery strategy because X11 has no window-bound wheel delivery",
        ));
      }
      _ => {}
    }
    let (recipient, screen) = self.activate_at(window, point, "window.scroll")?;
    let mut result = self.session.input().scroll_at(screen, scroll, options.settle)?;
    result.attempts.insert(
      0,
      InputAttempt::failure(
        InputDeliveryPath::WindowTargetedWheel,
        format!(
          "X11 has no window-bound wheel delivery; activated {} through EWMH _NET_ACTIVE_WINDOW and delivered foreground XTEST wheel events at root ({}, {})",
          recipient.describe(),
          screen.x,
          screen.y
        ),
      ),
    );
    result.focus_disturbance = DisturbanceLevel::Foreground;
    Ok(result)
  }

  /// Activates the window, then clicks with foreground XTEST at the
  /// window-relative point mapped through the window's fresh frame.
  ///
  /// X11 has no window-bound pointer route, so the result records a failed
  /// `window_targeted_mouse` attempt before the foreground delivery.
  // TODO(x11-window-click-strategy): `window_strategy` selects macOS
  // background routes; X11 has none until an MPX/uinput background pointer
  // is approved (cua-driver's MPX path is the reference), so it is ignored.
  pub fn click(&self, window: &Window, point: WindowPoint, options: ClickOptions) -> DriverResult<InputActionResult> {
    if options.policy == InputPolicy::BackgroundOnly {
      return Err(invalid("X11 window.click cannot use background_only input policy"));
    }
    let (recipient, screen) = self.activate_at(window, point, "window.click")?;
    let mut result = self.session.input().click_button_at(screen, options.button, options.click, options.modifiers)?;
    result.attempts.insert(
      0,
      InputAttempt::failure(
        InputDeliveryPath::WindowTargetedMouse,
        format!(
          "X11 has no window-bound pointer delivery; activated {} through EWMH _NET_ACTIVE_WINDOW and delivered a foreground XTEST click at root ({}, {})",
          recipient.describe(),
          screen.x,
          screen.y
        ),
      ),
    );
    result.focus_disturbance = DisturbanceLevel::Foreground;
    Ok(result)
  }

  /// Activates the window, then drags along `movement` with foreground XTEST.
  /// The movement is already in root coordinates, as for the other drivers.
  pub fn drag(
    &self,
    window: &Window,
    mut movement: MoveMouseRequest,
    button: MouseButton,
    policy: InputPolicy,
  ) -> DriverResult<(Point, InputActionResult)> {
    if policy == InputPolicy::BackgroundOnly {
      return Err(invalid("X11 window.drag cannot use background_only input policy"));
    }
    let recipient = self.recipient(&InputTarget::Window(window.clone()))?.expect("window target resolves a recipient");
    self.prepare(&recipient)?;
    movement.target = Some(InputTarget::Foreground);
    let (point, mut result) = self.session.input().drag_mouse(movement, button)?;
    result.attempts.insert(
      0,
      InputAttempt::failure(
        InputDeliveryPath::WindowTargetedMouse,
        format!(
          "X11 has no window-bound pointer delivery; activated {} through EWMH _NET_ACTIVE_WINDOW and delivered a foreground XTEST drag",
          recipient.describe()
        ),
      ),
    );
    result.focus_disturbance = DisturbanceLevel::Foreground;
    Ok((point, result))
  }

  /// Activates the window and maps a frame-relative point onto root pixels
  /// through the frame read after activation, which can restore or move it.
  fn activate_at(&self, window: &Window, point: WindowPoint, operation: &str) -> DriverResult<(Recipient, Point)> {
    let recipient = self.recipient(&InputTarget::Window(window.clone()))?.expect("window target resolves a recipient");
    let frame = {
      let ewmh = self.session.lock_windows()?;
      ewmh.activate(recipient.xid)?;
      ewmh.client(recipient.xid)?.ok_or_else(|| not_found(recipient.xid))?.window.frame
    };
    let local = point.point();
    if !local.x.is_finite()
      || !local.y.is_finite()
      || local.x < 0.0
      || local.y < 0.0
      || local.x > frame.size.width
      || local.y > frame.size.height
    {
      return Err(invalid(format!("X11 {operation} point must be inside the current window frame")));
    }
    Ok((recipient, Point::new(frame.origin.x + local.x, frame.origin.y + local.y)))
  }

  /// Activates an application's window through EWMH `_NET_ACTIVE_WINDOW`.
  ///
  /// The window is the one an application target resolves to (WM_CLASS or
  /// GTK application id, desktop-id aliases, a modal dialog of the same
  /// process first). Activation waits until the WM reports it active and
  /// keyboard focus is inside it; verification then re-reads the active
  /// window after `settle`.
  pub fn activate_application(&self, application_id: &str, settle: Duration) -> DriverResult<ApplicationActivationResult> {
    let recipient = self
      .recipient(&InputTarget::Application {
        id: application_id.to_string(),
      })?
      .expect("application target resolves a recipient");
    self.session.lock_windows()?.activate(recipient.xid)?;
    if !settle.is_zero() {
      std::thread::sleep(settle);
    }
    let ewmh = self.session.lock_windows()?;
    let observed = |xid: u32| -> DriverResult<String> {
      Ok(ewmh.client(xid)?.and_then(|client| client.application_ids().next().map(str::to_string)).unwrap_or_default())
    };
    let verification = match ewmh.active_window()? {
      Some(active) if active == recipient.xid => ApplicationActivationVerification::VerifiedForeground {
        observed_application_id: observed(active)?,
      },
      Some(active) => ApplicationActivationVerification::ForegroundMismatch {
        observed_application_id: observed(active)?,
      },
      None => ApplicationActivationVerification::Unavailable {
        reason: "the window manager reports no active window".to_string(),
      },
    };
    Ok(ApplicationActivationResult {
      requested_application_id: application_id.to_string(),
      application_id_kind: ApplicationIdKind::WmClass,
      method: ApplicationActivationMethod::EwmhActiveWindow,
      activated_window: Some(WindowRef {
        id: window_id(recipient.xid),
      }),
      verification,
    })
  }

  /// Binds an application or window target to one client window without
  /// activating it. Returns `None` for explicit foreground input.
  pub(crate) fn recipient(&self, target: &InputTarget) -> DriverResult<Option<Recipient>> {
    let ewmh = self.session.lock_windows()?;
    let client = match target {
      InputTarget::Foreground => return Ok(None),
      InputTarget::Application { id } => {
        let aliases = crate::desktop::wm_aliases(id);
        application_window(&ewmh.clients()?, id, &aliases, ewmh.active_window()?)?.clone()
      }
      InputTarget::Window(window) => {
        let xid = parse_window_id(&window.reference.id)?;
        // Only WM-managed clients are recipients; an XID can be reused.
        if !ewmh.client_ids()?.contains(&xid) {
          return Err(not_found(xid));
        }
        let client = ewmh.client(xid)?.ok_or_else(|| not_found(xid))?;
        if window.process_id.is_some() && client.window.process_id != window.process_id {
          return Err(invalid("target window owner changed; resolve the target again"));
        }
        client
      }
    };
    Ok(Some(Recipient {
      xid: client.xid,
      process_id: client.window.process_id,
    }))
  }

  /// Application ids of the EWMH active window; empty without an EWMH window
  /// manager or an active window.
  pub(crate) fn active_application_ids(&self) -> Vec<String> {
    let Ok(ewmh) = self.session.lock_windows() else {
      return Vec::new();
    };
    let Ok(Some(xid)) = ewmh.active_window() else {
      return Vec::new();
    };
    ewmh.client(xid).ok().flatten().map(|client| client.application_ids().map(str::to_owned).collect()).unwrap_or_default()
  }

  /// Activates the recipient and confirms it is active with keyboard focus.
  pub(crate) fn prepare(&self, recipient: &Recipient) -> DriverResult<()> {
    self.session.lock_windows()?.activate(recipient.xid)
  }

  /// Confirms the recipient is still active without requesting activation.
  pub(crate) fn confirm(&self, recipient: &Recipient) -> DriverResult<()> {
    let ewmh = self.session.lock_windows()?;
    if ewmh.is_active(recipient.xid)? {
      return Ok(());
    }
    let active = ewmh.active_window()?.map_or_else(|| "none".to_string(), window_id);
    Err(backend(format!(
      "X11 focus moved from {} to {active} during foreground delivery; events after the change may have reached another window and were not retried",
      recipient.describe()
    )))
  }
}

fn vanished<T>(reply: Result<T, ReplyError>) -> DriverResult<Option<T>> {
  match reply {
    Ok(value) => Ok(Some(value)),
    // BadWindow and similar errors mean the client disappeared mid-read.
    Err(ReplyError::X11Error(_)) => Ok(None),
    Err(error) => Err(backend(error)),
  }
}

fn not_found(xid: u32) -> DriverError {
  DriverError::NotFound {
    target: format!("window:{}", window_id(xid)),
  }
}

fn window_id(xid: u32) -> String {
  format!("{WINDOW_ID_PREFIX}{xid:x}")
}

fn parse_window_id(id: &str) -> DriverResult<u32> {
  let hex = id.strip_prefix(WINDOW_ID_PREFIX).ok_or_else(|| invalid(format!("X11 window ids use {WINDOW_ID_PREFIX}<XID>; got {id:?}")))?;
  match u32::from_str_radix(hex, 16) {
    Ok(xid) if xid != 0 && !hex.starts_with('+') => Ok(xid),
    _ => Err(invalid(format!("X11 window id {id:?} does not name a nonzero hexadecimal XID"))),
  }
}

/// WM_CLASS is two NUL-terminated Latin-1 strings: instance, then class.
fn parse_wm_class(value: &[u8]) -> Option<WmClass> {
  let mut parts = value.split(|byte| *byte == 0);
  let instance = latin1(parts.next()?);
  let class = latin1(parts.next().unwrap_or_default());
  (!instance.is_empty() || !class.is_empty()).then_some(WmClass { instance, class })
}

fn latin1(bytes: &[u8]) -> String {
  bytes.iter().map(|byte| char::from(*byte)).collect()
}

/// Prefers UTF-8 `_NET_WM_NAME`; legacy `WM_NAME` is ICCCM Latin-1 STRING.
// TODO(x11-compound-text): WM_NAME may also be COMPOUND_TEXT; it is decoded as
// Latin-1 here because only legacy clients without _NET_WM_NAME use it. Add a
// decoder if a supported X11 workload depends on non-Latin-1 legacy titles.
fn decode_title(net_wm_name: Option<&[u8]>, wm_name: Option<&[u8]>) -> Option<String> {
  net_wm_name
    .and_then(|name| std::str::from_utf8(name).ok().map(str::to_owned))
    .or_else(|| wm_name.map(latin1))
    .map(|title| title.trim_end_matches('\0').to_string())
    .filter(|title| !title.is_empty())
}

/// The outer frame: client origin and size expanded by WM decoration extents.
fn outer_frame(client_origin: (i32, i32), client_size: (u32, u32), extents: Option<[u32; 4]>) -> Rect {
  let [left, right, top, bottom] = extents.unwrap_or_default().map(f64::from);
  Rect {
    origin: Point::new(f64::from(client_origin.0) - left, f64::from(client_origin.1) - top),
    size: Size::new(f64::from(client_size.0) + left + right, f64::from(client_size.1) + top + bottom),
  }
}

fn client_window(xid: u32, facts: WindowFacts) -> ClientWindow {
  let wm_class = facts.wm_class.as_deref().and_then(parse_wm_class);
  let gtk_application_id = facts.gtk_application_id.as_deref().map(latin1_or_utf8).filter(|id| !id.is_empty());
  let class = wm_class.as_ref().map(|class| class.class.clone()).filter(|class| !class.is_empty());
  ClientWindow {
    xid,
    window: Window {
      reference: WindowRef { id: window_id(xid) },
      title: decode_title(facts.net_wm_name.as_deref(), facts.wm_name.as_deref()),
      app_name: class.clone(),
      // NOTICE(x11-app-id): X11 has no single application identity. The
      // application id is the GTK desktop application id when the client sets
      // one, else the WM_CLASS class.
      application_id: gtk_application_id.clone().or(class),
      process_id: facts.process_id,
      frame: outer_frame(facts.client_origin, facts.client_size, facts.frame_extents),
      coordinate_space: CoordinateSpace::Screen,
      is_main: facts.normal_type && !facts.transient,
      is_visible: facts.viewable && !facts.hidden,
    },
    wm_class,
    gtk_application_id,
  }
}

fn latin1_or_utf8(bytes: &[u8]) -> String {
  std::str::from_utf8(bytes).map_or_else(|_| latin1(bytes), str::to_owned).trim_end_matches('\0').to_string()
}

impl ClientWindow {
  /// Application identities in precedence order: GTK id, WM_CLASS class, instance.
  fn application_ids(&self) -> impl Iterator<Item = &str> {
    self
      .gtk_application_id
      .as_deref()
      .into_iter()
      .chain(self.wm_class.iter().flat_map(|class| [class.class.as_str(), class.instance.as_str()]))
      .filter(|id| !id.is_empty())
  }

  fn is_application(&self, matcher: &TextMatcher) -> bool {
    self.application_ids().any(|id| match matcher {
      TextMatcher::Exact(expected) => id.eq_ignore_ascii_case(expected),
      TextMatcher::Contains(needle) => id.to_lowercase().contains(&needle.to_lowercase()),
    })
  }

  /// `is_application`, or one of the WM_CLASS `aliases` a desktop entry of
  /// the requested id declares.
  fn is_application_or_alias(&self, matcher: &TextMatcher, aliases: &[String]) -> bool {
    self.is_application(matcher) || self.application_ids().any(|id| aliases.iter().any(|alias| id.eq_ignore_ascii_case(alias)))
  }

  /// Process identity used to tell application instances apart.
  fn instance_key(&self) -> Result<u32, u32> {
    self.window.process_id.ok_or(self.xid)
  }
}

// REVIEW(window-selector-sharing): the Wayland, Windows, and macOS drivers
// each keep a private copy of these selector rules. X11 differs only in
// case-insensitive application identities; consolidating into
// auv-driver-common is a candidate follow-up slice, not part of this one.
fn resolve_from<'a>(
  clients: &'a [ClientWindow],
  selector: &WindowSelector,
  active: Option<u32>,
  aliases: &[String],
) -> DriverResult<&'a ClientWindow> {
  let frontmost = active.and_then(|xid| clients.iter().find(|client| client.xid == xid)).map(ClientWindow::instance_key);
  let mut matches: Vec<&ClientWindow> = clients
    .iter()
    .filter(|client| client.window.is_visible)
    .filter(|client| selector.app.as_ref().is_none_or(|app| matches_app(client, app, frontmost, aliases)))
    .filter(|client| {
      selector.title.as_ref().is_none_or(|title| client.window.title.as_deref().is_some_and(|value| matches_text(value, title)))
    })
    .collect();
  if selector.main_visible {
    // Stable sort keeps top-most stacking order among equal ranks.
    matches.sort_by_key(|client| {
      std::cmp::Reverse((
        Some(client.xid) == active,
        client.window.is_main,
        client.window.title.as_ref().is_some_and(|title| !title.trim().is_empty()),
        (client.window.frame.size.width * client.window.frame.size.height).round() as i64,
      ))
    });
    return matches.first().copied().ok_or_else(|| DriverError::NotFound {
      target: "main visible window".into(),
    });
  }
  match matches.as_slice() {
    [client] => Ok(client),
    [] => match selector.app.as_ref().and_then(|app| app.id.as_ref()) {
      Some(TextMatcher::Exact(id) | TextMatcher::Contains(id)) => Err(missing_application(&format!("window of application:{id}"), clients)),
      None => Err(DriverError::NotFound {
        target: "window selector".into(),
      }),
    },
    _ => Err(invalid(format!("window selector was ambiguous: {} windows matched", matches.len()))),
  }
}

fn matches_app(client: &ClientWindow, selector: &AppSelector, frontmost: Option<Result<u32, u32>>, aliases: &[String]) -> bool {
  if selector.frontmost && frontmost != Some(client.instance_key()) {
    return false;
  }
  if selector.process_id.is_some_and(|pid| client.window.process_id != Some(pid)) {
    return false;
  }
  if selector.id.as_ref().is_some_and(|matcher| !client.is_application_or_alias(matcher, aliases)) {
    return false;
  }
  selector.name.as_ref().is_none_or(|name| client.window.app_name.as_deref().is_some_and(|value| matches_text(value, name)))
}

fn matches_text(value: &str, matcher: &TextMatcher) -> bool {
  match matcher {
    TextMatcher::Exact(expected) => value == expected,
    TextMatcher::Contains(needle) => value.to_lowercase().contains(&needle.to_lowercase()),
  }
}

/// Selects the keyboard recipient for an application target: the active
/// window when it belongs to the application, else its top-most main window.
/// Windows from several processes require an explicit window target.
fn application_window<'a>(
  clients: &'a [ClientWindow],
  application_id: &str,
  aliases: &[String],
  active: Option<u32>,
) -> DriverResult<&'a ClientWindow> {
  let matcher = TextMatcher::Exact(application_id.to_string());
  let candidates: Vec<&ClientWindow> =
    clients.iter().filter(|client| client.window.is_visible && client.is_application_or_alias(&matcher, aliases)).collect();
  let Some(first) = candidates.first() else {
    return Err(missing_application(&format!("application:{application_id}"), clients));
  };
  if candidates.iter().any(|client| client.instance_key() != first.instance_key()) {
    return Err(invalid(format!(
      "application {application_id:?} matched {} X11 windows across more than one process (or without _NET_WM_PID); select a window target",
      candidates.len()
    )));
  }
  if let Some(client) = candidates.iter().find(|client| Some(client.xid) == active) {
    return Ok(client);
  }
  // The active window of the same process is the application's too, even
  // when its WM_CLASS differs: LibreOffice dialogs carry another class than
  // the Calc frame. Activating the frame behind a modal dialog was refused by
  // the WM ("did not activate … within 2000ms") and dropped OSWorld input.
  if let Some(pid) = first.window.process_id
    && let Some(client) = clients.iter().find(|client| Some(client.xid) == active && client.window.process_id == Some(pid))
  {
    return Ok(client);
  }
  Ok(candidates.iter().find(|client| client.window.is_main).unwrap_or(first))
}

/// A `NotFound` for an application that names the ids visible windows do
/// carry, so a caller that guessed (`com.google.Chrome`) can retry with the
/// X11 one (`google-chrome`) without another `window.list` round trip.
// NOTICE(x11-app-id-hint): `DriverError::NotFound` has only a target, so the
// hint rides in it ("application:x (visible application ids: …) was not
// found"). Move it to a structured recovery field if NotFound gains one.
fn missing_application(target: &str, clients: &[ClientWindow]) -> DriverError {
  let visible = clients.iter().filter(|client| client.window.is_visible).filter_map(|client| client.application_ids().next());
  DriverError::NotFound {
    target: format!("{target} ({})", auv_driver_common::window::visible_applications_hint(visible)),
  }
}

#[cfg(test)]
#[path = "window_test.rs"]
mod tests;
