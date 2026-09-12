use super::*;

#[test]
fn text_mime_matches_portal_plain_text_contract() {
  assert_eq!(TEXT_MIME, "text/plain;charset=utf-8");
}

// ROOT CAUSE:
// Mutter returns a blocking SelectionWrite pipe. A stalled consumer filled it,
// so File::write never reached the timeout and joining the owner could hang.
// The whole transfer must expire even when the consumer keeps its pipe open.
#[test]
fn stalled_blocking_clipboard_writer_expires_before_consumer_closes() {
  let (reader, writer) = std::io::pipe().unwrap();
  let (sender, receiver) = mpsc::channel();
  let worker = thread::spawn(move || {
    let file = File::from(StdOwnedFd::from(writer));
    sender.send(write_fd_all(file, &vec![b'x'; 8 * 1024 * 1024])).unwrap();
  });
  let result = receiver.recv_timeout(FD_TRANSFER_TIMEOUT + Duration::from_secs(1));
  // Release the blocked old implementation too, so a regression fails rather
  // than leaving an orphaned worker or hanging the test suite.
  drop(reader);
  worker.join().unwrap();
  let error = result.expect("clipboard writer must expire while the reader remains open").unwrap_err();
  assert!(error.to_string().contains("timed out writing portal clipboard"));
}

#[test]
fn stalled_blocking_clipboard_reader_expires_before_owner_closes() {
  let (reader, writer) = std::io::pipe().unwrap();
  let (sender, receiver) = mpsc::channel();
  let worker = thread::spawn(move || {
    let file = File::from(StdOwnedFd::from(reader));
    sender.send(read_fd_to_end(file)).unwrap();
  });
  let result = receiver.recv_timeout(FD_TRANSFER_TIMEOUT + Duration::from_secs(1));
  drop(writer);
  worker.join().unwrap();
  let error = result.expect("clipboard reader must expire while the writer remains open").unwrap_err();
  assert!(error.to_string().contains("timed out reading portal clipboard"));
}

#[test]
fn clipboard_pipe_transfer_preserves_text_across_partial_io() {
  let (reader, writer) = std::io::pipe().unwrap();
  let expected = "clipboard 文本\n".repeat(16 * 1024).into_bytes();
  let payload = expected.clone();
  let worker = thread::spawn(move || write_fd_all(File::from(StdOwnedFd::from(writer)), &payload));
  let actual = read_fd_to_end(File::from(StdOwnedFd::from(reader))).unwrap();
  worker.join().unwrap().unwrap();
  assert_eq!(actual, expected);
}
