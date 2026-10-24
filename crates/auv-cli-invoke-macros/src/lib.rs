use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{Ident, ItemFn, LitStr, Result, Token, Type, braced, parenthesized, parse_macro_input};

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
  support: SupportAttribute,
  runner: RunnerAttribute,
  role: Option<RoleAttribute>,
}

impl Parse for InvokeCommandAttributes {
  fn parse(input: ParseStream<'_>) -> Result<Self> {
    let mut id = None;
    let mut group = None;
    let mut description = None;
    let mut input_type = None;
    let mut target = None;
    let mut support = None;
    let mut runner = None;
    let mut role = None;

    while !input.is_empty() {
      let key = input.parse::<Ident>()?;
      input.parse::<Token![=]>()?;
      match key.to_string().as_str() {
        "id" => set_once(&mut id, input.parse::<LitStr>()?, &key)?,
        "group" => set_once(&mut group, input.parse::<LitStr>()?, &key)?,
        "description" => set_once(&mut description, input.parse::<LitStr>()?, &key)?,
        "target" => set_once(&mut target, input.parse::<Ident>()?, &key)?,
        "input" => set_once(&mut input_type, input.parse::<Type>()?, &key)?,
        "support" => set_once(&mut support, input.parse::<SupportAttribute>()?, &key)?,
        "runner" => set_once(&mut runner, input.parse::<RunnerAttribute>()?, &key)?,
        "role" => set_once(&mut role, input.parse::<RoleAttribute>()?, &key)?,
        _ => {
          return Err(syn::Error::new(
            key.span(),
            "invoke_command unknown attribute; expected only: id, group, description, input, target, support, runner, role",
          ));
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
      support: required(support, "support", input)?,
      runner: required(runner, "runner", input)?,
      role,
    })
  }
}

/// `support = { macos: Supported, linux: Partial("foreground only"), windows: Unsupported }`
///
/// Every platform is required, so a command cannot omit one. A `Partial` cell must
/// name its limit, because "partial" alone does not tell a caller what is missing.
struct SupportAttribute {
  macos: SupportCell,
  linux: SupportCell,
  windows: SupportCell,
}

struct SupportCell {
  state: Ident,
  note: Option<LitStr>,
}

impl Parse for SupportAttribute {
  fn parse(input: ParseStream<'_>) -> Result<Self> {
    let span = input.span();
    let body;
    braced!(body in input);
    let (mut macos, mut linux, mut windows) = (None, None, None);
    while !body.is_empty() {
      let platform = body.parse::<Ident>()?;
      body.parse::<Token![:]>()?;
      let cell = body.parse::<SupportCell>()?;
      match platform.to_string().as_str() {
        "macos" => set_once(&mut macos, cell, &platform)?,
        "linux" => set_once(&mut linux, cell, &platform)?,
        "windows" => set_once(&mut windows, cell, &platform)?,
        _ => return Err(syn::Error::new(platform.span(), "invoke_command support names only: macos, linux, windows")),
      }
      if !body.is_empty() {
        body.parse::<Token![,]>()?;
      }
    }
    let missing = |name: &str| syn::Error::new(span, format!("invoke_command support is missing `{name}`; every platform is required"));
    Ok(Self {
      macos: macos.ok_or_else(|| missing("macos"))?,
      linux: linux.ok_or_else(|| missing("linux"))?,
      windows: windows.ok_or_else(|| missing("windows"))?,
    })
  }
}

impl Parse for SupportCell {
  fn parse(input: ParseStream<'_>) -> Result<Self> {
    let state = input.parse::<Ident>()?;
    if !matches!(state.to_string().as_str(), "Supported" | "Partial" | "Unsupported" | "NotEvidenced") {
      return Err(syn::Error::new(
        state.span(),
        "invoke_command support state must be one of: Supported, Partial, Unsupported, NotEvidenced",
      ));
    }
    let note = if input.peek(syn::token::Paren) {
      let inner;
      parenthesized!(inner in input);
      Some(inner.parse::<LitStr>()?)
    } else {
      None
    };
    if state == "Partial" && note.is_none() {
      return Err(syn::Error::new(
        state.span(),
        "invoke_command Partial support must name its limit, for example Partial(\"foreground only\")",
      ));
    }
    Ok(Self { state, note })
  }
}

/// `runner = Direct`, `runner = NoRoute`, or
/// `runner = { macos: Supported, linux: Partial("..."), windows: Unsupported }`.
///
/// The attribute is required, so a command cannot leave its Runner route
/// undeclared. `Direct` says the route's cells are the direct cells and is
/// allowed only where a source audit confirmed that equality; explicit cells
/// declare where the route differs from the direct path.
enum RunnerAttribute {
  Direct,
  NoRoute,
  Cells(SupportAttribute),
}

impl Parse for RunnerAttribute {
  fn parse(input: ParseStream<'_>) -> Result<Self> {
    if input.peek(syn::token::Brace) {
      return Ok(Self::Cells(input.parse::<SupportAttribute>()?));
    }
    let form = input.parse::<Ident>()?;
    match form.to_string().as_str() {
      "Direct" => Ok(Self::Direct),
      "NoRoute" => Ok(Self::NoRoute),
      _ => Err(syn::Error::new(
        form.span(),
        "invoke_command runner must be Direct, NoRoute, or per-platform cells such as { macos: Supported, linux: Supported, windows: Unsupported }",
      )),
    }
  }
}

impl RunnerAttribute {
  fn expand(&self) -> proc_macro2::TokenStream {
    match self {
      Self::Direct => quote! { ::auv_cli_invoke::support::RunnerSupport::Direct },
      Self::NoRoute => quote! { ::auv_cli_invoke::support::RunnerSupport::NoRoute },
      Self::Cells(cells) => {
        let cells = cells.expand();
        quote! { ::auv_cli_invoke::support::RunnerSupport::Cells(#cells) }
      }
    }
  }
}

/// `role = Alias("input.keys")` or `role = Fixture`. A command without one drives the desktop.
enum RoleAttribute {
  Alias(LitStr),
  Fixture,
}

impl Parse for RoleAttribute {
  fn parse(input: ParseStream<'_>) -> Result<Self> {
    let role = input.parse::<Ident>()?;
    match role.to_string().as_str() {
      "Alias" => {
        let inner;
        parenthesized!(inner in input);
        Ok(Self::Alias(inner.parse::<LitStr>()?))
      }
      "Fixture" => Ok(Self::Fixture),
      _ => Err(syn::Error::new(role.span(), "invoke_command role must be Alias(\"canonical.id\") or Fixture")),
    }
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
    support,
    runner,
    role,
  } = attributes;
  let target = target.unwrap_or_else(|| Ident::new("Forbidden", group.span()));
  let namespace = namespace(&group)?;
  let support = support.expand();
  let runner = runner.expand();
  let role = role.map(|role| role.expand());
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
        #support,
        #runner,
        #handler_name,
      ).with_target(::auv_cli_invoke::command::TargetPolicy::#target)#role
    }
  })
}

impl SupportAttribute {
  fn expand(&self) -> proc_macro2::TokenStream {
    let (macos, linux, windows) = (self.macos.expand(), self.linux.expand(), self.windows.expand());
    quote! {
      ::auv_cli_invoke::support::CommandSupport { macos: #macos, linux: #linux, windows: #windows }
    }
  }
}

impl SupportCell {
  fn expand(&self) -> proc_macro2::TokenStream {
    let state = &self.state;
    let note = match &self.note {
      Some(note) => quote! { ::core::option::Option::Some(#note) },
      None => quote! { ::core::option::Option::None },
    };
    quote! {
      ::auv_cli_invoke::support::PlatformSupport::new(::auv_cli_invoke::support::SupportState::#state, #note)
    }
  }
}

impl RoleAttribute {
  fn expand(&self) -> proc_macro2::TokenStream {
    let role = match self {
      Self::Alias(canonical) => quote! { ::auv_cli_invoke::support::CommandRole::Alias { canonical: #canonical } },
      Self::Fixture => quote! { ::auv_cli_invoke::support::CommandRole::Fixture },
    };
    quote! { .with_role(#role) }
  }
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
