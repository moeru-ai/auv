use super::*;

use auv_api_proto::auv::api::annotations::v1::method_docs_service_server::MethodDocsService;

const DOC: &str = "# Find text\n\nCaptures a window.\n\n## Examples\n\n### Click it\n\n```ts\nconst found = await window.findText('Play')\nawait window.click(found.matches[0])\n```\n\n```sh\nauv invoke window.findText Play\n```\n\n## Errors\n\n`NOT_FOUND` when the window closed.\n";

#[test]
fn a_method_doc_splits_into_markdown_and_titled_examples() {
  let parsed = parse(DOC);
  assert_eq!(parsed.markdown, "# Find text\n\nCaptures a window.\n\n## Errors\n\n`NOT_FOUND` when the window closed.");
  assert_eq!(parsed.examples.len(), 2);
  assert_eq!(
    (parsed.examples[0].title.as_str(), parsed.examples[0].language.as_str(), parsed.examples[0].code.as_str()),
    ("Click it", "ts", "const found = await window.findText('Play')\nawait window.click(found.matches[0])")
  );
  assert_eq!((parsed.examples[1].title.as_str(), parsed.examples[1].language.as_str()), ("Click it", "sh"));
}

#[tokio::test]
async fn docs_are_served_by_grpc_path_for_methods_the_runner_serves() {
  // ROOT CAUSE:
  //
  // Docs are named by `presentation.name`, but clients only know the gRPC path
  // of the call they made. The Runner's own descriptor set maps one to the
  // other, so a Runner serves docs only for methods it serves.
  let served = ["auv.api.driver.v1.TextRecognitionService"];
  let descriptor_set = auv_api_proto::descriptor_set_for_services(&served).expect("descriptor set");
  let docs = Service::new(
    &descriptor_set,
    &served,
    &[
      ("window.find_text", DOC),
      ("windows.list", "# List windows"),
    ],
  )
  .expect("docs service");

  let found = docs
    .get_method_docs(Request::new(GetMethodDocsRequest {
      method: "/auv.api.driver.v1.TextRecognitionService/FindWindowText".to_string(),
    }))
    .await
    .expect("docs for a served method")
    .into_inner();
  assert!(found.markdown.starts_with("# Find text"));
  assert_eq!(found.examples.len(), 2);

  // WindowService is in the descriptor set (a dependency of text
  // recognition) but not served here, so its docs are not either.
  let missing = docs
    .get_method_docs(Request::new(GetMethodDocsRequest {
      method: "/auv.api.driver.v1.WindowService/ListWindows".to_string(),
    }))
    .await
    .expect_err("an unserved method has no docs");
  assert_eq!(missing.code(), tonic::Code::NotFound);
}

#[test]
fn every_embedded_doc_names_an_annotated_method() {
  // A doc file whose name no method's `presentation.name` matches is never
  // served; catch typos in file names here.
  let pool = DescriptorPool::decode(auv_api_proto::FILE_DESCRIPTOR_SET).expect("descriptor set");
  let extension = pool.get_extension_by_name(PRESENTATION).expect("presentation option");
  let names = pool
    .services()
    .flat_map(|service| service.methods().collect::<Vec<_>>())
    .filter_map(|method| match method.options().get_extension(&extension).into_owned() {
      Value::Message(presentation) => presentation.get_field_by_name("name").and_then(|value| value.as_str().map(str::to_owned)),
      _ => None,
    })
    .collect::<std::collections::HashSet<_>>();
  for (name, _) in auv_api_proto::METHOD_DOCS {
    assert!(names.contains(*name), "docs/{name}.md names no annotated method");
  }
  assert!(!auv_api_proto::METHOD_DOCS.is_empty());
}
