//! EIS input delivery for the RemoteDesktop portal.
//!
//! The portal owns the RemoteDesktop session and hands AUV one end of an EIS
//! socket. This module owns the protocol worker because `EiEventConverter` is
//! not `Send`; callers use synchronous commands and one wake pipe.
use std::collections::HashSet;
use std::io;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use ashpd::enumflags2::BitFlags;
use auv_driver_common::error::{DriverError, DriverResult};
use auv_driver_common::geometry::Point;
use auv_driver_common::input::MouseButton;
use evdev::KeyCode;
use reis::PendingRequestResult;
use reis::ei;
use reis::event::{Device, DeviceCapability, EiEvent, EiEventConverter, Region};
use reis::handshake::{EiHandshaker, HandshakeResp};

use crate::error::{backend, invalid_input};

use super::keymap::Keymap;

const EIS_NAME: &str = "auv";
const OPEN_TIMEOUT: Duration = Duration::from_secs(3);
const BUTTON_LEFT: u32 = 0x110;
const BUTTON_RIGHT: u32 = 0x111;
const BUTTON_MIDDLE: u32 = 0x112;

/// A synchronous EIS input client. One worker owns the protocol converter and
/// all device state; calls never block the protocol loop.
pub(crate) struct EisInput {
  commands: Sender<Command>,
  wake: Option<OwnedFd>,
  worker: Option<JoinHandle<()>>,
  state: Arc<WorkerState>,
}

struct WorkerState {
  keymap: Mutex<Option<Keymap>>,
  failure: Mutex<Option<String>>,
  connected: AtomicBool,
}

impl WorkerState {
  fn new() -> Self {
    Self {
      keymap: Mutex::new(None),
      failure: Mutex::new(None),
      connected: AtomicBool::new(true),
    }
  }
}

impl EisInput {
  /// Starts a sender EIS context. `expect_keyboard` and `expect_pointer` must
  /// match the devices the portal session selected; open fails when the
  /// compositor does not provide them.
  pub fn open(fd: OwnedFd, expect_keyboard: bool, expect_pointer: bool) -> DriverResult<Self> {
    let (commands, command_rx) = mpsc::channel();
    let (wake_read, wake_write) = wake_pipe()?;
    let state = Arc::new(WorkerState::new());
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);

    let worker_state = Arc::clone(&state);
    let worker = thread::Builder::new()
      .name("auv-eis-input".to_owned())
      .spawn(move || {
        run_worker(fd, wake_read, command_rx, ready_tx, worker_state, expect_keyboard, expect_pointer);
      })
      .map_err(|error| backend(format!("start EIS input worker: {error}")))?;

    let mut input = Self {
      commands,
      wake: Some(wake_write),
      worker: Some(worker),
      state,
    };

    match ready_rx.recv_timeout(OPEN_TIMEOUT) {
      Ok(Ok(())) => {}
      Ok(Err(error)) => {
        input.shutdown();
        return Err(error);
      }
      Err(mpsc::RecvTimeoutError::Timeout) => {
        input.shutdown();
        return Err(backend("EIS input did not become ready within 3s"));
      }
      Err(mpsc::RecvTimeoutError::Disconnected) => {
        input.shutdown();
        return Err(input.failure());
      }
    }

