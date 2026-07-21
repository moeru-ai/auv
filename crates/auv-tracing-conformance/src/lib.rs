#![forbid(unsafe_code)]

//! Shared public-behavior assertions for every `RunStore` authority backend.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use auv_tracing::{
  ArtifactId, ArtifactPurpose, ArtifactReader, ArtifactWriteError, Attributes, AuthorityId, ByteLength, CommitError, ContentType, EventId,
  EventName, EventOccurred, EventSchema, IdempotencyKey, JsonPayload, PageLimit, ReadError, RunCommit, RunCommitPage, RunCommitRequest,
  RunFact, RunId, RunMutation, RunRevision, RunStore, RunSubscription, Sha256Digest, SpanEnded, SpanId, StoreArtifactRequest,
  SubscriptionError, Timestamp, reduce_commits,
};
use futures_io::AsyncRead;
use futures_timer::Delay;
use futures_util::future::{Either, select};
use futures_util::{StreamExt, pin_mut};
use serde::Serialize;
use sha2::{Digest, Sha256};

const CANONICAL_PAGE_BUDGET: usize = 32 * 1024 * 1024;
const CONTRACT_CASE_TIMEOUT: Duration = Duration::from_secs(30);

/// Runs the complete V1 store contract with a fresh run identity for each case.
///
/// The synchronous factory may return either a new backend or a shared backend
/// connection; run identity isolation is the behavior this harness guarantees.
pub async fn assert_store_contract(make: impl Fn() -> Arc<dyn RunStore>) {
  assert_case("authority_is_stable_and_non_nil", async {
    authority_is_stable_and_non_nil(make()).await;
  })
  .await;
  assert_case("authority_mismatch_precedes_invalid_mutation", async {
    authority_mismatch_precedes_invalid_mutation(make()).await;
  })
  .await;
  assert_case("authority_mismatch_does_not_poll_artifact_body", async {
    authority_mismatch_does_not_poll_artifact_body(make()).await;
  })
  .await;
  assert_case("revisions_start_at_one_and_remain_contiguous", async {
    revisions_start_at_one_and_remain_contiguous(make()).await;
  })
  .await;
  assert_case("equal_commit_replay_returns_the_original_commit", async {
    equal_commit_replay_returns_the_original_commit(make()).await;
  })
  .await;
  assert_case("mismatched_commit_replay_is_rejected", async {
    mismatched_commit_replay_is_rejected(make()).await;
  })
  .await;
  assert_case("lookup_resolves_an_already_committed_request", async {
    lookup_resolves_an_already_committed_request(make()).await;
  })
  .await;
  assert_case("event_ids_are_unique_across_idempotency_keys", async {
    event_ids_are_unique_across_idempotency_keys(make()).await;
  })
  .await;
  assert_case("snapshot_reduction_is_deterministic", async {
    snapshot_reduction_is_deterministic(make()).await;
  })
  .await;
  assert_case("page_cursors_cover_pagination_ahead_and_empty", async {
    page_cursors_cover_pagination_ahead_and_empty(make()).await;
  })
  .await;
  assert_case("pages_stop_at_the_canonical_byte_budget", async {
    pages_stop_at_the_canonical_byte_budget(make()).await;
  })
  .await;
  assert_case("equal_artifact_replay_does_not_poll_replacement_body", async {
    equal_artifact_replay_does_not_poll_replacement_body(make()).await;
  })
  .await;
  assert_case("artifact_id_conflict_cannot_replace_committed_bytes", async {
    artifact_id_conflict_cannot_replace_committed_bytes(make()).await;
  })
  .await;
  assert_case("artifact_length_and_digest_are_verified_before_publication", async {
    artifact_length_and_digest_are_verified_before_publication(make()).await;
  })
  .await;
  assert_case("interrupted_artifact_body_publishes_no_fact", async {
    interrupted_artifact_body_publishes_no_fact(make()).await;
  })
  .await;
  assert_case("open_artifact_returns_exact_committed_bytes", async {
    open_artifact_returns_exact_committed_bytes(make()).await;
  })
  .await;
  assert_case("subscription_resumes_after_cursor_without_a_snapshot_race", async {
    subscription_resumes_after_cursor_without_a_snapshot_race(make()).await;
  })
  .await;
}

