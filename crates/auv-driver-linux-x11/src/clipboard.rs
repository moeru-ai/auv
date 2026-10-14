//! X11 CLIPBOARD selection ownership for text paste and clipboard access.
//!
//! X11 has no clipboard storage: the clipboard is whichever client owns the
//! `CLIPBOARD` selection, and every paste is a conversion request sent to that
//! owner. To paste, this module owns the selection on its own connection and
//! answers requests from an event thread until the session drops.
//!
//! The thread is the only reader of events on the connection. Callers send
//! requests on the same connection and wait on [`Shared`] for the events that
//! the thread records for them (conversion replies, timestamps, markers).

use crate::{
  X11DriverSession,
  session::{backend, invalid},
};
use auv_driver_common::DriverResult;
use std::{
  sync::{Arc, Condvar, Mutex, MutexGuard},
  thread::JoinHandle,
  time::{Duration, Instant},
};
use x11rb::{
  CURRENT_TIME, NONE,
  connection::{Connection, RequestConnection as _},
  protocol::{
    Event,
    xproto::{
      self, Atom, AtomEnum, ClientMessageEvent, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, SelectionNotifyEvent,
      SelectionRequestEvent, Timestamp, Window, WindowClass,
    },
  },
  rust_connection::RustConnection,
  wrapper::ConnectionExt as _,
};

x11rb::atom_manager! {
  pub(crate) Atoms: AtomsCookie {
    CLIPBOARD,
    CLIPBOARD_MANAGER,
    TARGETS,
    TIMESTAMP,
    MULTIPLE,
    UTF8_STRING,
    TEXT,
    TEXT_PLAIN_UTF8: b"text/plain;charset=utf-8",
    INCR,
    AUV_SELECTION: b"_AUV_SELECTION",
    AUV_TIMESTAMP: b"_AUV_TIMESTAMP",
    AUV_MARKER: b"_AUV_CLIPBOARD_MARKER",
    _NET_SUPPORTING_WM_CHECK,
  }
}

// NOTICE: ICCCM gives no deadline for a selection owner. One second matches
// common clipboard tools (xclip, xsel) and keeps a hung owner from stalling
// input delivery; raise it only with evidence of a slow but healthy owner.
const OWNER_TIMEOUT: Duration = Duration::from_secs(1);
// NOTICE: ChangeProperty adds a 24-byte header to the payload. Keep a margin
// so a text near the server request limit still fits in one request.
const REQUEST_HEADROOM: usize = 256;

// NOTICE(x11-paste-drain): after the consumer's first text read, a paste
// keeps CLIPBOARD until the consumer has made no request for 200 ms, at most
// two seconds. Chrome 130 in OSWorld finished its read rounds within 36 ms;
// raise these only with evidence of a consumer that reads later.
const PASTE_QUIET: Duration = Duration::from_millis(200);
const PASTE_DRAIN_LIMIT: Duration = Duration::from_secs(2);

const MARKER_BARRIER: u32 = 0;
const MARKER_ARM: u32 = 1;
const MARKER_SHUTDOWN: u32 = 2;

/// The focused X client a paste is meant for, captured before the shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PasteRecipient {
  /// Resource-id base of the client that created the focus window.
  client: u32,
  process_id: Option<u32>,
}

/// Text the session currently serves as the CLIPBOARD owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Served {
  pub(crate) text: String,
  pub(crate) timestamp: Timestamp,
}

/// Clipboard content captured before a paste, used to restore it after.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Snapshot {
  /// No client owned CLIPBOARD.
  Empty,
  /// The owner offered text. `rich` is true when it also offered non-text
  /// targets that a text restore cannot reproduce.
  Owned { text: Option<String>, rich: bool },
}

/// How the clipboard ended after a paste transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Restore {
  /// The previous text, or the previous empty clipboard, is back.
  Exact,
  /// The previous owner offered non-text targets; only text (or nothing)
  /// was restored.
  // TODO(x11-clipboard-rich-restore): restore every snapshot target instead
  // of text only. Deferred until a consumer needs rich content to survive a
  // paste; it needs per-target storage and INCR reads.
  TextOnly,
  /// Another client took CLIPBOARD during the transaction; its content was
  /// kept and the snapshot was not restored.
  Skipped,
}

