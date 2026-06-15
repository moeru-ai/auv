use std::collections::BTreeMap;

use proc_macro::{TokenStream, TokenTree};

#[proc_macro_attribute]
pub fn invoke_command(attr: TokenStream, item: TokenStream) -> TokenStream {
  let handler_name = match find_function_name(item.clone()) {
    Some(name) => name,
    None => return compile_error("invoke_command must annotate a function"),
  };
  let values = match parse_attrs(attr) {
    Ok(values) => values,
    Err(error) => return compile_error(&error),
  };
  for key in [
    "id",
    "group",
    "summary",
    "driver",
    "operation",
    "args",
    "disturbance",
    "max_disturbance",
    "artifacts",
    "signals",
    "verification",
  ] {
    if !values.contains_key(key) {
      return compile_error(&format!(
        "invoke_command missing required `{key}` attribute"
      ));
    }
  }

  let id = &values["id"];
  let group = &values["group"];
  let namespace = match namespace_for_group_literal(group) {
    Ok(namespace) => namespace,
    Err(error) => return compile_error(&error),
  };
  let summary = &values["summary"];
  let driver = &values["driver"];
  let operation = &values["operation"];
  let args = &values["args"];
  let disturbance = &values["disturbance"];
  let max_disturbance = &values["max_disturbance"];
  let artifacts = &values["artifacts"];
  let signals = &values["signals"];
  let verification = &values["verification"];
  let export_name = format!("{handler_name}_invoke_command");

  let spec_call = if let Some(operation_namespace) = values.get("operation_namespace") {
    format!(
      "::auv_cli_invoke::command::spec_with_operation_namespace(
        {id},
        ::auv_cli_invoke::InvokeNamespace::{namespace},
        {operation_namespace},
        {summary},
        {driver},
        {operation},
        {disturbance},
        {max_disturbance},
        {args},
        &{artifacts},
        &{signals},
        {verification},
      )"
    )
  } else {
    format!(
      "::auv_cli_invoke::command::spec(
        {id},
        ::auv_cli_invoke::InvokeNamespace::{namespace},
        {summary},
        {driver},
        {operation},
        {disturbance},
        {max_disturbance},
        {args},
        &{artifacts},
        &{signals},
        {verification},
      )"
    )
  };

  let generated = format!(
    "pub fn {export_name}() -> ::auv_cli_invoke::InvokeCommand {{
      {spec_call}.with_handler({handler_name}, stringify!({handler_name}))
    }}"
  );

  let mut output = item.to_string();
  output.push('\n');
  output.push_str(&generated);
  match output.parse() {
    Ok(tokens) => tokens,
    Err(_) => compile_error("invoke_command generated invalid Rust"),
  }
}

fn find_function_name(item: TokenStream) -> Option<String> {
  let mut saw_fn = false;
  for token in item {
    match token {
      TokenTree::Ident(ident) if saw_fn => return Some(ident.to_string()),
      TokenTree::Ident(ident) if ident.to_string() == "fn" => saw_fn = true,
      _ => {}
    }
  }
  None
}

fn parse_attrs(attr: TokenStream) -> Result<BTreeMap<String, String>, String> {
  let mut values = BTreeMap::new();
  for entry in split_top_level_commas(attr) {
    if entry.is_empty() {
      continue;
    }
    let (key, value) = parse_key_value(entry)?;
    if values.insert(key.clone(), value).is_some() {
      return Err(format!("invoke_command duplicate `{key}` attribute"));
    }
  }
  Ok(values)
}

fn split_top_level_commas(tokens: TokenStream) -> Vec<Vec<TokenTree>> {
  let mut entries = Vec::new();
  let mut current = Vec::new();
  for token in tokens {
    match &token {
      TokenTree::Punct(punct) if punct.as_char() == ',' => {
        entries.push(current);
        current = Vec::new();
      }
      _ => current.push(token),
    }
  }
  if !current.is_empty() {
    entries.push(current);
  }
  entries
}

fn parse_key_value(tokens: Vec<TokenTree>) -> Result<(String, String), String> {
  let mut iter = tokens.into_iter();
  let key = match iter.next() {
    Some(TokenTree::Ident(ident)) => ident.to_string(),
    _ => return Err("invoke_command attributes must start with an identifier key".to_string()),
  };
  match iter.next() {
    Some(TokenTree::Punct(punct)) if punct.as_char() == '=' => {}
    _ => return Err(format!("invoke_command `{key}` must use `=`")),
  }
  let value_tokens: Vec<_> = iter.collect();
  if value_tokens.is_empty() {
    return Err(format!("invoke_command `{key}` must have a value"));
  }
  Ok((key, tokens_to_string(value_tokens)))
}

fn tokens_to_string(tokens: Vec<TokenTree>) -> String {
  tokens.into_iter().collect::<TokenStream>().to_string()
}

fn namespace_for_group_literal(group: &str) -> Result<&'static str, String> {
  match group {
    "\"display\"" => Ok("Display"),
    "\"screen\"" => Ok("Screen"),
    "\"window\"" => Ok("Window"),
    "\"input\"" => Ok("Input"),
    "\"app\"" => Ok("App"),
    "\"overlay\"" => Ok("Overlay"),
    "\"mediaControl\"" => Ok("MediaControl"),
    "\"steam\"" => Ok("Steam"),
    "\"fixture\"" => Ok("Fixture"),
    _ => Err(format!(
      "invoke_command unknown group {group}; expected one of: display, screen, window, input, app, overlay, mediaControl, steam, fixture"
    )),
  }
}

fn compile_error(message: &str) -> TokenStream {
  format!("compile_error!({message:?});")
    .parse()
    .expect("compile_error expansion should parse")
}

#[cfg(test)]
mod tests {
  use super::namespace_for_group_literal;

  #[test]
  fn namespace_for_group_literal_accepts_supported_groups() {
    assert_eq!(namespace_for_group_literal("\"screen\""), Ok("Screen"));
    assert_eq!(
      namespace_for_group_literal("\"mediaControl\""),
      Ok("MediaControl")
    );
    assert_eq!(namespace_for_group_literal("\"steam\""), Ok("Steam"));
  }

  #[test]
  fn namespace_for_group_literal_rejects_unknown_groups() {
    let error = namespace_for_group_literal("\"media_control\"")
      .expect_err("unknown groups should fail during macro expansion");

    assert!(error.contains("invoke_command unknown group"));
    assert!(error.contains("mediaControl"));
  }
}