/// Asserts matching page and subscription gaps after fixture-controlled retention.
///
/// The hook receives the fresh run identity and is awaited after three commits.
pub async fn assert_gap_contract<F, Fut>(store: Arc<dyn RunStore>, induce_retention_gap: F)
where
  F: FnOnce(RunId) -> Fut,
  Fut: Future<Output = ()>,
{
  assert_case("page_and_subscription_report_the_same_typed_gap", async {
    page_and_subscription_report_the_same_typed_gap(store, induce_retention_gap).await;
  })
  .await;
}

async fn assert_case(name: &'static str, case: impl Future<Output = ()>) {
  let timeout = Delay::new(CONTRACT_CASE_TIMEOUT);
  pin_mut!(case, timeout);
  // TODO(file-store-lock-timeout): This cooperative timeout cannot interrupt a synchronous OS lock wait; use child isolation once the shared harness has a deterministic process boundary.
  if matches!(select(case, timeout).await, Either::Right(_)) {
    panic!("RunStore conformance case `{name}` timed out after 30 seconds");
  }
}

async fn authority_is_stable_and_non_nil(store: Arc<dyn RunStore>) {
  let _run_id = RunId::new();
  let first = store.authority_id();
  let second = store.authority_id();
  assert_eq!(first, second, "authority identity changed between calls");
  assert!(!first.as_uuid().is_nil(), "authority identity must not be nil");
}

async fn authority_mismatch_precedes_invalid_mutation(store: Arc<dyn RunStore>) {
  let run_id = RunId::new();
  let received = distinct_authority(store.authority_id());
  let request = RunCommitRequest::new(
    received,
    run_id,
    IdempotencyKey::new(),
    vec![RunMutation::EndSpan(SpanEnded::new(
      SpanId::new(),
      timestamp(2),
    ))],
  )
  .expect("the request shape is valid even though its mutation cannot reduce");

  let error = store.commit(request).await.expect_err("wrong authority must be rejected first");
  assert_eq!(
    error,
    CommitError::AuthorityMismatch {
      expected: store.authority_id(),
      received,
    }
  );
}

async fn authority_mismatch_does_not_poll_artifact_body(store: Arc<dyn RunStore>) {
  let run_id = RunId::new();
  let received = distinct_authority(store.authority_id());
  let bytes = b"wrong authority body".to_vec();
  let request = artifact_request(received, run_id, IdempotencyKey::new(), ArtifactId::new(), &bytes);
  let probe = ProbeArtifactBody::complete(bytes);
  let polled = probe.polled();

  let error = store.write_artifact(request, Box::pin(probe)).await.expect_err("wrong authority must reject an artifact request");
  assert_eq!(
    error,
    ArtifactWriteError::AuthorityMismatch {
      expected: store.authority_id(),
      received,
    }
  );
  assert!(!polled.load(Ordering::SeqCst), "authority mismatch polled the one-shot body");
}

async fn revisions_start_at_one_and_remain_contiguous(store: Arc<dyn RunStore>) {
  let run_id = RunId::new();
  let first = commit_sample(&store, run_id, IdempotencyKey::new(), EventId::new(), "first").await;
  let second = commit_sample(&store, run_id, IdempotencyKey::new(), EventId::new(), "second").await;
  assert_eq!(first.revision(), revision(1));
  assert_eq!(second.revision(), revision(2));
}

async fn equal_commit_replay_returns_the_original_commit(store: Arc<dyn RunStore>) {
  let run_id = RunId::new();
  let request = sample_event_request(run_id, IdempotencyKey::new(), EventId::new(), "same").for_authority(store.authority_id());
  let first = store.commit(request.clone()).await.expect("initial commit must succeed");
  let replay = store.commit(request).await.expect("equal replay must succeed");
  assert_eq!(canonical_json(&replay), canonical_json(&first));

  let page = page(&store, run_id, revision(0), 1024).await;
  assert_eq!(page.commits().len(), 1, "equal replay appended another commit");
  assert_eq!(canonical_json(&page.commits()[0]), canonical_json(&first));
}