/// The answer to one SelectionRequest, decided without I/O.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Answer {
  Atoms(Vec<Atom>),
  Timestamp(Timestamp),
  Text { type_: Atom, bytes: Vec<u8> },
  Refuse,
}

/// Decides the reply to a request for `target`. Only text targets count as a
/// paste consumer; `TARGETS` and `TIMESTAMP` are metadata.
pub(crate) fn answer(atoms: &Atoms, served: Option<&Served>, target: Atom, time: Timestamp, limit: usize) -> Answer {
  let Some(served) = served else {
    return Answer::Refuse;
  };
  // ICCCM 2.2: refuse requests timestamped before we acquired ownership.
  if time != CURRENT_TIME && time < served.timestamp {
    return Answer::Refuse;
  }
  let latin1 = latin1(&served.text);
  if target == atoms.TARGETS {
    let mut targets = vec![
      atoms.TARGETS,
      atoms.TIMESTAMP,
      atoms.UTF8_STRING,
      atoms.TEXT,
      atoms.TEXT_PLAIN_UTF8,
    ];
    if latin1.is_some() {
      targets.push(AtomEnum::STRING.into());
    }
    return Answer::Atoms(targets);
  }
  if target == atoms.TIMESTAMP {
    return Answer::Timestamp(served.timestamp);
  }
  let (type_, bytes) = if target == atoms.UTF8_STRING || target == atoms.TEXT {
    (atoms.UTF8_STRING, served.text.as_bytes().to_vec())
  } else if target == atoms.TEXT_PLAIN_UTF8 {
    (atoms.TEXT_PLAIN_UTF8, served.text.as_bytes().to_vec())
  } else if target == Atom::from(AtomEnum::STRING) {
    match latin1 {
      Some(bytes) => (AtomEnum::STRING.into(), bytes),
      None => return Answer::Refuse,
    }
  } else {
    // TODO(x11-clipboard-multiple): MULTIPLE is refused. Deferred because
    // text consumers request a single target; add it when a consumer that
    // requires MULTIPLE is observed.
    return Answer::Refuse;
  };
  // TODO(x11-clipboard-incr): payloads above one request need the INCR
  // protocol. Paste rejects such text before input; this guards restores.
  if bytes.len() > limit {
    return Answer::Refuse;
  }
  Answer::Text { type_, bytes }
}

/// Encodes text as ICCCM `STRING` (ISO Latin-1) when every character fits.
fn latin1(text: &str) -> Option<Vec<u8>> {
  text.chars().map(|ch| u8::try_from(u32::from(ch)).ok()).collect()
}

/// Rejects a paste text that cannot be served in one X request.
pub(crate) fn check_size(text: &str, limit: usize) -> DriverResult<()> {
  if text.len() > limit {
    // TODO(x11-clipboard-incr): see `answer`; unlocked when a caller needs
    // pastes larger than the server's maximum request size.
    return Err(invalid(format!(
      "X11 clipboard paste is limited to {limit} bytes of UTF-8 text on this server; use input.typeText for larger text"
    )));
  }
  Ok(())
}

/// Classifies a target name advertised by another owner as text or metadata.
fn is_text_or_meta(name: &str) -> bool {
  matches!(
    name,
    "TARGETS"
      | "TIMESTAMP"
      | "MULTIPLE"
      | "SAVE_TARGETS"
      | "DELETE"
      | "INSERT_SELECTION"
      | "INSERT_PROPERTY"
      | "UTF8_STRING"
      | "STRING"
      | "TEXT"
      | "COMPOUND_TEXT"
      | "C_STRING"
  ) || name.to_ascii_lowercase().starts_with("text/plain")
}

#[derive(Default)]
struct State {
  served: Option<Served>,
  /// SelectionClear arrived while serving.
  lost: bool,
  /// An arm marker was processed; requests after it belong to the paste.
  armed: bool,
  /// Requestor of every request since the arm marker, and whether it
  /// received text (`TARGETS` and `TIMESTAMP` do not).
  requests: Vec<(Window, bool)>,
  notifications: Vec<SelectionNotifyEvent>,
  timestamps: Vec<Timestamp>,
  marker: u32,
  failure: Option<String>,
}

struct Shared {
  state: Mutex<State>,
  changed: Condvar,
}

