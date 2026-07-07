use std::collections::BTreeMap;

use proc_macro::{TokenStream, TokenTree};

const ALLOWED_ATTR_KEYS: &[&str] = &["id", "group", "summary", "args"];

#[proc_macro_attribute]
pub fn invoke_command(attr: TokenStream, item: TokenStream) -> TokenStream {
  let function_name = match find_function_name(item.clone()) {
    Some(name) => name,
    None => return compile_error("invoke_command must annotate a function"),
  };
  let values = match parse_attrs(attr) {
    Ok(values) => values,
    Err(error) => return compile_error(&error),
  };
  for key in ALLOWED_ATTR_KEYS {
    if !values.contains_key(*key) {
      return compile_error(&format!("invoke_command missing required `{key}` attribute"));
    }
  }

  let id = &values["id"];
  let group = &values["group"];
  let namespace = match namespace_for_group_literal(group) {
    Ok(namespace) => namespace,
    Err(error) => return compile_error(&error),
  };
  let summary = &values["summary"];
  let args = &values["args"];
  let export_name = format!("{function_name}_invoke_command");

  let generated = format!(
    "pub fn {export_name}() -> ::auv_cli_invoke::InvokeCommand {{
      ::auv_cli_invoke::command::spec(
        {id},
        ::auv_cli_invoke::InvokeNamespace::{namespace},
        {summary},
        {args},
        {function_name},
      )
    }}"
  );

  let mut output = item.to_string();
  output.push('\n');
  output.push_str(&generated);
  output.parse().map_or_else(|_| compile_error("invoke_command generated invalid Rust"), |tokens| tokens)
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
    validate_attr_key(&key)?;
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

fn validate_attr_key(key: &str) -> Result<(), String> {
  if ALLOWED_ATTR_KEYS.contains(&key) {
    Ok(())
  } else {
    Err(format!("invoke_command unknown attribute `{key}`; expected only: id, group, summary, args"))
  }
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
    "\"fixture\"" => Ok("Fixture"),
    "\"scan\"" => Ok("Scan"),
    _ => Err(format!(
      "invoke_command unknown group {group}; expected one of: display, screen, window, input, app, overlay, mediaControl, fixture, scan"
    )),
  }
}

fn compile_error(message: &str) -> TokenStream {
  format!("compile_error!({message:?});").parse().expect("compile_error expansion should parse")
}

#[cfg(test)]
mod tests {
  use super::{namespace_for_group_literal, validate_attr_key};

  #[test]
  fn namespace_for_group_literal_accepts_supported_groups() {
    assert_eq!(namespace_for_group_literal("\"screen\""), Ok("Screen"));
    assert_eq!(namespace_for_group_literal("\"mediaControl\""), Ok("MediaControl"));
    assert_eq!(namespace_for_group_literal("\"scan\""), Ok("Scan"));
  }

  #[test]
  fn namespace_for_group_literal_rejects_unknown_groups() {
    let error = namespace_for_group_literal("\"media_control\"").expect_err("unknown groups should fail during macro expansion");

    assert!(error.contains("invoke_command unknown group"));
    assert!(error.contains("mediaControl"));
  }

  #[test]
  fn validate_attr_key_rejects_execution_metadata_keys() {
    for key in [
      "driver",
      "operation",
      "disturbance",
      "max_disturbance",
      "artifacts",
      "signals",
      "verification",
      "operation_namespace",
    ] {
      let error = validate_attr_key(key).expect_err("old execution metadata should be rejected");

      assert!(error.contains("invoke_command unknown attribute"));
      assert!(error.contains(key));
      assert!(error.contains("id, group, summary, args"));
    }
  }
}
