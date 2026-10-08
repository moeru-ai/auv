//! Serves `auv.api.annotations.v1.MethodDocsService` for one Runner.
//!
//! Docs are Markdown files named by a method's `presentation.name` (see
//! `annotations.proto`). The Runner's own descriptor set maps each served
//! gRPC path to that name, so a Runner answers only for methods it serves.

use std::collections::HashMap;

use auv_api_proto::auv::api::annotations::v1::method_docs_service_server::{MethodDocsService, MethodDocsServiceServer};
use auv_api_proto::auv::api::annotations::v1::{GetMethodDocsRequest, GetMethodDocsResponse, MethodExample};
use prost_reflect::{DescriptorPool, Value};
use tonic::{Request, Response, Status};

const PRESENTATION: &str = "auv.api.annotations.v1.presentation";

/// Builds the docs service for the methods of `services` (full names, as
/// served by this Runner), described by `encoded_file_descriptor_set`, from
/// `docs` as `(api name, markdown)` pairs such as `auv_api_proto::METHOD_DOCS`.
/// Docs that name no served method are ignored.
pub fn service(
  encoded_file_descriptor_set: &[u8],
  services: &[&str],
  docs: &[(&str, &str)],
) -> Result<MethodDocsServiceServer<Service>, String> {
  Service::new(encoded_file_descriptor_set, services, docs).map(MethodDocsServiceServer::new)
}

#[derive(Debug)]
pub struct Service {
  by_method: HashMap<String, GetMethodDocsResponse>,
}

impl Service {
  /// Maps each served method with a `presentation.name` to its parsed doc.
  /// A descriptor set also describes the services of the files it depends on,
  /// so the served services are named explicitly.
  pub fn new(encoded_file_descriptor_set: &[u8], services: &[&str], docs: &[(&str, &str)]) -> Result<Self, String> {
    let pool = DescriptorPool::decode(encoded_file_descriptor_set).map_err(|error| format!("invalid Runner descriptor set: {error}"))?;
    let docs = docs.iter().copied().collect::<HashMap<_, _>>();
    let mut by_method = HashMap::new();
    if let Some(extension) = pool.get_extension_by_name(PRESENTATION) {
      for name in services {
        let service = pool.get_service_by_name(name).ok_or_else(|| format!("unknown served service: {name}"))?;
        for method in service.methods() {
          let options = method.options();
          if !options.has_extension(&extension) {
            continue;
          }
          let Value::Message(presentation) = options.get_extension(&extension).into_owned() else {
            continue;
          };
          let name = presentation.get_field_by_name("name").and_then(|value| value.as_str().map(str::to_owned)).unwrap_or_default();
          if let Some(markdown) = docs.get(name.as_str()) {
            by_method.insert(format!("/{}/{}", service.full_name(), method.name()), parse(markdown));
          }
        }
      }
    }
    Ok(Self { by_method })
  }
}

#[tonic::async_trait]
impl MethodDocsService for Service {
  async fn get_method_docs(&self, request: Request<GetMethodDocsRequest>) -> Result<Response<GetMethodDocsResponse>, Status> {
    let method = request.into_inner().method;
    self.by_method.get(&method).cloned().map(Response::new).ok_or_else(|| Status::not_found(format!("no docs for {method}")))
  }
}

/// Splits a method doc into its Markdown and its examples: the fenced code
/// blocks of the `## Examples` section, each titled by the `###` heading
/// above it. The rest of the document, before and after that section, stays
/// Markdown.
fn parse(markdown: &str) -> GetMethodDocsResponse {
  let mut docs = Vec::new();
  let mut examples = Vec::new();
  let mut in_examples = false;
  let mut title = String::new();
  let mut fence: Option<MethodExample> = None;
  for line in markdown.lines() {
    if let Some(example) = fence.as_mut() {
      if line.trim_start().starts_with("```") {
        examples.push(fence.take().expect("open fence"));
      } else {
        if !example.code.is_empty() {
          example.code.push('\n');
        }
        example.code.push_str(line);
      }
      continue;
    }
    if let Some(heading) = line.strip_prefix("## ") {
      in_examples = heading.trim().eq_ignore_ascii_case("examples");
      if in_examples {
        continue;
      }
    }
    if !in_examples {
      docs.push(line);
      continue;
    }
    if let Some(heading) = line.strip_prefix("### ") {
      title = heading.trim().to_string();
    } else if let Some(language) = line.trim_start().strip_prefix("```") {
      fence = Some(MethodExample {
        title: title.clone(),
        language: language.trim().to_string(),
        code: String::new(),
      });
    }
  }
  GetMethodDocsResponse {
    markdown: docs.join("\n").trim().to_string(),
    examples,
  }
}

#[cfg(test)]
#[path = "method_docs_test.rs"]
mod tests;