async fn mismatched_commit_replay_is_rejected(store: Arc<dyn RunStore>) {
  let run_id = RunId::new();
  let key = IdempotencyKey::new();
  store
    .commit(sample_event_request(run_id, key, EventId::new(), "first").for_authority(store.authority_id()))
    .await
    .expect("initial commit must succeed");

  let error = store
    .commit(sample_event_request(run_id, key, EventId::new(), "different").for_authority(store.authority_id()))
    .await
    .expect_err("a changed canonical request must not reuse an idempotency key");
  assert_eq!(error, CommitError::IdempotencyMismatch);
}

async fn lookup_resolves_an_already_committed_request(store: Arc<dyn RunStore>) {
  let run_id = RunId::new();
  let key = IdempotencyKey::new();
  let committed = store
    .commit(sample_event_request(run_id, key, EventId::new(), "lookup").for_authority(store.authority_id()))
    .await
    .expect("initial commit must succeed");

  let resolved = store.lookup_commit(run_id, key).await.expect("lookup must be readable").expect("accepted idempotency key must resolve");
  assert_eq!(canonical_json(&resolved), canonical_json(&committed));
}

async fn event_ids_are_unique_across_idempotency_keys(store: Arc<dyn RunStore>) {
  let run_id = RunId::new();
  let event_id = EventId::new();
  commit_sample(&store, run_id, IdempotencyKey::new(), event_id, "first").await;

  // Keep the payload equal so only the new idempotency key tests EventId uniqueness.
  let error = store
    .commit(sample_event_request(run_id, IdempotencyKey::new(), event_id, "first").for_authority(store.authority_id()))
    .await
    .expect_err("an event identity cannot be claimed by a new request");
  assert!(matches!(error, CommitError::Rejected(_)));
}

async fn snapshot_reduction_is_deterministic(store: Arc<dyn RunStore>) {
  let run_id = RunId::new();
  commit_sample(&store, run_id, IdempotencyKey::new(), EventId::new(), "first").await;
  commit_sample(&store, run_id, IdempotencyKey::new(), EventId::new(), "second").await;

  let stored = store.load_snapshot(run_id).await.expect("snapshot read must succeed").expect("committed run must have a snapshot");
  let commits = page(&store, run_id, revision(0), 1024).await;
  let reduced = reduce_commits(commits.commits()).expect("canonical committed history must reduce");
  assert_eq!(stored, reduced);
}

async fn page_cursors_cover_pagination_ahead_and_empty(store: Arc<dyn RunStore>) {
  let run_id = RunId::new();
  for value in ["one", "two", "three"] {
    commit_sample(&store, run_id, IdempotencyKey::new(), EventId::new(), value).await;
  }

  let first = page(&store, run_id, revision(0), 2).await;
  assert_eq!(revisions(&first), vec![1, 2]);
  assert_eq!(first.last_revision(), revision(2));
  assert!(first.has_more());

  let second = page(&store, run_id, first.last_revision(), 2).await;
  assert_eq!(revisions(&second), vec![3]);
  assert_eq!(second.last_revision(), revision(3));
  assert!(!second.has_more());

  let empty = page(&store, run_id, second.last_revision(), 2).await;
  assert!(empty.commits().is_empty());
  assert_eq!(empty.last_revision(), revision(3));
  assert!(!empty.has_more());

  let error = store.commits_after(run_id, revision(4), page_limit(2)).await.expect_err("a future cursor must not look like an empty page");
  assert_eq!(
    error,
    ReadError::CursorAhead {
      requested_after: revision(4),
      latest: revision(3),
    }
  );
}

async fn pages_stop_at_the_canonical_byte_budget(store: Arc<dyn RunStore>) {
  let run_id = RunId::new();
  let payload = "x".repeat(62 * 1024);
  for _ in 0..3 {
    commit_sample_batch(&store, run_id, 180, &payload).await;
  }

  let first = page(&store, run_id, revision(0), 1024).await;
  assert!(!first.commits().is_empty(), "a valid first commit must always fit");
  assert!(first.has_more(), "history larger than 32 MiB must require another page");
  assert!(canonical_json(&first).len() <= CANONICAL_PAGE_BUDGET);
  assert_contiguous(first.commits());

  let second = page(&store, run_id, first.last_revision(), 1024).await;
  assert!(!second.commits().is_empty(), "the continuation cursor must make progress");
  assert_eq!(second.commits()[0].revision().get(), first.last_revision().get() + 1);

  let mut over_budget = first.commits().to_vec();
  over_budget.push(second.commits()[0].clone());
  assert!(
    canonical_page_json(&over_budget, second.commits()[0].revision(), true).len() > CANONICAL_PAGE_BUDGET,
    "the first page stopped before the canonical byte budget required it"
  );
}

