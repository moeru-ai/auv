use std::convert::Infallible;
use std::sync::Arc;

use auv_tracing::{
  CommitError, CommitResult, ErrorCode, IdempotencyKey, PageLimit, ReadError, RunCommit, RunCommitRequest, RunFact, RunId, RunMutation,
  RunRevision, SubscriptionError,
};
use auv_tracing_inspect::protocol::{
  ARTIFACT_ORIGIN_HEADER, AuthorityResponse, RUN_MEDIA_TYPE, RunApiError, RunCommitBody, RunStreamGap, decode_strict,
};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::rejection::{PathRejection, QueryRejection};
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
  Router::new()
    .route("/v1/authority", get(authority))
    .route("/v1/runs/{run_id}/commits", get(commits_after).post(commit))
    .route("/v1/runs/{run_id}/commits/by-idempotency-key/{key}", get(lookup_commit))
    .route("/v1/runs/{run_id}/snapshot", get(snapshot))
    .route("/v1/runs/{run_id}/extensions/{extension}", get(run_extension))
    .route("/v1/runs/{run_id}/commits/stream", get(commit_stream))
}

/// Returns the stable identity of the one store authority installed in server state.
async fn authority(State(state): State<Arc<InspectServerState>>) -> Response {
  let mut response = run_json(
    StatusCode::OK,
    &AuthorityResponse {
      authority_id: state.store.authority_id(),
    },
  );
  if let Some(origin) = &state.artifact_origin {
    response
      .headers_mut()
      .insert(ARTIFACT_ORIGIN_HEADER, HeaderValue::from_str(origin.as_str()).expect("validated artifact origin is a header value"));
  }
  response
}

/// Validates and appends one path-scoped ordinary run commit.
async fn commit(
  State(state): State<Arc<InspectServerState>>,
  path: Result<Path<String>, PathRejection>,
  headers: HeaderMap,
  request: Request,
) -> Result<Response, ApiFailure> {
  require_run_media_type(&headers)?;
  let Path(run_id) = path.map_err(|_| ApiFailure::invalid_reference())?;
  let run_id = parse_run_id(&run_id)?;
  let key = parse_idempotency_key(&headers)?;
  let bytes = to_bytes(request.into_body(), MAX_RUN_JSON_BYTES).await.map_err(ApiFailure::from_body)?;
  let body = decode_strict::<RunCommitBody>(&bytes).map_err(|_| ApiFailure::invalid_reference())?;
  let expected = state.store.authority_id();
  if body.authority_id != expected {
    return Err(ApiFailure::authority_mismatch(expected, body.authority_id));
  }
  let request =
    RunCommitRequest::new(body.authority_id, run_id, key, body.mutations.into_vec()).map_err(|_| ApiFailure::invalid_reference())?;
  let _mutation = state.mutation_arbitrator.acquire(run_id).await;
  if state.artifacts.reserves(run_id, key) {
    return Err(ApiFailure::from_commit(CommitError::IdempotencyMismatch));
  }
  let recovery_request = request.clone();
  match state.store.commit(request).await {
    Ok(CommitResult::Appended(commit)) => Ok(run_json(StatusCode::CREATED, &commit)),
    Ok(CommitResult::Replayed(commit)) => Ok(run_json(StatusCode::OK, &commit)),
    Err(CommitError::CommitUnknown(_)) => resolve_commit_unknown(&state, &recovery_request).await,
    Err(error) => Err(ApiFailure::from_commit(error)),
  }
}

async fn resolve_commit_unknown(state: &InspectServerState, request: &RunCommitRequest) -> Result<Response, ApiFailure> {
  match state.store.lookup_commit(request.run_id(), request.idempotency_key()).await {
    Ok(Some(commit)) if commit_matches_request(&commit, request) => Ok(run_json(StatusCode::OK, &commit)),
    Ok(Some(_)) => Err(ApiFailure::from_commit(CommitError::IdempotencyMismatch)),
    Ok(None) | Err(_) => Err(ApiFailure::commit_unknown()),
  }
}