impl Shared {
  fn lock(&self) -> DriverResult<MutexGuard<'_, State>> {
    self.state.lock().map_err(|_| backend("X11 clipboard state mutex poisoned; open a new session"))
  }
}

/// One CLIPBOARD owner per session: a connection, an unmapped InputOnly
/// window, and the event thread that answers SelectionRequest events.
pub(crate) struct Owner {
  connection: Arc<RustConnection>,
  root: Window,
  window: Window,
  atoms: Atoms,
  limit: usize,
  resource_mask: u32,
  shared: Arc<Shared>,
  next_marker: Mutex<u32>,
  thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Owner {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("Owner").field("window", &self.window).finish_non_exhaustive()
  }
}

impl Owner {
  fn open(display: &str) -> DriverResult<Self> {
    let (connection, screen) = RustConnection::connect(Some(display)).map_err(backend)?;
    let root = connection.setup().roots.get(screen).ok_or_else(|| backend("X11 screen missing"))?.root;
    let resource_mask = connection.setup().resource_id_mask;
    let window = connection.generate_id().map_err(backend)?;
    connection
      .create_window(
        0,
        window,
        root,
        0,
        0,
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
    let atoms = Atoms::new(&connection).map_err(backend)?.reply().map_err(backend)?;
    let limit = connection.maximum_request_bytes().saturating_sub(REQUEST_HEADROOM);
    let connection = Arc::new(connection);
    let shared = Arc::new(Shared {
      state: Mutex::new(State::default()),
      changed: Condvar::new(),
    });
    let thread = std::thread::Builder::new()
      .name("auv-x11-clipboard".into())
      .spawn({
        let connection = connection.clone();
        let shared = shared.clone();
        move || serve(&connection, window, atoms, limit, &shared)
      })
      .map_err(backend)?;
    Ok(Self {
      connection,
      root,
      window,
      atoms,
      limit,
      resource_mask,
      shared,
      next_marker: Mutex::new(0),
      thread: Some(thread),
    })
  }

  /// Largest UTF-8 paste text this server accepts in one property write.
  pub(crate) fn limit(&self) -> usize {
    self.limit
  }

  /// Reads the current CLIPBOARD text. Fails if the owner does not answer
  /// instead of treating a hung owner as an empty clipboard.
  pub(crate) fn snapshot(&self) -> DriverResult<Snapshot> {
    let owner = self.selection_owner()?;
    if owner == NONE {
      return Ok(Snapshot::Empty);
    }
    if owner == self.window {
      let state = self.shared.lock()?;
      return Ok(match &state.served {
        Some(served) => Snapshot::Owned {
          text: Some(served.text.clone()),
          rich: false,
        },
        None => Snapshot::Empty,
      });
    }
    let targets = self.convert(self.atoms.TARGETS)?.map(|(_, value)| atoms_of(&value));
    let rich = match &targets {
      Some(targets) => self.has_rich_target(targets)?,
      None => false,
    };
    let offers = |target: Atom| targets.as_ref().is_none_or(|targets| targets.contains(&target));
    let mut text = None;
    if offers(self.atoms.UTF8_STRING)
      && let Some((_, bytes)) = self.convert(self.atoms.UTF8_STRING)?
    {
      text = Some(String::from_utf8(bytes).map_err(|_| backend("CLIPBOARD owner returned invalid UTF8_STRING"))?);
    }
    if text.is_none()
      && offers(AtomEnum::STRING.into())
      && let Some((_, bytes)) = self.convert(AtomEnum::STRING.into())?
    {
      text = Some(bytes.into_iter().map(char::from).collect());
    }
    // An owner that answered nothing readable may hold content we cannot
    // describe, so a text restore is not exact.
    let rich = rich || text.is_none();
    Ok(Snapshot::Owned { text, rich })
  }

  /// Takes CLIPBOARD ownership with a server timestamp and serves `text`.
  pub(crate) fn own(&self, text: &str) -> DriverResult<()> {
    let timestamp = self.server_time()?;
    {
      let mut state = self.shared.lock()?;
      state.served = Some(Served {
        text: text.to_owned(),
        timestamp,
      });
      state.lost = false;
    }
    self.connection.set_selection_owner(self.window, self.atoms.CLIPBOARD, timestamp).map_err(backend)?;
    if self.selection_owner()? != self.window {
      self.shared.lock()?.served = None;
      return Err(backend("X11 server did not grant CLIPBOARD ownership"));
    }
    Ok(())
  }

  /// Starts counting data requests as paste consumers. The marker travels
  /// through the server, so requests queued before it are not counted.
  pub(crate) fn arm(&self) -> DriverResult<()> {
    self.marker(MARKER_ARM)
  }

  /// The X client and process that receive keyboard input, which a paste
  /// must reach: the focus window, or with PointerRoot focus the deepest
  /// window under the pointer. `None` only when PointerRoot focus has no
  /// window under the pointer; a None focus, where keys reach no window, is
  /// an error.
  pub(crate) fn focused_recipient(&self) -> DriverResult<Option<PasteRecipient>> {
    const FOCUS_NONE: Window = 0;
    const FOCUS_POINTER_ROOT: Window = 1;
    let mut focus = self.connection.get_input_focus().map_err(backend)?.reply().map_err(backend)?.focus;
    if focus == FOCUS_NONE {
      return Err(backend("no X11 window has keyboard focus, so a paste shortcut would reach no application; focus the target first"));
    }
    if focus == FOCUS_POINTER_ROOT {
      // Under a reparenting WM the root's child is a frame owned by the WM;
      // descend to the application window that actually receives keys.
      let mut window = self.root;
      loop {
        let child = self.connection.query_pointer(window).map_err(backend)?.reply().map_err(backend)?.child;
        if child == NONE {
          break;
        }
        window = child;
      }
      if window == self.root {
        return Ok(None);
      }
      focus = window;
    }
    let client = focus & !self.resource_mask;
    // A WM parks focus on a window of its own when no application is active
    // (mutter's no-focus window). The WM also reads new CLIPBOARD owners, so
    // treating it as the recipient would report a paste that reached nobody.
    if self.window_manager_window()?.is_some_and(|check| check & !self.resource_mask == client) {
      return Err(backend(
        "keyboard focus is on the window manager, not an application window, so nothing would be pasted; activate the target window first",
      ));
    }
    Ok(Some(PasteRecipient {
      client,
      process_id: self.client_pid(focus),
    }))
  }

  /// The EWMH `_NET_SUPPORTING_WM_CHECK` window, which the running window
  /// manager creates on its own connection. `None` without an EWMH WM.
  fn window_manager_window(&self) -> DriverResult<Option<Window>> {
    let reply = self
      .connection
      .get_property(false, self.root, self.atoms._NET_SUPPORTING_WM_CHECK, AtomEnum::WINDOW, 0, 1)
      .map_err(backend)?
      .reply()
      .map_err(backend)?;
    Ok(reply.value32().and_then(|mut values| values.next()).filter(|window| *window != NONE))
  }

  /// The local process id of the client that created `window`, through the
  /// X-Resource extension. `None` when the server lacks XRes, the window is
  /// gone, or the client is remote.
  fn client_pid(&self, window: Window) -> Option<u32> {
    use x11rb::protocol::res::{ClientIdMask, ClientIdSpec, ConnectionExt as _};
    self.connection.extension_information(x11rb::protocol::res::X11_EXTENSION_NAME).ok()??;
    let spec = ClientIdSpec {
      client: window,
      mask: ClientIdMask::LOCAL_CLIENT_PID,
    };
    let reply = self.connection.res_query_client_ids(&[spec]).ok()?.reply().ok()?;
    reply.ids.into_iter().find(|id| id.spec.mask.contains(ClientIdMask::LOCAL_CLIENT_PID)).and_then(|id| id.value.first().copied())
  }

  /// Waits until the focused application requests text after [`Owner::arm`].
  ///
  /// Only the focused window's X client, or another client of the same
  /// process, counts as the consumer: GNOME's mutter reads every new
  /// CLIPBOARD owner to mirror it, and counting that read made a paste into
  /// a window that ignores Ctrl+V look delivered. Without a focus window,
  /// any client other than the clipboard manager counts.
  ///
  /// The consumer must start within `start` (any request, such as
  /// `TARGETS`); once it has, it gets `transfer` from the shortcut to request
  /// the text. A cold Chrome in a loaded VM asked for `TARGETS` several times
  /// and missed a flat one-second window.
  ///
  /// Returns which process read the text and after how long, plus any other
  /// readers, as evidence for the result: delivery to the focused process
  /// still does not prove the text landed in the intended control.
  pub(crate) fn wait_for_consumer(&self, start: Duration, transfer: Duration, recipient: Option<PasteRecipient>) -> DriverResult<String> {
    let manager = self.connection.get_selection_owner(self.atoms.CLIPBOARD_MANAGER).map_err(backend)?.reply().map_err(backend)?.owner;
    let client = |window: Window| window & !self.resource_mask;
    let is_consumer = |window: Window| match recipient {
      Some(recipient) => client(window) == recipient.client || recipient.process_id.is_some_and(|pid| self.client_pid(window) == Some(pid)),
      None => manager == NONE || client(window) != client(manager),
    };
    let started = Instant::now();
    let mut deadline = started + start;
    let mut seen: Vec<(Window, bool)> = Vec::new();
    let mut consumer_started = false;
    let mut found = false;
    let mut read_after = Duration::ZERO;
    while !found {
      let remaining = deadline.saturating_duration_since(Instant::now());
      let fresh = self.wait(remaining, |state| (state.requests.len() > seen.len()).then(|| state.requests[seen.len()..].to_vec()))?;
      let Some(fresh) = fresh else {
        break;
      };
      // XRes lookups run outside the state lock; the event thread keeps
      // answering requests meanwhile.
      for &(window, text) in &fresh {
        if is_consumer(window) {
          if text && !found {
            found = true;
            read_after = started.elapsed();
          }
          if !consumer_started {
            consumer_started = true;
            deadline = started + transfer;
          }
        }
      }
      seen.extend(fresh);
    }
    // Keep serving until the consumer goes quiet: Chrome 130 reads CLIPBOARD
    // in several rounds per Ctrl+V (TARGETS x4 + UTF8_STRING, for ~36 ms) and
    // inserts a later round. Restoring after the first read pasted the old
    // clipboard instead, while the result claimed delivery.
    if found {
      let cap = Instant::now() + PASTE_DRAIN_LIMIT;
      let mut quiet_until = Instant::now() + PASTE_QUIET;
      loop {
        let remaining = quiet_until.min(cap).saturating_duration_since(Instant::now());
        let fresh = self.wait(remaining, |state| (state.requests.len() > seen.len()).then(|| state.requests[seen.len()..].to_vec()))?;
        let Some(fresh) = fresh else {
          break;
        };
        if fresh.iter().any(|&(window, _)| is_consumer(window)) {
          quiet_until = Instant::now() + PASTE_QUIET;
        }
        seen.extend(fresh);
      }
    }
    {
      let mut state = self.shared.lock()?;
      state.armed = false;
      state.requests.clear();
    }
    // Name the other clients that read CLIPBOARD, so a log shows whether the
    // focused application was slow or a mirror (mutter) was the only reader.
    let pid_of =
      |window: Window| self.client_pid(window).map_or_else(|| format!("client {:#x}", client(window)), |pid| format!("pid {pid}"));
    let mut readers: Vec<String> =
      seen.iter().filter(|&&(window, text)| text && !is_consumer(window)).map(|&(window, _)| pid_of(window)).collect();
    readers.dedup();
    let focused = recipient.and_then(|recipient| recipient.process_id).map_or_else(String::new, |pid| format!(" (pid {pid})"));
    if !found {
      if consumer_started {
        return Err(backend(format!(
          "the focused window started a paste (it asked which formats CLIPBOARD offers) but did not request the text within {} ms; nothing was pasted (use input.typeText)",
          transfer.as_millis()
        )));
      }
      let others = if readers.is_empty() {
        String::new()
      } else {
        format!("; only other clients read it ({})", readers.join(", "))
      };
      return Err(backend(format!(
        "the focused window{focused} did not request CLIPBOARD within {} ms after the paste shortcut{others}; it may not accept paste here, so nothing was pasted (use input.typeText)",
        start.as_millis()
      )));
    }
    // Round trip so the SelectionNotify we sent is processed before any
    // later XTEST event from the input connection.
    self.connection.get_input_focus().map_err(backend)?.reply().map_err(backend)?;
    let others = if readers.is_empty() {
      String::new()
    } else {
      format!("; also read by {}", readers.join(", "))
    };
    Ok(format!("the focused application{focused} read the pasted text after {} ms{others}", read_after.as_millis()))
  }

  /// Puts `snapshot` back unless another client took CLIPBOARD meanwhile.
  ///
  // NOTICE: X11 cannot hand ownership back to the previous owner. A restored
  // text stays served by this session and disappears when the session drops.
  // TODO(x11-clipboard-manager-handoff): hand the restored text to a
  // CLIPBOARD_MANAGER through SAVE_TARGETS before dropping. Deferred until a
  // workflow needs clipboard content to outlive the AUV session.
  pub(crate) fn restore(&self, snapshot: &Snapshot) -> DriverResult<Restore> {
    {
      let state = self.shared.lock()?;
      if state.lost || state.served.is_none() {
        return Ok(Restore::Skipped);
      }
    }
    if self.selection_owner()? != self.window {
      return Ok(Restore::Skipped);
    }
    match snapshot {
      Snapshot::Owned {
        text: Some(text),
        rich,
      } => {
        let mut state = self.shared.lock()?;
        let served = state.served.as_mut().expect("checked served above");
        served.text.clone_from(text);
        Ok(if *rich {
          Restore::TextOnly
        } else {
          Restore::Exact
        })
      }
      Snapshot::Empty => {
        self.release()?;
        Ok(Restore::Exact)
      }
      Snapshot::Owned { text: None, .. } => {
        self.release()?;
        Ok(Restore::TextOnly)
      }
    }
  }

  /// Gives up ownership, leaving CLIPBOARD empty.
  fn release(&self) -> DriverResult<()> {
    let timestamp = self.server_time()?;
    self.shared.lock()?.served = None;
    // NOTICE: SetSelectionOwner(None) clears the selection whoever owns it.
    // `restore` checked ownership just before; a client taking CLIPBOARD in
    // this short gap is cleared too. X11 offers no conditional release.
    self.connection.set_selection_owner(NONE, self.atoms.CLIPBOARD, timestamp).map_err(backend)?;
    // Drain the SelectionClear our own release produces before the next
    // transaction starts, so it is not mistaken for a lost ownership.
    self.marker(MARKER_BARRIER)?;
    self.shared.lock()?.lost = false;
    Ok(())
  }

  fn selection_owner(&self) -> DriverResult<Window> {
    Ok(self.connection.get_selection_owner(self.atoms.CLIPBOARD).map_err(backend)?.reply().map_err(backend)?.owner)
  }

  /// Asks the current owner to convert CLIPBOARD to `target`. Returns
  /// `None` when the owner refuses the target.
  fn convert(&self, target: Atom) -> DriverResult<Option<(Atom, Vec<u8>)>> {
    self.shared.lock()?.notifications.clear();
    self.connection.delete_property(self.window, self.atoms.AUV_SELECTION).map_err(backend)?;
    self.connection.convert_selection(self.window, self.atoms.CLIPBOARD, target, self.atoms.AUV_SELECTION, CURRENT_TIME).map_err(backend)?;
    self.connection.flush().map_err(backend)?;
    let selection = self.atoms.CLIPBOARD;
    let notification = self.wait(OWNER_TIMEOUT, |state| {
      let index = state.notifications.iter().position(|event| event.selection == selection && event.target == target)?;
      Some(state.notifications.remove(index))
    })?;
    let Some(notification) = notification else {
      return Err(backend(format!(
        "CLIPBOARD owner did not answer a conversion request within {} ms; clipboard left unchanged",
        OWNER_TIMEOUT.as_millis()
      )));
    };
    if notification.property == NONE {
      return Ok(None);
    }
    let reply = self
      .connection
      .get_property(true, self.window, notification.property, AtomEnum::ANY, 0, u32::MAX / 4)
      .map_err(backend)?
      .reply()
      .map_err(backend)?;
    if reply.type_ == self.atoms.INCR {
      // TODO(x11-clipboard-incr): read INCR transfers. Deferred with the
      // serving side; unlocked when large clipboard snapshots are needed.
      return Err(backend("CLIPBOARD owner started an INCR transfer, which the X11 driver does not read; clipboard left unchanged"));
    }
    Ok(Some((reply.type_, reply.value)))
  }

  fn has_rich_target(&self, targets: &[Atom]) -> DriverResult<bool> {
    let cookies = targets.iter().map(|&target| self.connection.get_atom_name(target).map_err(backend)).collect::<DriverResult<Vec<_>>>()?;
    for cookie in cookies {
      let name = cookie.reply().map_err(backend)?.name;
      if !is_text_or_meta(&String::from_utf8_lossy(&name)) {
        return Ok(true);
      }
    }
    Ok(false)
  }

  /// Obtains a server timestamp from a zero-length property append, as
  /// ICCCM 2.1 recommends instead of CurrentTime for SetSelectionOwner.
  fn server_time(&self) -> DriverResult<Timestamp> {
    self.shared.lock()?.timestamps.clear();
    self.connection.change_property8(PropMode::APPEND, self.window, self.atoms.AUV_TIMESTAMP, AtomEnum::INTEGER, &[]).map_err(backend)?;
    self.connection.flush().map_err(backend)?;
    self
      .wait(OWNER_TIMEOUT, |state| state.timestamps.pop())?
      .ok_or_else(|| backend("X11 server did not report a timestamp for the clipboard window"))
  }

  /// Sends a marker to our own window and waits for the event thread to
  /// process it, which orders it after every earlier event.
  fn marker(&self, kind: u32) -> DriverResult<()> {
    let sequence = self.send_marker(kind)?;
    self
      .wait(OWNER_TIMEOUT, |state| (state.marker == sequence).then_some(()))?
      .ok_or_else(|| backend("X11 clipboard event thread did not respond"))
  }

  fn send_marker(&self, kind: u32) -> DriverResult<u32> {
    let mut next = self.next_marker.lock().map_err(|_| backend("X11 clipboard marker mutex poisoned"))?;
    *next = next.wrapping_add(1);
    let event = ClientMessageEvent::new(32, self.window, self.atoms.AUV_MARKER, [kind, *next, 0, 0, 0]);
    self.connection.send_event(false, self.window, EventMask::NO_EVENT, event).map_err(backend)?;
    self.connection.flush().map_err(backend)?;
    Ok(*next)
  }

  fn wait<T>(&self, timeout: Duration, mut ready: impl FnMut(&mut State) -> Option<T>) -> DriverResult<Option<T>> {
    let deadline = Instant::now() + timeout;
    let mut state = self.shared.lock()?;
    loop {
      if let Some(value) = ready(&mut state) {
        return Ok(Some(value));
      }
      if let Some(failure) = &state.failure {
        return Err(backend(format!("X11 clipboard connection failed: {failure}; open a new session")));
      }
      let remaining = deadline.saturating_duration_since(Instant::now());
      if remaining.is_zero() {
        return Ok(None);
      }
      state = self.shared.changed.wait_timeout(state, remaining).map_err(|_| backend("X11 clipboard state mutex poisoned"))?.0;
    }
  }
}

impl Drop for Owner {
  fn drop(&mut self) {
    // If the marker cannot be sent the connection is broken and the thread
    // has already returned from `wait_for_event`.
    let _ = self.send_marker(MARKER_SHUTDOWN);
    if let Some(thread) = self.thread.take() {
      let _ = thread.join();
    }
  }
}

fn atoms_of(value: &[u8]) -> Vec<Atom> {
  value.as_chunks::<4>().0.iter().map(|chunk| u32::from_ne_bytes(*chunk)).collect()
}

/// Event loop: answers SelectionRequest and records events callers wait on.
fn serve(connection: &RustConnection, window: Window, atoms: Atoms, limit: usize, shared: &Shared) {
  loop {
    let event = match connection.wait_for_event() {
      Ok(event) => event,
      Err(error) => {
        if let Ok(mut state) = shared.state.lock() {
          state.failure = Some(error.to_string());
        }
        shared.changed.notify_all();
        return;
      }
    };
    let Ok(mut state) = shared.state.lock() else {
      return;
    };
    match event {
      Event::SelectionRequest(request) => {
        let consumer = reply(connection, &atoms, state.served.as_ref(), &request, limit);
        if state.armed {
          state.requests.push((request.requestor, consumer));
        }
      }
      Event::SelectionClear(clear) if clear.selection == atoms.CLIPBOARD && state.served.is_some() => {
        state.served = None;
        state.lost = true;
      }
      Event::SelectionNotify(notification) if notification.requestor == window => state.notifications.push(notification),
      Event::PropertyNotify(property) if property.window == window && property.atom == atoms.AUV_TIMESTAMP => {
        state.timestamps.push(property.time);
      }
      Event::ClientMessage(message) if message.window == window && message.type_ == atoms.AUV_MARKER => {
        let [kind, sequence, ..] = message.data.as_data32();
        if kind == MARKER_SHUTDOWN {
          return;
        }
        if kind == MARKER_ARM {
          state.armed = true;
          state.requests.clear();
        }
        state.marker = sequence;
      }
      // Errors here come from replies to requestors that vanished; the
      // requestor already gave up, so there is nobody to report them to.
      _ => {}
    }
    drop(state);
    shared.changed.notify_all();
  }
}

/// Writes the answer to the requestor's property and notifies it. Returns
/// true when text was delivered.
fn reply(connection: &RustConnection, atoms: &Atoms, served: Option<&Served>, request: &SelectionRequestEvent, limit: usize) -> bool {
  // ICCCM 2.2: an obsolete requestor passes None and expects the target name.
  let property = if request.property == NONE {
    request.target
  } else {
    request.property
  };
  let answer = if request.selection == atoms.CLIPBOARD {
    answer(atoms, served, request.target, request.time, limit)
  } else {
    Answer::Refuse
  };
  let written = match &answer {
    Answer::Atoms(targets) => connection.change_property32(PropMode::REPLACE, request.requestor, property, AtomEnum::ATOM, targets).is_ok(),
    Answer::Timestamp(time) => {
      connection.change_property32(PropMode::REPLACE, request.requestor, property, AtomEnum::INTEGER, &[*time]).is_ok()
    }
    Answer::Text { type_, bytes } => connection.change_property8(PropMode::REPLACE, request.requestor, property, *type_, bytes).is_ok(),
    Answer::Refuse => false,
  };
  let notification = SelectionNotifyEvent {
    response_type: xproto::SELECTION_NOTIFY_EVENT,
    sequence: 0,
    time: request.time,
    requestor: request.requestor,
    selection: request.selection,
    target: request.target,
    property: if written { property } else { NONE },
  };
  let _ = connection.send_event(false, request.requestor, EventMask::NO_EVENT, notification);
  let _ = connection.flush();
  written && matches!(answer, Answer::Text { .. })
}

/// Runs `operation` on the session's clipboard owner, creating it on first
/// use. Holding the slot lock serializes clipboard transactions.
pub(crate) fn with_owner<T>(session: &X11DriverSession, operation: impl FnOnce(&Owner) -> DriverResult<T>) -> DriverResult<T> {
  session.check_environment()?;
  let mut slot = session.clipboard.lock().map_err(|_| backend("X11 clipboard mutex poisoned; open a new session"))?;
  if slot.is_none() {
    *slot = Some(Owner::open(session.display_name())?);
  }
  operation(slot.as_ref().expect("clipboard owner was just created"))
}

/// Text clipboard access through this session's CLIPBOARD ownership.
///
/// `set_text` and `restore` keep the text available only while the session
/// lives; see [`Owner::restore`].
#[derive(Clone, Copy, Debug)]
pub struct ClipboardApi<'a> {
  pub(crate) session: &'a X11DriverSession,
}

impl ClipboardApi<'_> {
  /// Reads CLIPBOARD as text; empty when nobody owns it or it has no text.
  pub fn snapshot(&self) -> DriverResult<String> {
    with_owner(self.session, |owner| {
      Ok(match owner.snapshot()? {
        Snapshot::Owned {
          text: Some(text), ..
        } => text,
        Snapshot::Empty | Snapshot::Owned { text: None, .. } => String::new(),
      })
    })
  }

  /// Writes a snapshot back as CLIPBOARD text.
  pub fn restore(&self, snapshot: &str) -> DriverResult<()> {
    self.set_text(snapshot)
  }

  /// Owns CLIPBOARD and serves `text` until replaced or the session drops.
  pub fn set_text(&self, text: &str) -> DriverResult<()> {
    with_owner(self.session, |owner| {
      check_size(text, owner.limit())?;
      owner.own(text)
    })
  }
}

#[cfg(test)]
#[path = "clipboard_test.rs"]
mod tests;