async fn equal_artifact_replay_does_not_poll_replacement_body(store: Arc<dyn RunStore>) {
  let run_id = RunId::new();
  let bytes = b"canonical artifact".to_vec();
  let request = artifact_request(store.authority_id(), run_id, IdempotencyKey::new(), ArtifactId::new(), &bytes);
  let first = store
    .write_artifact(request.clone(), Box::pin(ProbeArtifactBody::complete(bytes)))
    .await
    .expect("initial artifact publication must succeed");

  let replacement = ProbeArtifactBody::complete(b"must not be consumed".to_vec());
  let polled = replacement.polled();
  let replay = store.write_artifact(request, Box::pin(replacement)).await.expect("equal artifact replay must return the first commit");
  assert_eq!(canonical_json(&replay), canonical_json(&first));
  assert!(!polled.load(Ordering::SeqCst), "equal replay polled its replacement body");

  let page = page(&store, run_id, revision(0), 1024).await;
  assert_eq!(page.commits().len(), 1, "equal artifact replay appended another fact");
}

async fn artifact_id_conflict_cannot_replace_committed_bytes(store: Arc<dyn RunStore>) {
  let run_id = RunId::new();
  let artifact_id = ArtifactId::new();
  let key = IdempotencyKey::new();
  let original = b"original artifact bytes".to_vec();
  let first_request = artifact_request(store.authority_id(), run_id, key, artifact_id, &original);
  store
    .write_artifact(first_request.clone(), Box::pin(ProbeArtifactBody::complete(original.clone())))
    .await
    .expect("initial artifact publication must succeed");

  let changed = b"changed artifact bytes".to_vec();
  let changed_request = artifact_request(store.authority_id(), run_id, key, artifact_id, &changed);
  assert_eq!(
    store
      .write_artifact(changed_request, Box::pin(ProbeArtifactBody::complete(changed)))
      .await
      .expect_err("same key with changed metadata must fail"),
    ArtifactWriteError::IdempotencyMismatch
  );

  let conflicting_key = artifact_request(store.authority_id(), run_id, IdempotencyKey::new(), artifact_id, &original);
  assert!(matches!(
    store.write_artifact(conflicting_key, Box::pin(ProbeArtifactBody::complete(original.clone()))).await,
    Err(ArtifactWriteError::Rejected(_))
  ));

  let reader =
    store.open_artifact(auv_tracing::ArtifactUri::from_ids(run_id, artifact_id)).await.expect("original artifact must remain readable");
  assert_eq!(collect_artifact(reader).await, original);
}

async fn artifact_length_and_digest_are_verified_before_publication(store: Arc<dyn RunStore>) {
  let run_id = RunId::new();
  let bytes = b"integrity checked bytes".to_vec();

  let length_request = StoreArtifactRequest::new(
    store.authority_id(),
    run_id,
    IdempotencyKey::new(),
    ArtifactId::new(),
    None,
    artifact_purpose(),
    content_type(),
    ByteLength::new(bytes.len() as u64 + 1).expect("test length is bounded"),
    digest(&bytes),
    Attributes::empty(),
  );
  assert!(matches!(
    store.write_artifact(length_request, Box::pin(ProbeArtifactBody::complete(bytes.clone()))).await,
    Err(ArtifactWriteError::Integrity(_))
  ));

  let digest_request = StoreArtifactRequest::new(
    store.authority_id(),
    run_id,
    IdempotencyKey::new(),
    ArtifactId::new(),
    None,
    artifact_purpose(),
    content_type(),
    ByteLength::new(bytes.len() as u64).expect("test length is bounded"),
    digest(b"different bytes"),
    Attributes::empty(),
  );
  assert!(matches!(
    store.write_artifact(digest_request, Box::pin(ProbeArtifactBody::complete(bytes))).await,
    Err(ArtifactWriteError::Integrity(_))
  ));

  assert_eq!(store.load_snapshot(run_id).await.expect("snapshot read must succeed"), None);
}

