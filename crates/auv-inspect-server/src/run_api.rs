use std::convert::Infallible;
use std::sync::Arc;

use auv_tracing::{CommitError, ErrorCode, IdempotencyKey, PageLimit, ReadError, RunCommitRequest, RunId, RunRevision, SubscriptionError};
use auv_tracing_inspect::protocol::{AuthorityResponse, RUN_MEDIA_TYPE, RunApiError, RunCommitBody, RunStreamGap, decode_strict};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, Request, State};
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures_util::StreamExt;
use serde::Serialize;

use crate::server::InspectServerState;

const MAX_RUN_JSON_BYTES: usize = 32 * 1024 * 1024;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CommitPageQuery {
  after_revision: RunRevision,
  limit: PageLimit,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CommitStreamQuery {
  after_revision: RunRevision,
}

pub(crate) fn routes() -> Router<Arc<InspectServerState>> {
  // TODO(inspect-artifact-transfer-v1): Binary upload/read routes are deferred
  // to Task 12 and must retain their independent streaming body boundary.
  Router::new()
    .route("/v1/authority", get(authority))
    .route("/v1/runs/{run_id}/commits", get(commits_after).post(commit))
    .route("/v1/runs/{run_id}/commits/by-idempotency-key/{key}", get(lookup_commit))
    .route("/v1/runs/{run_id}/snapshot", get(snapshot))
    .route("/v1/runs/{run_id}/commits/stream", get(commit_stream))
}

/// Returns the stable identity of the one store authority installed in server state.
async fn authority(State(state): State<Arc<InspectServerState>>) -> Response {
  run_json(
    StatusCode::OK,
    &AuthorityResponse {
      authority_id: state.store.authority_id(),
    },
  )
}

/// Validates and appends one path-scoped ordinary run commit.
async fn commit(
  State(state): State<Arc<InspectServerState>>,
  Path(run_id): Path<String>,
  headers: HeaderMap,
  request: Request,
) -> Result<Response, ApiFailure> {
  require_run_media_type(&headers)?;
  let run_id = parse_run_id(&run_id)?;
  let key = parse_idempotency_key(&headers)?;
  let bytes = to_bytes(request.into_body(), MAX_RUN_JSON_BYTES).await.map_err(|_| ApiFailure::payload_too_large())?;
  let body = decode_strict::<RunCommitBody>(&bytes).map_err(|_| ApiFailure::invalid_reference())?;
  let request =
    RunCommitRequest::new(body.authority_id, run_id, key, body.mutations.into_vec()).map_err(|_| ApiFailure::invalid_reference())?;

  let _guard = state.commit_status_lock().await;
  let replay = state.store.lookup_commit(run_id, key).await.map_err(ApiFailure::from_read)?.is_some();
  let commit = state.store.commit(request).await.map_err(ApiFailure::from_commit)?;
  Ok(run_json(
    if replay {
      StatusCode::OK
    } else {
      StatusCode::CREATED
    },
    &commit,
  ))
}

/// Resolves an accepted commit without replaying application work.
async fn lookup_commit(
  State(state): State<Arc<InspectServerState>>,
  Path((run_id, key)): Path<(String, String)>,
) -> Result<Response, ApiFailure> {
  let run_id = parse_run_id(&run_id)?;
  let key = key.parse::<IdempotencyKey>().map_err(|_| ApiFailure::invalid_reference())?;
  match state.store.lookup_commit(run_id, key).await.map_err(ApiFailure::from_read)? {
    Some(commit) => Ok(run_json(StatusCode::OK, &commit)),
    None => Err(ApiFailure::not_found()),
  }
}

/// Returns the canonical snapshot through its explicit revision cursor.
async fn snapshot(State(state): State<Arc<InspectServerState>>, Path(run_id): Path<String>) -> Result<Response, ApiFailure> {
  let run_id = parse_run_id(&run_id)?;
  match state.store.load_snapshot(run_id).await.map_err(ApiFailure::from_read)? {
    Some(snapshot) => Ok(run_json(StatusCode::OK, &snapshot)),
    None => Err(ApiFailure::not_found()),
  }
}

/// Returns a bounded canonical commit page after the requested revision.
async fn commits_after(
  State(state): State<Arc<InspectServerState>>,
  Path(run_id): Path<String>,
  query: Result<Query<CommitPageQuery>, QueryRejection>,
) -> Result<Response, ApiFailure> {
  let run_id = parse_run_id(&run_id)?;
  let Query(query) = query.map_err(|_| ApiFailure::invalid_reference())?;
  let page = state.store.commits_after(run_id, query.after_revision, query.limit).await.map_err(ApiFailure::from_read)?;
  Ok(run_json(StatusCode::OK, &page))
}

/// Streams ordered commits after the greater valid query/header cursor.
async fn commit_stream(
  State(state): State<Arc<InspectServerState>>,
  Path(run_id): Path<String>,
  query: Result<Query<CommitStreamQuery>, QueryRejection>,
  headers: HeaderMap,
) -> Result<Response, ApiFailure> {
  let run_id = parse_run_id(&run_id)?;
  let Query(query) = query.map_err(|_| ApiFailure::invalid_reference())?;
  let after = greater_cursor(query.after_revision, headers.get("Last-Event-ID"));
  let subscription = state.store.subscribe(run_id, after).await.map_err(ApiFailure::from_read)?;
  let stream = futures_util::stream::unfold((subscription, false), |(mut subscription, closed)| async move {
    if closed {
      return None;
    }
    match subscription.next().await {
      Some(Ok(commit)) => {
        let revision = commit.revision();
        let data = serde_json::to_string(&commit).expect("validated commits must encode as JSON");
        let event = Event::default().id(revision.get().to_string()).event("commit").data(data);
        Some((Ok::<_, Infallible>(event), (subscription, false)))
      }
      Some(Err(SubscriptionError::Gap {
        requested_after,
        earliest_available,
      })) => {
        let data = serde_json::to_string(&RunStreamGap {
          requested_after,
          earliest_available,
        })
        .expect("validated gap must encode as JSON");
        Some((Ok(Event::default().event("gap").data(data)), (subscription, true)))
      }
      Some(Err(SubscriptionError::Store(_))) | None => None,
    }
  });
  Ok(Sse::new(stream).into_response())
}

fn greater_cursor(query: RunRevision, header: Option<&HeaderValue>) -> RunRevision {
  let Some(header) = header.and_then(|value| value.to_str().ok()).and_then(|value| value.parse::<u64>().ok()) else {
    return query;
  };
  RunRevision::new(header).ok().map_or(query, |header| header.max(query))
}

fn parse_run_id(value: &str) -> Result<RunId, ApiFailure> {
  value.parse().map_err(|_| ApiFailure::invalid_reference())
}

fn parse_idempotency_key(headers: &HeaderMap) -> Result<IdempotencyKey, ApiFailure> {
  headers
    .get("Idempotency-Key")
    .and_then(|value| value.to_str().ok())
    .ok_or_else(ApiFailure::invalid_reference)?
    .parse()
    .map_err(|_| ApiFailure::invalid_reference())
}

fn require_run_media_type(headers: &HeaderMap) -> Result<(), ApiFailure> {
  if headers.get(CONTENT_TYPE).and_then(|value| value.to_str().ok()) == Some(RUN_MEDIA_TYPE) {
    Ok(())
  } else {
    Err(ApiFailure::invalid_reference())
  }
}

fn run_json(status: StatusCode, value: &impl Serialize) -> Response {
  let bytes = serde_json::to_vec(value).expect("validated run protocol value must encode as JSON");
  let mut response = Response::new(Body::from(bytes));
  *response.status_mut() = status;
  response.headers_mut().insert(CONTENT_TYPE, HeaderValue::from_static(RUN_MEDIA_TYPE));
  response
}

struct ApiFailure {
  status: StatusCode,
  body: RunApiError,
}

impl ApiFailure {
  fn not_found() -> Self {
    Self {
      status: StatusCode::NOT_FOUND,
      body: RunApiError::NotFound,
    }
  }

  fn invalid_reference() -> Self {
    Self {
      status: StatusCode::BAD_REQUEST,
      body: RunApiError::InvalidReference {
        code: ErrorCode::parse("auv.inspect.invalid_reference").expect("static error code"),
      },
    }
  }

  fn payload_too_large() -> Self {
    Self {
      status: StatusCode::PAYLOAD_TOO_LARGE,
      body: RunApiError::InvalidReference {
        code: ErrorCode::parse("auv.inspect.run_json_too_large").expect("static error code"),
      },
    }
  }

  fn from_commit(error: CommitError) -> Self {
    match error {
      CommitError::AuthorityMismatch { expected, received } => Self {
        status: StatusCode::CONFLICT,
        body: RunApiError::AuthorityMismatch { expected, received },
      },
      CommitError::IdempotencyMismatch => Self {
        status: StatusCode::CONFLICT,
        body: RunApiError::IdempotencyMismatch,
      },
      CommitError::Rejected(code) => Self {
        status: StatusCode::UNPROCESSABLE_ENTITY,
        body: RunApiError::Rejected { code },
      },
      CommitError::Unavailable(code) | CommitError::CommitUnknown(code) => Self {
        status: StatusCode::SERVICE_UNAVAILABLE,
        body: RunApiError::Unavailable { code },
      },
    }
  }

  fn from_read(error: ReadError) -> Self {
    match error {
      ReadError::NotFound => Self::not_found(),
      ReadError::Forbidden => Self {
        status: StatusCode::FORBIDDEN,
        body: RunApiError::Forbidden,
      },
      ReadError::InvalidReference(code) => Self {
        status: StatusCode::BAD_REQUEST,
        body: RunApiError::InvalidReference { code },
      },
      ReadError::HistoryGap {
        requested_after,
        earliest_available,
      } => Self {
        status: StatusCode::GONE,
        body: RunApiError::HistoryGap {
          requested_after,
          earliest_available,
        },
      },
      ReadError::CursorAhead {
        requested_after,
        latest,
      } => Self {
        status: StatusCode::CONFLICT,
        body: RunApiError::CursorAhead {
          requested_after,
          latest,
        },
      },
      ReadError::Unavailable(code) => Self {
        status: StatusCode::SERVICE_UNAVAILABLE,
        body: RunApiError::Unavailable { code },
      },
      ReadError::Integrity(code) => Self {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        body: RunApiError::Integrity { code },
      },
    }
  }
}

impl IntoResponse for ApiFailure {
  fn into_response(self) -> Response {
    run_json(self.status, &self.body)
  }
}
