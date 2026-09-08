use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{Ident, ItemFn, LitStr, Result, Token, Type, parse_macro_input};

#[proc_macro_attribute]
pub fn invoke_command(attr: TokenStream, item: TokenStream) -> TokenStream {
  let attributes = parse_macro_input!(attr as InvokeCommandAttributes);
  let function = parse_macro_input!(item as ItemFn);
  expand(attributes, function).unwrap_or_else(syn::Error::into_compile_error).into()
}

struct InvokeCommandAttributes {
  id: LitStr,
  group: LitStr,
  description: LitStr,
  input: Type,
  target: Option<Ident>,
}

impl Parse for InvokeCommandAttributes {
  fn parse(input: ParseStream<'_>) -> Result<Self> {
    let mut id = None;
    let mut group = None;
    let mut description = None;
    let mut input_type = None;
    let mut target = None;

    while !input.is_empty() {
      let key = input.parse::<Ident>()?;
      input.parse::<Token![=]>()?;
      match key.to_string().as_str() {
        "id" => set_once(&mut id, input.parse::<LitStr>()?, &key)?,
        "group" => set_once(&mut group, input.parse::<LitStr>()?, &key)?,
        "description" => set_once(&mut description, input.parse::<LitStr>()?, &key)?,
        "target" => set_once(&mut target, input.parse::<Ident>()?, &key)?,
        "input" => set_once(&mut input_type, input.parse::<Type>()?, &key)?,
        _ => {
          return Err(syn::Error::new(key.span(), "invoke_command unknown attribute; expected only: id, group, description, input, target"));
        }
      }
      if !input.is_empty() {
        input.parse::<Token![,]>()?;
      }
    }

    Ok(Self {
      id: required(id, "id", input)?,
      group: required(group, "group", input)?,
      description: required(description, "description", input)?,
      input: required(input_type, "input", input)?,
      target,
    })
  }
}

fn set_once<T>(slot: &mut Option<T>, value: T, key: &Ident) -> Result<()> {
  if slot.replace(value).is_some() {
    return Err(syn::Error::new(key.span(), format!("invoke_command duplicate `{key}` attribute")));
  }
  Ok(())
}

fn required<T>(value: Option<T>, name: &str, input: ParseStream<'_>) -> Result<T> {
  value.ok_or_else(|| input.error(format!("invoke_command missing required `{name}` attribute")))
}

fn expand(attributes: InvokeCommandAttributes, function: ItemFn) -> Result<proc_macro2::TokenStream> {
  let InvokeCommandAttributes {
    id,
    group,
    description,
    input,
    target,
  } = attributes;
  let target = target.unwrap_or_else(|| Ident::new("Forbidden", group.span()));
  let namespace = namespace(&group)?;
  let function_name = &function.sig.ident;
  let export_name = format_ident!("{function_name}_invoke_command");
  let handler_name = format_ident!("__{function_name}_invoke_handler");

  Ok(quote! {
    #function

    fn #handler_name(mut input: ::auv_cli_invoke::InvokeCommandInput) -> ::auv_cli_invoke::InvokeCommandFuture {
      Box::pin(async move {
        let args = ::auv_cli_invoke::command::decode_args::<#input>(&input)?;
        if input.typed_args.is_none() {
          input.inputs.extend(::auv_cli_invoke::command::encode_args(&args)?);
        }
        #function_name(input, args).await.map_err(::auv_cli_invoke::InvokeFailure::from)
      })
    }

    pub fn #export_name() -> ::auv_cli_invoke::InvokeCommand {
      ::auv_cli_invoke::command::typed_spec::<#input>(
        #id,
        ::auv_cli_invoke::InvokeNamespace::#namespace,
        #description,
        #handler_name,
      ).with_target(::auv_cli_invoke::command::TargetPolicy::#target)
    }
  })
}

fn namespace(group: &LitStr) -> Result<Ident> {
  let name = match group.value().as_str() {
    "display" => "Display",
    "screen" => "Screen",
    "window" => "Window",
    "input" => "Input",
    "app" => "App",
    "game" => "Game",
    "overlay" => "Overlay",
    "mediaControl" => "MediaControl",
    "fixture" => "Fixture",
    "scan" => "Scan",
    _ => {
      return Err(syn::Error::new(
        group.span(),
        "invoke_command unknown group; expected one of: display, screen, window, input, app, game, overlay, mediaControl, fixture, scan",
      ));
    }
  };
  Ok(Ident::new(name, group.span()))
}