async fn interrupted_artifact_body_publishes_no_fact(store: Arc<dyn RunStore>) {
  let run_id = RunId::new();
  let artifact_id = ArtifactId::new();
  let key = IdempotencyKey::new();
  let bytes = b"body interrupted after a prefix".to_vec();
  let request = artifact_request(store.authority_id(), run_id, key, artifact_id, &bytes);

  assert!(matches!(
    store.write_artifact(request, Box::pin(ProbeArtifactBody::interrupted(bytes, 5))).await,
    Err(ArtifactWriteError::Unavailable(_))
  ));
  assert_eq!(store.lookup_commit(run_id, key).await.expect("lookup after failed upload must succeed"), None);
  assert_eq!(store.load_snapshot(run_id).await.expect("snapshot read must succeed"), None);
  assert!(matches!(store.open_artifact(auv_tracing::ArtifactUri::from_ids(run_id, artifact_id)).await, Err(ReadError::NotFound)));
}

async fn open_artifact_returns_exact_committed_bytes(store: Arc<dyn RunStore>) {
  let run_id = RunId::new();
  let artifact_id = ArtifactId::new();
  let bytes = b"exact committed artifact bytes".to_vec();
  let commit = store
    .write_artifact(
      artifact_request(store.authority_id(), run_id, IdempotencyKey::new(), artifact_id, &bytes),
      Box::pin(ProbeArtifactBody::complete(bytes.clone())),
    )
    .await
    .expect("artifact publication must succeed");
  let metadata = commit
    .facts()
    .iter()
    .find_map(|fact| match fact {
      RunFact::ArtifactPublished(artifact) => Some(artifact.metadata()),
      _ => None,
    })
    .expect("artifact write commit must publish artifact metadata");
  assert_eq!(metadata.byte_length(), ByteLength::new(bytes.len() as u64).unwrap());
  assert_eq!(metadata.sha256(), digest(&bytes));

  let reader = store.open_artifact(metadata.uri().clone()).await.expect("committed artifact must be readable");
  let collected = collect_artifact(reader).await;
  assert_eq!(collected, bytes);
  assert_eq!(ByteLength::new(collected.len() as u64).unwrap(), metadata.byte_length());
  assert_eq!(digest(&collected), metadata.sha256());
}

async fn subscription_resumes_after_cursor_without_a_snapshot_race(store: Arc<dyn RunStore>) {
  let run_id = RunId::new();
  commit_sample(&store, run_id, IdempotencyKey::new(), EventId::new(), "snapshot").await;
  let snapshot = store.load_snapshot(run_id).await.expect("snapshot read must succeed").expect("committed run must have a snapshot");
  let mut subscription =
    store.subscribe(run_id, snapshot.through_revision()).await.expect("subscription must start after the snapshot cursor");

  let second = commit_sample(&store, run_id, IdempotencyKey::new(), EventId::new(), "second").await;
  let received_second = next_subscription(&mut subscription).await.expect("subscription item must be a commit");
  assert_eq!(canonical_json(&received_second), canonical_json(&second));

  let third = commit_sample(&store, run_id, IdempotencyKey::new(), EventId::new(), "third").await;
  let received_third = next_subscription(&mut subscription).await.expect("subscription item must be a commit");
  assert_eq!(canonical_json(&received_third), canonical_json(&third));
}

async fn page_and_subscription_report_the_same_typed_gap<F, Fut>(store: Arc<dyn RunStore>, induce_retention_gap: F)
where
  F: FnOnce(RunId) -> Fut,
  Fut: Future<Output = ()>,
{
  let run_id = RunId::new();
  for value in ["one", "two", "three"] {
    commit_sample(&store, run_id, IdempotencyKey::new(), EventId::new(), value).await;
  }
  induce_retention_gap(run_id).await;

  let requested_after = revision(0);
  let page_error =
    store.commits_after(run_id, requested_after, page_limit(1024)).await.expect_err("retained history must report a page gap");
  let ReadError::HistoryGap {
    requested_after: page_requested,
    earliest_available,
  } = page_error
  else {
    panic!("expected HistoryGap, got {page_error:?}");
  };
  assert_eq!(page_requested, requested_after);

  let mut subscription = store.subscribe(run_id, requested_after).await.expect("a retained-history gap is delivered as a subscription item");
  let error = next_subscription(&mut subscription).await.expect_err("subscription must report the retained-history gap");
  assert_eq!(
    error,
    SubscriptionError::Gap {
      requested_after,
      earliest_available,
    }
  );
}