    if expect_keyboard && input.state.keymap.lock().map_err(|_| backend("EIS keymap lock poisoned"))?.is_none() {
      match Keymap::load() {
        Ok(keymap) => *input.state.keymap.lock().map_err(|_| backend("EIS keymap lock poisoned"))? = Some(keymap),
        Err(error) => {
          input.shutdown();
          return Err(error);
        }
      }
    }
    if !input.state.connected.load(Ordering::SeqCst) {
      input.shutdown();
      return Err(input.failure());
    }
    Ok(input)
  }

  pub fn move_to(&self, point: Point) -> DriverResult<()> {
    self.call(|reply| Command::Move { point, reply })
  }

  pub fn button(&self, point: Point, button: MouseButton, down: bool) -> DriverResult<()> {
    self.call(|reply| Command::Button {
      point,
      button: button_code(button),
      down,
      reply,
    })
  }

  pub fn scroll(&self, point: Point, notches: (i32, i32)) -> DriverResult<()> {
    self.call(|reply| Command::Scroll {
      point,
      notches,
      reply,
    })
  }

  pub fn key_transition(&self, key: KeyCode, down: bool) -> DriverResult<()> {
    self.call(|reply| Command::Key { key, down, reply })
  }

  pub fn keymap(&self) -> DriverResult<Keymap> {
    self
      .state
      .keymap
      .lock()
      .map_err(|_| backend("EIS keymap lock poisoned"))?
      .clone()
      .ok_or_else(|| backend("EIS keyboard has no XKB keymap"))
  }

  fn call(&self, command: impl FnOnce(SyncSender<DriverResult<()>>) -> Command) -> DriverResult<()> {
    if !self.state.connected.load(Ordering::SeqCst) {
      return Err(self.failure());
    }
    let (reply, response) = mpsc::sync_channel(0);
    self.commands.send(command(reply)).map_err(|_| self.failure())?;
    wake(self.wake.as_ref())?;
    response.recv().map_err(|_| self.failure())?
  }

  fn failure(&self) -> DriverError {
    let detail = self.state.failure.lock().ok().and_then(|failure| failure.clone());
    backend(detail.unwrap_or_else(|| "EIS input session closed".to_owned()))
  }

  pub fn shutdown(&mut self) {
    self.wake.take();
    if let Some(worker) = self.worker.take() {
      let _ = worker.join();
    }
  }
}

impl Drop for EisInput {
  fn drop(&mut self) {
    self.shutdown();
  }
}

enum Command {
  Move {
    point: Point,
    reply: SyncSender<DriverResult<()>>,
  },
  Button {
    point: Point,
    button: u32,
    down: bool,
    reply: SyncSender<DriverResult<()>>,
  },
  Scroll {
    point: Point,
    notches: (i32, i32),
    reply: SyncSender<DriverResult<()>>,
  },
  Key {
    key: KeyCode,
    down: bool,
    reply: SyncSender<DriverResult<()>>,
  },
}

struct PointerDevice {
  device: Device,
  paused: bool,
  absolute: ei::PointerAbsolute,
  button: Option<ei::Button>,
  scroll: Option<ei::Scroll>,
}

struct KeyboardDevice {
  device: Device,
  paused: bool,
  keyboard: ei::Keyboard,
}

struct State {
  pointers: Vec<PointerDevice>,
  keyboards: Vec<KeyboardDevice>,
  // TODO(eis-multi-monitor-drag): this host has one output, so the press/release
  // route is not live-tested across two absolute devices. Add a fake-EIS or
  // multi-monitor regression when that setup is available.
  button_device: Option<ei::Device>,
  started: HashSet<ei::Device>,
  sequence: u32,
  worker: Arc<WorkerState>,
}

impl State {
  fn new(worker: Arc<WorkerState>) -> Self {
    Self {
      pointers: Vec::new(),
      keyboards: Vec::new(),
      button_device: None,
      started: HashSet::new(),
      sequence: 0,
      worker,
    }
  }

  fn ready(&self, expect_keyboard: bool, expect_pointer: bool) -> bool {
    (!expect_keyboard || self.keyboards.iter().any(|keyboard| !keyboard.paused))
      && (!expect_pointer || self.pointers.iter().any(|pointer| !pointer.paused))
  }

