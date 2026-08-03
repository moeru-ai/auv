use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::daemon::Daemon;
use crate::server::RequestAuth;

use super::router;

#[tokio::test]
async fn generic_invoke_rest_path_is_not_registered() {
  let store = tempfile::tempdir().expect("temporary API store");
  let daemon = Arc::new(Daemon::open(store.path()).expect("daemon"));
  let auth = RequestAuth::local(
    #[cfg(unix)]
    None,
    None,
  );
  let response =
    router(daemon, auth).oneshot(Request::post("/v1/operations:invoke").body(Body::empty()).expect("request")).await.expect("REST response");

  assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