struct SampleEventRequest {
  run_id: RunId,
  key: IdempotencyKey,
  event_id: EventId,
  payload: JsonPayload,
}

impl SampleEventRequest {
  fn for_authority(self, authority_id: AuthorityId) -> RunCommitRequest {
    let schema =
      EventSchema::new(EventName::parse("auv.test.event").expect("sample event name is valid"), 1).expect("sample event schema is valid");
    let event = EventOccurred::new(self.event_id, None, timestamp(1), schema, self.payload);
    RunCommitRequest::new(authority_id, self.run_id, self.key, vec![RunMutation::EmitEvent(event)]).expect("sample event request is valid")
  }
}

fn sample_event_request(run_id: RunId, key: IdempotencyKey, event_id: EventId, value: impl Serialize) -> SampleEventRequest {
  SampleEventRequest {
    run_id,
    key,
    event_id,
    payload: JsonPayload::encode(&serde_json::json!({ "value": value })).expect("sample event payload is valid"),
  }
}

/// Builds the canonical single-event request used by backend conformance tests.
pub fn event_request(
  authority_id: AuthorityId,
  run_id: RunId,
  event_id: EventId,
  key: IdempotencyKey,
  value: impl Serialize,
) -> RunCommitRequest {
  sample_event_request(run_id, key, event_id, value).for_authority(authority_id)
}

async fn commit_sample(
  store: &Arc<dyn RunStore>,
  run_id: RunId,
  key: IdempotencyKey,
  event_id: EventId,
  value: impl Serialize,
) -> RunCommit {
  store
    .commit(sample_event_request(run_id, key, event_id, value).for_authority(store.authority_id()))
    .await
    .expect("sample event commit must succeed")
}

async fn commit_sample_batch(store: &Arc<dyn RunStore>, run_id: RunId, event_count: usize, value: &str) -> RunCommit {
  let schema =
    EventSchema::new(EventName::parse("auv.test.event").expect("sample event name is valid"), 1).expect("sample event schema is valid");
  let payload = JsonPayload::encode(&serde_json::json!({ "value": value })).expect("sample event payload is valid");
  let mutations = (0..event_count)
    .map(|_| RunMutation::EmitEvent(EventOccurred::new(EventId::new(), None, timestamp(1), schema.clone(), payload.clone())))
    .collect();
  let request =
    RunCommitRequest::new(store.authority_id(), run_id, IdempotencyKey::new(), mutations).expect("sample event batch request is valid");
  store.commit(request).await.expect("sample event batch commit must succeed")
}

/// Builds the canonical binary artifact request used by backend conformance tests.
pub fn artifact_request(
  authority_id: AuthorityId,
  run_id: RunId,
  key: IdempotencyKey,
  artifact_id: ArtifactId,
  bytes: &[u8],
) -> StoreArtifactRequest {
  artifact_request_with_span(authority_id, run_id, key, artifact_id, None, bytes)
}

/// Builds a span-associated binary artifact request for process conformance tests.
pub fn artifact_request_with_span(
  authority_id: AuthorityId,
  run_id: RunId,
  key: IdempotencyKey,
  artifact_id: ArtifactId,
  span_id: Option<SpanId>,
  bytes: &[u8],
) -> StoreArtifactRequest {
  StoreArtifactRequest::new(
    authority_id,
    run_id,
    key,
    artifact_id,
    span_id,
    artifact_purpose(),
    content_type(),
    ByteLength::new(bytes.len() as u64).expect("sample artifact length is bounded"),
    digest(bytes),
    Attributes::empty(),
  )
}

fn artifact_purpose() -> ArtifactPurpose {
  ArtifactPurpose::parse("auv.test.artifact").expect("sample purpose is valid")
}

fn content_type() -> ContentType {
  ContentType::parse("application/octet-stream").expect("sample content type is valid")
}