  fn handle(&mut self, event: EiEvent, converter: &EiEventConverter) -> DriverResult<()> {
    match event {
      EiEvent::SeatAdded(seat) => {
        let capabilities: BitFlags<DeviceCapability> = [
          DeviceCapability::PointerAbsolute,
          DeviceCapability::Button,
          DeviceCapability::Scroll,
          DeviceCapability::Keyboard,
        ]
        .into_iter()
        .collect();
        seat.seat.bind_capabilities(capabilities);
      }
      EiEvent::DeviceAdded(added) => self.add_device(added.device, converter)?,
      EiEvent::DeviceRemoved(removed) => self.remove_device(&removed.device),
      EiEvent::DevicePaused(paused) => self.set_paused(&paused.device, true),
      EiEvent::DeviceResumed(resumed) => self.set_paused(&resumed.device, false),
      EiEvent::KeyboardModifiers(modifiers) => {
        if let Some(keymap) = self.worker.keymap.lock().map_err(|_| backend("EIS keymap lock poisoned"))?.as_mut() {
          keymap.set_group(modifiers.group);
        }
      }
      EiEvent::Disconnected(disconnected) => {
        return Err(backend(format!(
          "EIS disconnected ({:?}){}",
          disconnected.reason,
          disconnected.explanation.map(|explanation| format!(": {explanation}")).unwrap_or_default()
        )));
      }
      _ => {}
    }
    Ok(())
  }

  fn add_device(&mut self, device: Device, converter: &EiEventConverter) -> DriverResult<()> {
    let keyboard = device.interface::<ei::Keyboard>();
    let pointer = device.interface::<ei::PointerAbsolute>();
    if keyboard.is_none() && pointer.is_none() {
      return Ok(());
    }
    if let Some(keymap) = device.keymap()
      && keymap.type_ == ei::keyboard::KeymapType::Xkb
      && let Ok(loaded) = Keymap::from_fd(keymap.fd.as_fd(), keymap.size)
    {
      *self.worker.keymap.lock().map_err(|_| backend("EIS keymap lock poisoned"))? = Some(loaded);
    }
    if let Some(keyboard) = keyboard {
      self.keyboards.push(KeyboardDevice {
        device: device.clone(),
        paused: true,
        keyboard,
      });
    }
    if let Some(absolute) = pointer {
      self.pointers.push(PointerDevice {
        device: device.clone(),
        paused: true,
        absolute,
        button: device.interface::<ei::Button>(),
        scroll: device.interface::<ei::Scroll>(),
      });
    }
    if self.started.insert(device.device().clone()) {
      if device.device().version() >= 3 {
        device.device().ready();
      }
      let sequence = self.sequence.wrapping_add(1).max(1);
      self.sequence = sequence;
      device.device().start_emulating(converter.connection().serial(), sequence);
      converter.connection().flush().map_err(|error| backend(format!("start EIS emulation: {error}")))?;
    }
    Ok(())
  }

  fn remove_device(&mut self, device: &Device) {
    if self.button_device.as_ref() == Some(device.device()) {
      self.button_device = None;
    }
    self.keyboards.retain(|keyboard| &keyboard.device != device);
    self.pointers.retain(|pointer| &pointer.device != device);
    self.started.remove(device.device());
  }

  fn set_paused(&mut self, device: &Device, paused: bool) {
    for keyboard in &mut self.keyboards {
      if &keyboard.device == device {
        keyboard.paused = paused;
      }
    }
    for pointer in &mut self.pointers {
      if &pointer.device == device {
        pointer.paused = paused;
      }
    }
  }

  fn pointer_index_for_point(&self, point: Point) -> DriverResult<usize> {
    if !point.x.is_finite() || !point.y.is_finite() {
      return Err(invalid_input("EIS pointer target must be finite"));
    }
    self
      .pointers
      .iter()
      .position(|pointer| !pointer.paused && pointer.device.regions().iter().any(|region| region_contains(region, point)))
      .ok_or_else(|| invalid_input(format!("no EIS pointer region contains point {point:?}")))
  }

