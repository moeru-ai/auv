use quote::quote;
use syn::{Data, DeriveInput, Error, Expr, Fields, LitStr};

pub(crate) fn expand(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
  let name = input.ident;
  let fields = match input.data {
    Data::Struct(data) => match data.fields {
      Fields::Named(fields) => fields.named,
      _ => return Err(Error::new_spanned(name, "TableRow requires a struct with named fields")),
    },
    _ => return Err(Error::new_spanned(name, "TableRow can only be derived for structs")),
  };

  let mut columns = Vec::new();
  let mut cells = Vec::new();
  for field in fields {
    let ident = field.ident.expect("named fields have identifiers");
    let mut header = inferred_header(&ident.to_string());
    let mut hidden = false;
    let mut wide = false;
    let mut display_with: Option<Expr> = None;
    let mut display_zero: Option<LitStr> = None;

    for attribute in field.attrs.iter().filter(|attribute| attribute.path().is_ident("table")) {
      attribute.parse_nested_meta(|meta| {
        if meta.path.is_ident("hidden") {
          hidden = true;
          return Ok(());
        }
        if meta.path.is_ident("wide") {
          wide = true;
          return Ok(());
        }
        if meta.path.is_ident("header") {
          header = meta.value()?.parse::<LitStr>()?.value();
          return Ok(());
        }
        if meta.path.is_ident("display_with") {
          let input = meta.value()?;
          display_with = Some(if input.peek(LitStr) {
            input.parse::<LitStr>()?.parse()?
          } else {
            input.parse()?
          });
          return Ok(());
        }
        if meta.path.is_ident("display_zero") {
          display_zero = Some(meta.value()?.parse()?);
          return Ok(());
        }
        Err(meta.error("unknown table attribute; expected header, hidden, wide, display_with, or display_zero"))
      })?;
    }

    if hidden {
      continue;
    }

    let condition = if wide {
      quote!(options.wide)
    } else {
      quote!(true)
    };
    columns.push(quote! {
      if #condition {
        columns.push(::auv_cli_common::outputs::formats::table::Column { header: #header });
      }
    });

    let formatted = match display_with {
      Some(formatter) => quote!(::std::string::ToString::to_string(&(#formatter)(&self.#ident))),
      None => quote!(::auv_cli_common::outputs::formats::table::TableValue::table_value(&self.#ident)),
    };
    let value = match display_zero {
      Some(display_zero) => quote! {
        if ::auv_cli_common::outputs::formats::table::TableValue::is_zero(&self.#ident) {
          ::std::string::String::from(#display_zero)
        } else {
          #formatted
        }
      },
      None => formatted,
    };
    cells.push(quote! {
      if #condition {
        cells.push(#value);
      }
    });
  }

  let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
  Ok(quote! {
    impl #impl_generics ::auv_cli_common::outputs::formats::table::TableRow for #name #type_generics #where_clause {
      fn columns(options: ::auv_cli_common::outputs::formats::table::TableOptions<'_>) -> ::std::vec::Vec<::auv_cli_common::outputs::formats::table::Column> {
        let mut columns = ::std::vec::Vec::new();
        #(#columns)*
        columns
      }

      fn cells(&self, options: ::auv_cli_common::outputs::formats::table::TableOptions<'_>) -> ::std::vec::Vec<::std::string::String> {
        let mut cells = ::std::vec::Vec::new();
        #(#cells)*
        cells
      }
    }
  })
}

fn inferred_header(field_name: &str) -> String {
  field_name.replace('_', " ").to_ascii_uppercase()
}

#[cfg(test)]
#[path = "table_row_test.rs"]
mod tests;