fn digest(bytes: &[u8]) -> Sha256Digest {
  Sha256Digest::new(Sha256::digest(bytes).into())
}

fn timestamp(unix_seconds: i64) -> Timestamp {
  Timestamp::new(unix_seconds, 0).expect("sample timestamp is valid")
}

fn revision(value: u64) -> RunRevision {
  RunRevision::new(value).expect("sample revision is valid")
}

fn page_limit(value: u32) -> PageLimit {
  PageLimit::new(value).expect("sample page limit is valid")
}

async fn page(store: &Arc<dyn RunStore>, run_id: RunId, after: RunRevision, limit: u32) -> RunCommitPage {
  store.commits_after(run_id, after, page_limit(limit)).await.expect("commit page read must succeed")
}

fn revisions(page: &RunCommitPage) -> Vec<u64> {
  page.commits().iter().map(|commit| commit.revision().get()).collect()
}

fn assert_contiguous(commits: &[RunCommit]) {
  for pair in commits.windows(2) {
    assert_eq!(pair[1].revision().get(), pair[0].revision().get() + 1);
  }
}

fn canonical_json(value: &impl Serialize) -> Vec<u8> {
  serde_json::to_vec(value).expect("contract value must have canonical compact JSON")
}

fn canonical_page_json(commits: &[RunCommit], last_revision: RunRevision, has_more: bool) -> Vec<u8> {
  #[derive(Serialize)]
  struct Page<'a> {
    commits: &'a [RunCommit],
    last_revision: RunRevision,
    has_more: bool,
  }

  canonical_json(&Page {
    commits,
    last_revision,
    has_more,
  })
}

async fn collect_artifact(mut reader: ArtifactReader) -> Vec<u8> {
  let mut bytes = Vec::new();
  while let Some(chunk) = reader.next().await {
    bytes.extend_from_slice(&chunk.expect("artifact stream must remain readable"));
  }
  bytes
}

async fn next_subscription(subscription: &mut RunSubscription) -> Result<RunCommit, SubscriptionError> {
  let next = subscription.next();
  let timeout = Delay::new(Duration::from_secs(5));
  pin_mut!(next, timeout);
  match select(next, timeout).await {
    Either::Left((Some(item), _)) => item,
    Either::Left((None, _)) => panic!("subscription ended before producing the required item"),
    Either::Right(_) => panic!("subscription did not produce the required item within five seconds"),
  }
}

fn distinct_authority(authority_id: AuthorityId) -> AuthorityId {
  loop {
    let candidate = AuthorityId::new();
    if candidate != authority_id {
      return candidate;
    }
  }
}

struct ProbeArtifactBody {
  bytes: Vec<u8>,
  offset: usize,
  interrupt_after: Option<usize>,
  polled: Arc<AtomicBool>,
}

impl ProbeArtifactBody {
  fn complete(bytes: Vec<u8>) -> Self {
    Self {
      bytes,
      offset: 0,
      interrupt_after: None,
      polled: Arc::new(AtomicBool::new(false)),
    }
  }

  fn interrupted(bytes: Vec<u8>, interrupt_after: usize) -> Self {
    Self {
      bytes,
      offset: 0,
      interrupt_after: Some(interrupt_after),
      polled: Arc::new(AtomicBool::new(false)),
    }
  }

  fn polled(&self) -> Arc<AtomicBool> {
    Arc::clone(&self.polled)
  }
}

impl AsyncRead for ProbeArtifactBody {
  fn poll_read(mut self: Pin<&mut Self>, _cx: &mut Context<'_>, buffer: &mut [u8]) -> Poll<io::Result<usize>> {
    self.polled.store(true, Ordering::SeqCst);
    if self.interrupt_after.is_some_and(|limit| self.offset >= limit) {
      return Poll::Ready(Err(io::Error::new(io::ErrorKind::ConnectionReset, "injected artifact body interruption")));
    }

    let readable_end = self.interrupt_after.unwrap_or(self.bytes.len()).min(self.bytes.len());
    let count = buffer.len().min(readable_end.saturating_sub(self.offset));
    if count == 0 {
      return Poll::Ready(Ok(0));
    }
    buffer[..count].copy_from_slice(&self.bytes[self.offset..self.offset + count]);
    self.offset += count;
    Poll::Ready(Ok(count))
  }
}