  fn move_to(&mut self, point: Point, converter: &EiEventConverter) -> DriverResult<()> {
    let index = self.pointer_index_for_point(point)?;
    let pointer = &mut self.pointers[index];
    pointer.absolute.motion_absolute(point.x as f32, point.y as f32);
    frame(&pointer.device, converter);
    Ok(())
  }

  fn button(&mut self, point: Point, button: u32, down: bool, converter: &EiEventConverter) -> DriverResult<()> {
    let index = if down {
      if self.button_device.is_some() {
        return Err(backend("EIS pointer already has a pressed button"));
      }
      self.pointer_index_for_point(point)?
    } else if let Some(device) = self.button_device.as_ref() {
      // Keep the release on the device that received the press, even when the
      // pointer moved into another region during the drag.
      self
        .pointers
        .iter()
        .position(|pointer| pointer.device.device() == device)
        .ok_or_else(|| backend("EIS button device is no longer available"))?
    } else {
      self.pointer_index_for_point(point)?
    };
    let pointer = &mut self.pointers[index];
    let interface = pointer.button.as_ref().ok_or_else(|| backend("EIS pointer has no button capability"))?;
    interface.button(
      button,
      if down {
        ei::button::ButtonState::Press
      } else {
        ei::button::ButtonState::Released
      },
    );
    frame(&pointer.device, converter);
    self.button_device = down.then(|| pointer.device.device().clone());
    Ok(())
  }

  fn scroll(&mut self, point: Point, notches: (i32, i32), converter: &EiEventConverter) -> DriverResult<()> {
    let index = self.pointer_index_for_point(point)?;
    let pointer = &mut self.pointers[index];
    let interface = pointer.scroll.as_ref().ok_or_else(|| backend("EIS pointer has no scroll capability"))?;
    interface.scroll_discrete(notches.0.saturating_mul(120), notches.1.saturating_mul(120));
    frame(&pointer.device, converter);
    Ok(())
  }

  fn key(&mut self, key: KeyCode, down: bool, converter: &EiEventConverter) -> DriverResult<()> {
    let keyboard = self.keyboards.iter().find(|keyboard| !keyboard.paused).ok_or_else(|| backend("EIS keyboard is not resumed"))?;
    keyboard.keyboard.key(
      u32::from(key.0),
      if down {
        ei::keyboard::KeyState::Press
      } else {
        ei::keyboard::KeyState::Released
      },
    );
    frame(&keyboard.device, converter);
    Ok(())
  }
}

fn run_worker(
  fd: OwnedFd,
  wake_read: OwnedFd,
  commands: Receiver<Command>,
  ready: SyncSender<DriverResult<()>>,
  state: Arc<WorkerState>,
  expect_keyboard: bool,
  expect_pointer: bool,
) {
  let mut ready = Some(ready);
  let result = run_worker_inner(fd, &wake_read, &commands, &mut ready, Arc::clone(&state), expect_keyboard, expect_pointer);
  state.connected.store(false, Ordering::SeqCst);
  if let Err(error) = result {
    if let Ok(mut failure) = state.failure.lock() {
      *failure = Some(error.to_string());
    }
    if let Some(ready) = ready {
      let _ = ready.send(Err(error));
    }
  }
}

