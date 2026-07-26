use super::*;

#[test]
fn parses_multiple_zero_arg_invocations() {
  let invocations = parse(["permissions", "windows", "copy", "paste", "input-boundary"]);

  assert_eq!(
    invocations,
    vec![
      invocation("permissions", []),
      invocation("windows", []),
      invocation("copy", []),
      invocation("paste", []),
      invocation("input-boundary", [])
    ]
  );
}

#[test]
fn parses_mixed_invocations_with_fixed_args() {
  let invocations = parse([
    "resolve",
    "Terminal",
    "click",
    "10",
    "20",
    "window-click",
    "Terminal",
    "10",
    "20",
    "find-window-text",
    "Terminal",
    "Shell",
    "paste-text",
    "hello",
    "press",
    "Return",
  ]);

  assert_eq!(
    invocations,
    vec![
      invocation("resolve", ["Terminal"]),
      invocation("click", ["10", "20"]),
      invocation("window-click", ["Terminal", "10", "20"]),
      invocation("find-window-text", ["Terminal", "Shell"]),
      invocation("paste-text", ["hello"]),
      invocation("press", ["Return"])
    ]
  );
}

#[test]
fn optional_args_stop_at_next_command() {
  let invocations = parse(["capture-screen", "clipboard"]);

  assert_eq!(
    invocations,
    vec![
      invocation("capture-screen", []),
      invocation("clipboard", [])
    ]
  );
}

#[test]
fn explicit_separator_disambiguates_optional_args() {
  let invocations = parse(["capture-screen", "--", "clipboard"]);

  assert_eq!(
    invocations,
    vec![
      invocation("capture-screen", []),
      invocation("clipboard", [])
    ]
  );
}

fn parse<const N: usize>(args: [&str; N]) -> Vec<Invocation> {
  let args = args.into_iter().map(ToString::to_string).collect::<Vec<_>>();
  parse_invocations(&args).expect("args should parse")
}

fn invocation<const N: usize>(command: &str, args: [&str; N]) -> Invocation {
  Invocation {
    command: command.to_string(),
    args: args.into_iter().map(ToString::to_string).collect(),
  }
}