fn commit_matches_request(commit: &RunCommit, request: &RunCommitRequest) -> bool {
  commit.authority_id() == request.authority_id()
    && commit.run_id() == request.run_id()
    && commit.idempotency_key() == request.idempotency_key()
    && commit.facts().len() == request.mutations().len()
    && commit.facts().iter().zip(request.mutations()).all(|(fact, mutation)| match (fact, mutation) {
      (RunFact::SpanStarted(left), RunMutation::StartSpan(right)) => left == right,
      (RunFact::SpanEnded(left), RunMutation::EndSpan(right)) => left == right,
      (RunFact::EventOccurred(left), RunMutation::EmitEvent(right)) => left == right,
      _ => false,
    })
}

/// Resolves an accepted commit without replaying application work.
async fn lookup_commit(
  State(state): State<Arc<InspectServerState>>,
  path: Result<Path<(String, String)>, PathRejection>,
) -> Result<Response, ApiFailure> {
  let Path((run_id, key)) = path.map_err(|_| ApiFailure::invalid_reference())?;
  let run_id = parse_run_id(&run_id)?;
  let key = key.parse::<IdempotencyKey>().map_err(|_| ApiFailure::invalid_reference())?;
  match state.store.lookup_commit(run_id, key).await.map_err(ApiFailure::from_read)? {
    Some(commit) => Ok(run_json(StatusCode::OK, &commit)),
    None => Err(ApiFailure::not_found()),
  }
}

/// Returns the canonical snapshot through its explicit revision cursor.
async fn snapshot(State(state): State<Arc<InspectServerState>>, path: Result<Path<String>, PathRejection>) -> Result<Response, ApiFailure> {
  let Path(run_id) = path.map_err(|_| ApiFailure::invalid_reference())?;
  let run_id = parse_run_id(&run_id)?;
  match state.store.load_snapshot(run_id).await.map_err(ApiFailure::from_read)? {
    Some(snapshot) => Ok(run_json(StatusCode::OK, &snapshot)),
    None => Err(ApiFailure::not_found()),
  }
}

/// Projects one named extension from the V1 server's canonical snapshot.
///
/// Triggering workflow:
/// `routes` -> `GET /v1/runs/{run_id}/extensions/{extension}`
/// -> `run_extension` -> `InspectRunExtension::project_json`
///
/// Upstream: `routes` installs this handler on the V1 run router.
/// Downstream: the installed projection reads through `InspectServerState::store`.
async fn run_extension(
  State(state): State<Arc<InspectServerState>>,
  path: Result<Path<(String, String)>, PathRejection>,
) -> Result<Response, ApiFailure> {
  let Path((run_id, extension)) = path.map_err(|_| ApiFailure::invalid_reference())?;
  let run_id = parse_run_id(&run_id)?;
  let snapshot = state.store.load_snapshot(run_id).await.map_err(ApiFailure::from_read)?.ok_or_else(ApiFailure::not_found)?;
  match state.extension.project_json(&extension, &state.store, &snapshot).await.map_err(|_| ApiFailure::extension_failed())? {
    Some(payload) => Ok(axum::Json(payload).into_response()),
    None => Err(ApiFailure::not_found()),
  }
}

/// Returns a bounded canonical commit page after the requested revision.
async fn commits_after(
  State(state): State<Arc<InspectServerState>>,
  path: Result<Path<String>, PathRejection>,
  query: Result<Query<CommitPageQuery>, QueryRejection>,
) -> Result<Response, ApiFailure> {
  let Path(run_id) = path.map_err(|_| ApiFailure::invalid_reference())?;
  let run_id = parse_run_id(&run_id)?;
  let Query(query) = query.map_err(|_| ApiFailure::invalid_reference())?;
  let page = state.store.commits_after(run_id, query.after_revision, query.limit).await.map_err(ApiFailure::from_read)?;
  Ok(run_json(StatusCode::OK, &page))
}