fn run_worker_inner(
  fd: OwnedFd,
  wake_read: &OwnedFd,
  commands: &Receiver<Command>,
  ready: &mut Option<SyncSender<DriverResult<()>>>,
  worker: Arc<WorkerState>,
  expect_keyboard: bool,
  expect_pointer: bool,
) -> DriverResult<()> {
  let context = ei::Context::new(UnixStream::from(fd)).map_err(|error| backend(format!("open EIS socket: {error}")))?;
  let response = handshake(&context, wake_read)?;
  let mut converter = EiEventConverter::new(&context, response);
  let mut state = State::new(Arc::clone(&worker));

  // The compositor sends seats and devices immediately after the handshake.
  let deadline = Instant::now() + OPEN_TIMEOUT;
  loop {
    drain_pending(&context, &mut converter)?;
    drain_converted(&mut converter, &mut state)?;
    if state.ready(expect_keyboard, expect_pointer) {
      break;
    }
    let now = Instant::now();
    if now >= deadline {
      return Err(backend("EIS portal did not provide the selected keyboard and pointer devices"));
    }
    match poll(&context, wake_read.as_raw_fd(), Some(deadline - now))? {
      PollResult::Eis => read_context(&context)?,
      PollResult::Wake => return Err(backend("EIS input closed while opening")),
      PollResult::Timeout | PollResult::Interrupted => {}
    }
  }
  if let Some(ready) = ready.take() {
    let _ = ready.send(Ok(()));
  }

  loop {
    drain_pending(&context, &mut converter)?;
    drain_converted(&mut converter, &mut state)?;
    match poll(&context, wake_read.as_raw_fd(), None)? {
      PollResult::Eis => read_context(&context)?,
      PollResult::Wake => {
        if !drain_wake(wake_read)? {
          return Ok(());
        }
        while let Ok(command) = commands.try_recv() {
          handle_command(&mut state, &mut converter, command)?;
        }
      }
      PollResult::Timeout | PollResult::Interrupted => {}
    }
  }
}

fn handshake(context: &ei::Context, wake_read: &OwnedFd) -> DriverResult<HandshakeResp> {
  let mut handshaker = EiHandshaker::new(EIS_NAME, ei::handshake::ContextType::Sender);
  let deadline = Instant::now() + OPEN_TIMEOUT;
  loop {
    while let Some(pending) = context.pending_event() {
      let event = match pending {
        PendingRequestResult::Request(event) => event,
        PendingRequestResult::ParseError(error) => return Err(backend(format!("parse EIS handshake: {error}"))),
        PendingRequestResult::InvalidObject(id) => return Err(backend(format!("EIS handshake used invalid object {id}"))),
      };
      if let Some(response) = handshaker.handle_event(event).map_err(|error| backend(format!("EIS handshake: {error}")))? {
        return Ok(response);
      }
    }
    let now = Instant::now();
    if now >= deadline {
      return Err(backend("EIS handshake timed out"));
    }
    match poll(context, wake_read.as_raw_fd(), Some(deadline - now))? {
      PollResult::Eis => read_context(context)?,
      PollResult::Wake => return Err(backend("EIS input closed during the handshake")),
      PollResult::Timeout | PollResult::Interrupted => {}
    }
  }
}

fn drain_pending(context: &ei::Context, converter: &mut EiEventConverter) -> DriverResult<()> {
  while let Some(pending) = context.pending_event() {
    match pending {
      PendingRequestResult::Request(event) => {
        converter.handle_event(event).map_err(|error| backend(format!("handle EIS event: {error}")))?;
      }
      PendingRequestResult::ParseError(error) => return Err(backend(format!("parse EIS event: {error}"))),
      PendingRequestResult::InvalidObject(id) => return Err(backend(format!("EIS event used invalid object {id}"))),
    }
  }
  Ok(())
}

fn drain_converted(converter: &mut EiEventConverter, state: &mut State) -> DriverResult<()> {
  while let Some(event) = converter.next_event() {
    state.handle(event, converter)?;
  }
  converter.connection().flush().map_err(|error| backend(format!("flush EIS requests: {error}")))
}

fn read_context(context: &ei::Context) -> DriverResult<()> {
  match context.read() {
    Ok(0) => Err(backend("EIS connection closed")),
    Ok(_) => Ok(()),
    Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(()),
    Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(()),
    Err(error) => Err(backend(format!("read EIS connection: {error}"))),
  }
}

