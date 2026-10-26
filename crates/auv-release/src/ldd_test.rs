use super::*;

#[test]
fn parses_resolved_missing_and_loader_lines() {
  let output = "\
\tlinux-vdso.so.1 (0x0000ffff9b5f0000)
\tlibtesseract.so.5 => /tmp/b/auv/../lib/auv/libtesseract.so.5 (0x0000ffff9a000000)
\tlibmissing.so.1 => not found
\tlibc.so.6 => /lib/aarch64-linux-gnu/libc.so.6 (0x0000ffff99e00000)
\t/lib/ld-linux-aarch64.so.1 (0x0000ffff9b5b0000)
";
  assert_eq!(
    parse(output),
    vec![
      Dependency::Resolved {
        name: "libtesseract.so.5".into(),
        path: "/tmp/b/auv/../lib/auv/libtesseract.so.5".into(),
      },
      Dependency::Missing("libmissing.so.1".into()),
      Dependency::Resolved {
        name: "libc.so.6".into(),
        path: "/lib/aarch64-linux-gnu/libc.so.6".into(),
      },
      Dependency::Resolved {
        name: "ld-linux-aarch64.so.1".into(),
        path: "/lib/ld-linux-aarch64.so.1".into(),
      },
    ]
  );
}