/// Streams ordered commits after the greater valid query/header cursor.
async fn commit_stream(
  State(state): State<Arc<InspectServerState>>,
  path: Result<Path<String>, PathRejection>,
  query: Result<Query<CommitStreamQuery>, QueryRejection>,
  headers: HeaderMap,
) -> Result<Response, ApiFailure> {
  let Path(run_id) = path.map_err(|_| ApiFailure::invalid_reference())?;
  let run_id = parse_run_id(&run_id)?;
  let Query(query) = query.map_err(|_| ApiFailure::invalid_reference())?;
  let after = greater_cursor(query.after_revision, optional_single_header(&headers, "Last-Event-ID")?);
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
      Some(Err(SubscriptionError::Store(error))) => {
        let failure = ApiFailure::from_read(error);
        let data = serde_json::to_string(&failure.body).expect("validated run error must encode as JSON");
        Some((Ok(Event::default().event("error").data(data)), (subscription, true)))
      }
      None => None,
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
  exactly_one_header(headers, "idempotency-key")?
    .to_str()
    .ok()
    .ok_or_else(ApiFailure::invalid_reference)?
    .parse()
    .map_err(|_| ApiFailure::invalid_reference())
}

fn require_run_media_type(headers: &HeaderMap) -> Result<(), ApiFailure> {
  let mut values = headers.get_all(CONTENT_TYPE).iter();
  let Some(value) = values.next() else {
    return Err(ApiFailure::unsupported_media_type());
  };
  if values.next().is_some() {
    return Err(ApiFailure::invalid_reference());
  }
  if value.to_str().ok() == Some(RUN_MEDIA_TYPE) {
    Ok(())
  } else {
    Err(ApiFailure::unsupported_media_type())
  }
}

fn exactly_one_header<'a>(headers: &'a HeaderMap, name: &'static str) -> Result<&'a HeaderValue, ApiFailure> {
  let mut values = headers.get_all(name).iter();
  let value = values.next().ok_or_else(ApiFailure::invalid_reference)?;
  if values.next().is_some() {
    return Err(ApiFailure::invalid_reference());
  }
  Ok(value)
}

fn optional_single_header<'a>(headers: &'a HeaderMap, name: &'static str) -> Result<Option<&'a HeaderValue>, ApiFailure> {
  let mut values = headers.get_all(name).iter();
  let value = values.next();
  if values.next().is_some() {
    return Err(ApiFailure::invalid_reference());
  }
  Ok(value)
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
  fn from_body(error: axum::Error) -> Self {
    if std::error::Error::source(&error).is_some_and(|source| source.is::<http_body_util::LengthLimitError>()) {
      Self::payload_too_large()
    } else {
      Self::invalid_reference()
    }
  }

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

  fn unsupported_media_type() -> Self {
    Self {
      status: StatusCode::UNSUPPORTED_MEDIA_TYPE,
      body: RunApiError::InvalidReference {
        code: ErrorCode::parse("auv.inspect.unsupported_media_type").expect("static error code"),
      },
    }
  }

  fn authority_mismatch(expected: auv_tracing::AuthorityId, received: auv_tracing::AuthorityId) -> Self {
    Self {
      status: StatusCode::CONFLICT,
      body: RunApiError::AuthorityMismatch { expected, received },
    }
  }

  fn unavailable(code: ErrorCode) -> Self {
    Self {
      status: StatusCode::SERVICE_UNAVAILABLE,
      body: RunApiError::Unavailable { code },
    }
  }

  fn commit_unknown() -> Self {
    Self::unavailable(ErrorCode::parse("auv.inspect.commit_unknown").expect("static error code"))
  }

  fn extension_failed() -> Self {
    Self {
      status: StatusCode::INTERNAL_SERVER_ERROR,
      body: RunApiError::Integrity {
        code: ErrorCode::parse("auv.inspect.extension_failed").expect("static error code"),
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
      CommitError::AuthorityMismatch { expected, received } => Self::authority_mismatch(expected, received),
      CommitError::IdempotencyMismatch => Self {
        status: StatusCode::CONFLICT,
        body: RunApiError::IdempotencyMismatch,
      },
      CommitError::Rejected(code) => Self {
        status: StatusCode::UNPROCESSABLE_ENTITY,
        body: RunApiError::Rejected { code },
      },
      CommitError::Unavailable(code) => Self::unavailable(code),
      CommitError::CommitUnknown(_) => Self::commit_unknown(),
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