fn handle_command(state: &mut State, converter: &mut EiEventConverter, command: Command) -> DriverResult<()> {
  let (reply, result) = match command {
    Command::Move { point, reply } => (reply, state.move_to(point, converter)),
    Command::Button {
      point,
      button,
      down,
      reply,
    } => (reply, state.button(point, button, down, converter)),
    Command::Scroll {
      point,
      notches,
      reply,
    } => (reply, state.scroll(point, notches, converter)),
    Command::Key { key, down, reply } => (reply, state.key(key, down, converter)),
  };
  match result {
    Ok(()) => {
      if let Err(error) = flush_command(converter) {
        let _ = reply.send(Err(backend(error.to_string())));
        return Err(error);
      }
      let _ = reply.send(Ok(()));
      Ok(())
    }
    Err(error) => {
      let _ = reply.send(Err(error));
      Ok(())
    }
  }
}

fn flush_command(converter: &EiEventConverter) -> DriverResult<()> {
  converter.connection().flush().map_err(|error| backend(format!("flush EIS command: {error}")))
}

fn frame(device: &Device, converter: &EiEventConverter) {
  device.device().frame(converter.connection().serial(), now_micros());
}

fn now_micros() -> u64 {
  let mut time = libc::timespec {
    tv_sec: 0,
    tv_nsec: 0,
  };
  // SAFETY: `time` points to valid writable memory and CLOCK_MONOTONIC is a
  // process-wide clock with no additional initialization.
  unsafe {
    libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time);
  }
  (time.tv_sec as u64).saturating_mul(1_000_000) + (time.tv_nsec as u64) / 1_000
}

fn button_code(button: MouseButton) -> u32 {
  match button {
    MouseButton::Left => BUTTON_LEFT,
    MouseButton::Right => BUTTON_RIGHT,
    MouseButton::Middle => BUTTON_MIDDLE,
  }
}

fn region_contains(region: &Region, point: Point) -> bool {
  point.x >= f64::from(region.x)
    && point.y >= f64::from(region.y)
    && point.x < f64::from(region.x) + f64::from(region.width)
    && point.y < f64::from(region.y) + f64::from(region.height)
}

fn wake_pipe() -> DriverResult<(OwnedFd, OwnedFd)> {
  let mut fds = [0; 2];
  // SAFETY: `fds` is a valid two-element array for pipe2.
  let result = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) };
  if result != 0 {
    return Err(backend(format!("create EIS wake pipe: {}", io::Error::last_os_error())));
  }
  // SAFETY: pipe2 returned two new owned descriptors.
  Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

fn wake(write: Option<&OwnedFd>) -> DriverResult<()> {
  let Some(write) = write else {
    return Err(backend("EIS input session closed"));
  };
  loop {
    // SAFETY: the descriptor is owned by this handle and the byte is initialized.
    let written = unsafe { libc::write(write.as_raw_fd(), b"x".as_ptr().cast(), 1) };
    if written == 1 {
      return Ok(());
    }
    if written == 0 {
      return Err(backend("wake EIS input worker wrote no bytes"));
    }
    let error = io::Error::last_os_error();
    match error.kind() {
      // A full pipe already holds a wake byte for the worker.
      io::ErrorKind::WouldBlock => return Ok(()),
      io::ErrorKind::Interrupted => continue,
      _ => return Err(backend(format!("wake EIS input worker: {error}"))),
    }
  }
}

fn drain_wake(read: &OwnedFd) -> DriverResult<bool> {
  let mut open = true;
  let mut buffer = [0_u8; 64];
  loop {
    // SAFETY: the descriptor is owned by the worker and the buffer is valid.
    let read_bytes = unsafe { libc::read(read.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len()) };
    if read_bytes > 0 {
      continue;
    }
    if read_bytes == 0 {
      open = false;
      break;
    }
    let error = io::Error::last_os_error();
    match error.kind() {
      io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted => break,
      _ => return Err(backend(format!("read EIS wake pipe: {error}"))),
    }
  }
  Ok(open)
}

enum PollResult {
  Eis,
  Wake,
  Timeout,
  Interrupted,
}

