use super::*;
use auv_driver_common::App;

fn facts(class: &[u8]) -> WindowFacts {
  WindowFacts {
    wm_class: Some(class.to_vec()),
    normal_type: true,
    viewable: true,
    client_origin: (10, 40),
    client_size: (300, 200),
    ..WindowFacts::default()
  }
}

fn client(xid: u32, class: &[u8], pid: Option<u32>, title: &str) -> ClientWindow {
  client_window(
    xid,
    WindowFacts {
      net_wm_name: Some(title.as_bytes().to_vec()),
      process_id: pid,
      ..facts(class)
    },
  )
}

#[test]
fn window_ids_round_trip_and_reject_foreign_or_zero_ids() {
  assert_eq!(window_id(0x1a00007), "x11:0x1a00007");
  assert_eq!(parse_window_id("x11:0x1a00007").unwrap(), 0x1a00007);
  for id in [
    "0x1a00007",
    "x11:0x0",
    "x11:0x",
    "x11:0x+12",
    "x11:0xzz",
    "x11:0x100000000",
    "123",
  ] {
    assert!(matches!(parse_window_id(id), Err(DriverError::InvalidInput { .. })), "{id}");
  }
}

#[test]
fn wm_class_yields_instance_and_class() {
  assert_eq!(
    parse_wm_class(b"navigator\0Firefox\0"),
    Some(WmClass {
      instance: "navigator".into(),
      class: "Firefox".into(),
    })
  );
  // A missing trailing NUL or class is tolerated; an empty property is not.
  assert_eq!(parse_wm_class(b"xterm").map(|class| class.class), Some(String::new()));
  assert_eq!(parse_wm_class(b"\0\0"), None);
}

#[test]
fn title_prefers_utf8_net_wm_name_then_latin1_wm_name() {
  assert_eq!(decode_title(Some("Ωmega".as_bytes()), Some(b"legacy")), Some("Ωmega".into()));
  assert_eq!(decode_title(None, Some(b"caf\xe9")), Some("café".into()));
  // Invalid UTF-8 in _NET_WM_NAME falls back to WM_NAME rather than inventing text.
  assert_eq!(decode_title(Some(b"\xff\xfe"), Some(b"plain")), Some("plain".into()));
  assert_eq!(decode_title(Some(b""), None), None);
}

#[test]
fn frame_includes_window_manager_decorations() {
  assert_eq!(outer_frame((10, 40), (300, 200), Some([2, 3, 30, 4])), Rect::new(8.0, 10.0, 305.0, 234.0));
  assert_eq!(outer_frame((10, 40), (300, 200), None), Rect::new(10.0, 40.0, 300.0, 200.0));
}

#[test]
fn application_id_prefers_gtk_application_id_then_wm_class() {
  let gtk = client_window(
    1,
    WindowFacts {
      gtk_application_id: Some(b"org.gnome.TextEditor".to_vec()),
      ..facts(b"gnome-text-editor\0Gnome-text-editor\0")
    },
  );
  assert_eq!(gtk.window.application_id.as_deref(), Some("org.gnome.TextEditor"));
  assert_eq!(gtk.window.app_name.as_deref(), Some("Gnome-text-editor"));

  let plain = client(2, b"navigator\0Firefox\0", Some(7), "Mozilla Firefox");
  assert_eq!(plain.window.application_id.as_deref(), Some("Firefox"));
  assert_eq!(plain.window.reference.id, "x11:0x2");
  assert_eq!(plain.window.process_id, Some(7));
  assert_eq!(plain.window.coordinate_space, CoordinateSpace::Screen);

  // Application targets match every identity case-insensitively.
  for id in ["firefox", "FIREFOX", "Navigator"] {
    assert!(plain.is_application(&TextMatcher::Exact(id.into())), "{id}");
  }
  assert!(gtk.is_application(&TextMatcher::Exact("org.gnome.texteditor".into())));
  assert!(gtk.is_application(&TextMatcher::Exact("gnome-text-editor".into())));
  assert!(!plain.is_application(&TextMatcher::Exact("fire".into())));
  assert!(plain.is_application(&TextMatcher::Contains("FIRE".into())));
}

#[test]
fn visibility_and_main_flags_follow_map_state_and_ewmh_hints() {
  let hidden = client_window(
    1,
    WindowFacts {
      hidden: true,
      ..facts(b"a\0A\0")
    },
  );
  assert!(!hidden.window.is_visible);
  let unmapped = client_window(
    2,
    WindowFacts {
      viewable: false,
      ..facts(b"a\0A\0")
    },
  );
  assert!(!unmapped.window.is_visible);
  let dialog = client_window(
    3,
    WindowFacts {
      transient: true,
      ..facts(b"a\0A\0")
    },
  );
  assert!(dialog.window.is_visible && !dialog.window.is_main);
}