fn poll(context: &ei::Context, wake: RawFd, timeout: Option<Duration>) -> DriverResult<PollResult> {
  let mut fds = [
    libc::pollfd {
      fd: context.as_raw_fd(),
      events: libc::POLLIN,
      revents: 0,
    },
    libc::pollfd {
      fd: wake,
      events: libc::POLLIN,
      revents: 0,
    },
  ];
  let timeout = timeout.map(|timeout| timeout.as_millis().min(i32::MAX as u128) as i32).unwrap_or(-1);
  // SAFETY: `fds` is a valid two-element array for poll.
  let result = unsafe { libc::poll(fds.as_mut_ptr(), 2, timeout) };
  if result < 0 {
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::Interrupted {
      return Ok(PollResult::Interrupted);
    }
    return Err(backend(format!("poll EIS input: {error}")));
  }
  if result == 0 {
    return Ok(PollResult::Timeout);
  }
  if fds[1].revents != 0 {
    return Ok(PollResult::Wake);
  }
  if fds[0].revents != 0 {
    return Ok(PollResult::Eis);
  }
  Ok(PollResult::Wake)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn region(x: u32, y: u32, width: u32, height: u32) -> Region {
    Region {
      x,
      y,
      width,
      height,
      scale: 1.0,
      mapping_id: None,
    }
  }

  #[test]
  fn region_uses_half_open_bounds() {
    let region = region(100, 200, 800, 600);
    assert!(region_contains(&region, Point::new(100.0, 200.0)));
    assert!(region_contains(&region, Point::new(899.9, 799.9)));
    assert!(!region_contains(&region, Point::new(900.0, 799.0)));
    assert!(!region_contains(&region, Point::new(899.0, 800.0)));
    assert!(!region_contains(&region, Point::new(99.9, 200.0)));
  }

  #[test]
  fn frame_timestamps_use_monotonic_microseconds() {
    let before = now_micros();
    std::thread::sleep(Duration::from_millis(2));
    let after = now_micros();
    assert!(after > before);
    assert!(after - before >= 1_000);
  }

  fn handshake_converter() -> (ei::Context, EiEventConverter) {
    let (client, server) = UnixStream::pair().unwrap();
    let client_context = ei::Context::new(client).unwrap();
    let server_context = reis::eis::Context::new(server).unwrap();
    let server_thread = thread::spawn(move || {
      let mut handshaker = reis::handshake::EisHandshaker::new(&server_context, 1);
      let deadline = Instant::now() + Duration::from_secs(2);
      loop {
        match server_context.read() {
          Ok(_) => {}
          Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            assert!(Instant::now() < deadline, "test EIS handshake timed out");
            thread::sleep(Duration::from_millis(1));
            continue;
          }
          Err(error) => panic!("test EIS server read failed: {error}"),
        }
        while let Some(pending) = server_context.pending_request() {
          let request = match pending {
            reis::PendingRequestResult::Request(request) => request,
            _ => panic!("test EIS server received an invalid handshake request"),
          };
          if handshaker.handle_request(request).unwrap().is_some() {
            server_context.flush().unwrap();
            return;
          }
        }
      }
    });
    let response = reis::handshake::ei_handshake_blocking(&client_context, "test", ei::handshake::ContextType::Sender).unwrap();
    server_thread.join().unwrap();
    let converter = EiEventConverter::new(&client_context, response);
    (client_context, converter)
  }

  // ROOT CAUSE:
  //
  // The worker replied before flushing the EIS command. A flush failure was
  // lost, so the caller could treat an uncommitted event as delivered.
  //
  // Before the fix, the command reply path ignored `flush()`.
  // The fix flushes first and reports the transport error to the caller.
  #[test]
  fn flush_failure_is_backend_error() {
    let (_context, converter) = handshake_converter();
    converter.connection().connection().sync(1);
    let message = flush_command(&converter).unwrap_err().to_string();
    assert!(message.contains("flush EIS command"), "{message}");
  }
}