#[test]
fn selectors_match_app_identity_title_and_frontmost() {
  let clients = vec![
    client(3, b"editor\0Editor\0", Some(30), "notes.txt — Editor"),
    client(2, b"editor\0Editor\0", Some(30), "todo.txt — Editor"),
    client(1, b"navigator\0Firefox\0", Some(10), "Mozilla Firefox"),
  ];

  let firefox = resolve_from(&clients, &WindowSelector::default().owned_by(App::id("firefox")), None, &[]).unwrap();
  assert_eq!(firefox.xid, 1);
  let todo = resolve_from(&clients, &WindowSelector::default().owned_by(App::id("editor")).title_contains("TODO"), None, &[]).unwrap();
  assert_eq!(todo.xid, 2);
  assert!(matches!(
    resolve_from(&clients, &WindowSelector::default().owned_by(App::id("editor")), None, &[]),
    Err(DriverError::InvalidInput { .. })
  ));
  assert!(matches!(
    resolve_from(&clients, &WindowSelector::default().owned_by(App::id("missing")), None, &[]),
    Err(DriverError::NotFound { .. })
  ));

  // main_visible keeps top-most stacking order unless the active window ranks first.
  let main = WindowSelector {
    main_visible: true,
    ..WindowSelector::default().owned_by(App::id("editor"))
  };
  assert_eq!(resolve_from(&clients, &main, None, &[]).unwrap().xid, 3);
  assert_eq!(resolve_from(&clients, &main, Some(2), &[]).unwrap().xid, 2);

  let frontmost = WindowSelector {
    main_visible: true,
    ..WindowSelector::default().owned_by(App::frontmost())
  };
  assert_eq!(resolve_from(&clients, &frontmost, Some(1), &[]).unwrap().xid, 1);
  assert!(resolve_from(&clients, &frontmost, None, &[]).is_err());
}

#[test]
fn application_target_prefers_active_window_and_rejects_multiple_instances() {
  let clients = vec![
    client(3, b"editor\0Editor\0", Some(30), "a"),
    client(2, b"editor\0Editor\0", Some(30), "b"),
  ];
  assert_eq!(application_window(&clients, "Editor", &[], None).unwrap().xid, 3);
  assert_eq!(application_window(&clients, "editor", &[], Some(2)).unwrap().xid, 2);
  assert!(matches!(application_window(&clients, "missing", &[], None), Err(DriverError::NotFound { .. })));

  let two_processes = vec![
    client(3, b"editor\0Editor\0", Some(30), "a"),
    client(2, b"editor\0Editor\0", Some(31), "b"),
  ];
  assert!(matches!(application_window(&two_processes, "editor", &[], Some(2)), Err(DriverError::InvalidInput { .. })));

  // Without _NET_WM_PID, separate windows cannot be proven to share a process.
  let unknown = vec![
    client(3, b"editor\0Editor\0", None, "a"),
    client(2, b"editor\0Editor\0", None, "b"),
  ];
  assert!(matches!(application_window(&unknown, "editor", &[], None), Err(DriverError::InvalidInput { .. })));
  assert_eq!(application_window(&unknown[..1], "editor", &[], None).unwrap().xid, 3);
}

// ROOT CAUSE:
//
// If a model targeted an application by its desktop id (`gimp`) or a
// macOS-style guess (`com.google.Chrome`), the X11 driver reported only
// "application:… was not found": WM_CLASS is `gimp-2.10`/`Gimp-2.10`, and the
// error did not say which ids exist. OSWorld episodes spent steps guessing.
//
// The fix also accepts the WM_CLASS names a desktop entry of that id declares,
// and a miss lists the application ids of the visible windows.
#[test]
fn desktop_entry_aliases_match_and_a_miss_lists_visible_application_ids() {
  let clients = vec![
    client(3, b"gimp-2.10\0Gimp-2.10\0", Some(30), "GNU Image Manipulation Program"),
    client(2, b"google-chrome\0Google-chrome\0", Some(20), "New Tab - Google Chrome"),
  ];
  let aliases = ["gimp-2.10".to_string()];
  assert_eq!(application_window(&clients, "gimp", &aliases, None).unwrap().xid, 3);
  assert!(matches!(application_window(&clients, "gimp", &[], None), Err(DriverError::NotFound { .. })));
  let selector = WindowSelector::default().owned_by(App::id("gimp"));
  assert_eq!(resolve_from(&clients, &selector, None, &aliases).unwrap().xid, 3);

  let missing = application_window(&clients, "com.google.Chrome", &[], None).unwrap_err().to_string();
  assert_eq!(missing, "application:com.google.Chrome (visible applications: Gimp-2.10, Google-chrome) was not found");
  let missing = resolve_from(&clients, &WindowSelector::default().owned_by(App::id("org.gimp.GIMP")), None, &[]).unwrap_err();
  assert!(missing.to_string().contains("visible applications: Gimp-2.10, Google-chrome"), "{missing}");
}

// ROOT CAUSE:
//
// If a LibreOffice Calc dialog was active, an `application:libreoffice-calc`
// target resolved to the Calc frame (the dialog's WM_CLASS differs), and the
// window manager refused to activate the frame behind the modal dialog. Seven
// OSWorld keyboard and click commands failed with "did not activate within
// 2000ms" on 2026-10-13.
//
// The fix keeps input on the active window when the same process owns it.
#[test]
fn application_target_follows_the_active_dialog_of_the_same_process() {
  let clients = vec![
    client(0x602a7a, b"soffice\0Soffice\0", Some(40), "Format Cells"),
    client(0x600096, b"libreoffice-calc\0libreoffice-calc\0", Some(40), "Untitled 1 - LibreOffice Calc"),
    client(0x700001, b"gedit\0Gedit\0", Some(50), "notes"),
  ];
  assert_eq!(application_window(&clients, "libreoffice-calc", &[], Some(0x602a7a)).unwrap().xid, 0x602a7a);
  // Another process's active window does not capture the target.
  assert_eq!(application_window(&clients, "libreoffice-calc", &[], Some(0x700001)).unwrap().xid, 0x600096);
  // Without a pid, ownership cannot be proven; the frame is used.
  let unknown = vec![
    client(0x602a7a, b"soffice\0Soffice\0", None, "Format Cells"),
    client(0x600096, b"libreoffice-calc\0libreoffice-calc\0", None, "Calc"),
  ];
  assert_eq!(application_window(&unknown, "libreoffice-calc", &[], Some(0x602a7a)).unwrap().xid, 0x600096);
}
