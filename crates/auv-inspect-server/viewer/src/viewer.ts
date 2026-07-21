interface Timestamp {
  unix_seconds: number;
  nanoseconds: number;
}

interface SpanStarted {
  span_id: string;
  parent_span_id: string | null;
  remote_link: { span_id: string } | null;
  name: string;
  started_at: Timestamp;
  attributes: Record<string, boolean | number | string>;
}

interface SpanEnded {
  span_id: string;
  ended_at: Timestamp;
}

interface SpanSnapshot {
  started: SpanStarted;
  ended: SpanEnded | null;
}

interface EventOccurred {
  event_id: string;
  span_id: string | null;
  occurred_at: Timestamp;
  schema: { name: string; version: number };
  payload: unknown;
}

interface ArtifactPublished {
  span_id: string | null;
  metadata: {
    uri: string;
    purpose: string;
    content_type: string;
    byte_length: number;
    sha256: string;
    attributes: Record<string, boolean | number | string>;
  };
}

type RunFact =
  | { span_started: SpanStarted }
  | { span_ended: SpanEnded }
  | { event_occurred: EventOccurred }
  | { artifact_published: ArtifactPublished };

interface RunCommit {
  authority_id: string;
  run_id: string;
  revision: number;
  idempotency_key: string;
  committed_at: Timestamp;
  facts: RunFact[];
}

interface RunSnapshot {
  authority_id: string;
  run_id: string;
  through_revision: number;
  spans: Record<string, SpanSnapshot>;
  events: EventOccurred[];
  artifacts: Record<string, ArtifactPublished>;
}

interface ViewerState {
  runId: string | null;
  snapshot: RunSnapshot | null;
  source: EventSource | null;
  generation: number;
  retryTimer: number | null;
  stabilityTimer: number | null;
  recoveryAttempts: number;
}

interface ErrorState {
  message: string;
  transient: boolean;
}

const RUN_MEDIA_TYPE = "application/vnd.auv.run+json; version=1";
const MAX_RECOVERY_ATTEMPTS = 5;
const BASE_RETRY_MILLIS = 250;
const MAX_RETRY_MILLIS = 4_000;
// NOTICE(inspect-sse-stability-v1): Keep this below the first delayed retry so
// rapid open/error flapping consumes budget without slowing stable recovery.
const CONNECTION_STABILITY_MILLIS = 100;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const ARTIFACT_URI = /^auv:\/\/runs\/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})\/artifacts\/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$/;

function snapshotEndpoint(runId: string): string {
  return `/v1/runs/${encodeURIComponent(runId)}/snapshot`;
}

function streamEndpoint(runId: string, afterRevision: number): string {
  return `/v1/runs/${encodeURIComponent(runId)}/commits/stream?after_revision=${afterRevision}`;
}

function element<T extends HTMLElement>(document: Document, id: string): T {
  const value = document.getElementById(id);
  if (value === null) throw new Error(`viewer element #${id} is missing`);
  return value as T;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasExactKeys(value: Record<string, unknown>, expected: readonly string[]): boolean {
  const keys = Object.keys(value).sort();
  const required = [...expected].sort();
  return keys.length === required.length && keys.every((key, index) => key === required[index]);
}

function isUuid(value: unknown): value is string {
  return typeof value === "string" && UUID.test(value);
}

function isRevision(value: unknown): value is number {
  return Number.isSafeInteger(value) && Number(value) >= 0;
}

function isTimestamp(value: unknown): value is Timestamp {
  return isRecord(value)
    && hasExactKeys(value, ["unix_seconds", "nanoseconds"])
    && Number.isSafeInteger(value.unix_seconds)
    && Number.isSafeInteger(value.nanoseconds)
    && Number(value.nanoseconds) >= 0
    && Number(value.nanoseconds) < 1_000_000_000;
}

function compareTimestamp(left: Timestamp, right: Timestamp): number {
  if (left.unix_seconds !== right.unix_seconds) return left.unix_seconds < right.unix_seconds ? -1 : 1;
  if (left.nanoseconds !== right.nanoseconds) return left.nanoseconds < right.nanoseconds ? -1 : 1;
  return 0;
}

function artifactRunId(uri: string): string | null {
  const match = ARTIFACT_URI.exec(uri);
  return match === null ? null : match[1];
}

function hasValidSpanLinks(started: SpanStarted): boolean {
  const remoteSpanId = started.remote_link?.span_id ?? null;
  return started.parent_span_id !== started.span_id
    && remoteSpanId !== started.span_id
    && (started.parent_span_id === null || started.parent_span_id !== remoteSpanId);
}

function hasValidParentGraph(spans: Record<string, SpanSnapshot>): boolean {
  const states = new Map<string, "visiting" | "done">();
  for (const root of Object.keys(spans)) {
    if (states.get(root) === "done") continue;
    const path: string[] = [];
    let current: string | null = root;
    while (current !== null) {
      const state = states.get(current);
      if (state === "done") break;
      if (state === "visiting") return false;
      const span: SpanSnapshot | undefined = spans[current];
      if (span === undefined) return false;
      states.set(current, "visiting");
      path.push(current);
      current = span.started.parent_span_id;
    }
    for (const spanId of path) states.set(spanId, "done");
  }
  return true;
}

function recordMaxTimestamp(index: Map<string, Timestamp>, spanId: string, timestamp: Timestamp): void {
  const current = index.get(spanId);
  if (current === undefined || compareTimestamp(timestamp, current) > 0) index.set(spanId, timestamp);
}

function isAttributes(value: unknown): value is Record<string, boolean | number | string> {
  return isRecord(value)
    && Object.values(value).every((entry) => typeof entry === "boolean" || typeof entry === "string" || (typeof entry === "number" && Number.isFinite(entry)));
}

function isNullableUuid(value: unknown): value is string | null {
  return value === null || isUuid(value);
}

function isSpanStarted(value: unknown): value is SpanStarted {
  if (!isRecord(value) || !hasExactKeys(value, ["span_id", "parent_span_id", "remote_link", "name", "started_at", "attributes"])) return false;
  const link = value.remote_link;
  return isUuid(value.span_id)
    && isNullableUuid(value.parent_span_id)
    && (link === null || (isRecord(link) && hasExactKeys(link, ["span_id"]) && isUuid(link.span_id)))
    && typeof value.name === "string"
    && value.name.length > 0
    && isTimestamp(value.started_at)
    && isAttributes(value.attributes);
}

function isSpanEnded(value: unknown): value is SpanEnded {
  return isRecord(value)
    && hasExactKeys(value, ["span_id", "ended_at"])
    && isUuid(value.span_id)
    && isTimestamp(value.ended_at);
}

function isEventOccurred(value: unknown): value is EventOccurred {
  if (!isRecord(value) || !hasExactKeys(value, ["event_id", "span_id", "occurred_at", "schema", "payload"])) return false;
  const schema = value.schema;
  return isUuid(value.event_id)
    && isNullableUuid(value.span_id)
    && isTimestamp(value.occurred_at)
    && isRecord(schema)
    && hasExactKeys(schema, ["name", "version"])
    && typeof schema.name === "string"
    && schema.name.length > 0
    && Number.isSafeInteger(schema.version)
    && Number(schema.version) > 0;
}

function isArtifactPublished(value: unknown): value is ArtifactPublished {
  if (!isRecord(value) || !hasExactKeys(value, ["span_id", "metadata"]) || !isNullableUuid(value.span_id)) return false;
  const metadata = value.metadata;
  return isRecord(metadata)
    && hasExactKeys(metadata, ["uri", "purpose", "content_type", "byte_length", "sha256", "attributes"])
    && typeof metadata.uri === "string"
    && artifactRunId(metadata.uri) !== null
    && typeof metadata.purpose === "string"
    && metadata.purpose.length > 0
    && typeof metadata.content_type === "string"
    && metadata.content_type.length > 0
    && isRevision(metadata.byte_length)
    && typeof metadata.sha256 === "string"
    && /^[0-9a-f]{64}$/.test(metadata.sha256)
    && isAttributes(metadata.attributes);
}

function validateSnapshot(value: unknown, requestedRunId: string): RunSnapshot | null {
  if (
    !isRecord(value)
    || !hasExactKeys(value, ["authority_id", "run_id", "through_revision", "spans", "events", "artifacts"])
    || !isUuid(value.authority_id)
    || value.run_id !== requestedRunId
    || !isUuid(value.run_id)
    || !isRevision(value.through_revision)
    || !isRecord(value.spans)
    || !Array.isArray(value.events)
    || !isRecord(value.artifacts)
  ) return null;

  const spanIds = new Set<string>();
  for (const [spanId, candidate] of Object.entries(value.spans)) {
    if (!isRecord(candidate) || !hasExactKeys(candidate, ["started", "ended"]) || !isSpanStarted(candidate.started)) return null;
    if (spanId !== candidate.started.span_id || spanIds.has(spanId) || !hasValidSpanLinks(candidate.started)) return null;
    if (candidate.ended !== null && (!isSpanEnded(candidate.ended) || candidate.ended.span_id !== spanId)) return null;
    if (candidate.ended !== null && compareTimestamp(candidate.ended.ended_at, candidate.started.started_at) < 0) return null;
    spanIds.add(spanId);
  }
  const spans = value.spans as Record<string, SpanSnapshot>;
  if (!hasValidParentGraph(spans)) return null;
  for (const span of Object.values(spans)) {
    if (span.started.parent_span_id === null) continue;
    const parent = spans[span.started.parent_span_id];
    if (parent === undefined || compareTimestamp(span.started.started_at, parent.started.started_at) < 0) return null;
    if (parent.ended !== null && compareTimestamp(span.started.started_at, parent.ended.ended_at) > 0) return null;
  }

  const eventIds = new Set<string>();
  for (const candidate of value.events) {
    if (!isEventOccurred(candidate) || eventIds.has(candidate.event_id)) return null;
    if (candidate.span_id !== null) {
      const span = spans[candidate.span_id];
      if (span === undefined || compareTimestamp(candidate.occurred_at, span.started.started_at) < 0) return null;
      if (span.ended !== null && compareTimestamp(candidate.occurred_at, span.ended.ended_at) > 0) return null;
    }
    eventIds.add(candidate.event_id);
  }

  for (const [uri, candidate] of Object.entries(value.artifacts)) {
    if (!isArtifactPublished(candidate) || candidate.metadata.uri !== uri || artifactRunId(uri) !== value.run_id) return null;
    if (candidate.span_id !== null && !spanIds.has(candidate.span_id)) return null;
  }

  const factCount = spanIds.size
    + Object.values(spans).filter((span) => span.ended !== null).length
    + value.events.length
    + Object.keys(value.artifacts).length;
  const minimumRevision = Math.ceil(factCount / 256);
  if (factCount === 0 || value.through_revision === 0 || value.through_revision < minimumRevision || value.through_revision > factCount) return null;

  return value as unknown as RunSnapshot;
}

function parseFact(value: unknown): RunFact | null {
  if (!isRecord(value) || Object.keys(value).length !== 1) return null;
  if ("span_started" in value && isSpanStarted(value.span_started)) return { span_started: value.span_started };
  if ("span_ended" in value && isSpanEnded(value.span_ended)) return { span_ended: value.span_ended };
  if ("event_occurred" in value && isEventOccurred(value.event_occurred)) return { event_occurred: value.event_occurred };
  if ("artifact_published" in value && isArtifactPublished(value.artifact_published)) return { artifact_published: value.artifact_published };
  return null;
}

function parseCommit(value: unknown): RunCommit | null {
  if (
    !isRecord(value)
    || !hasExactKeys(value, ["authority_id", "run_id", "revision", "idempotency_key", "committed_at", "facts"])
    || !isUuid(value.authority_id)
    || !isUuid(value.run_id)
    || !isRevision(value.revision)
    || !isUuid(value.idempotency_key)
    || !isTimestamp(value.committed_at)
    || !Array.isArray(value.facts)
    || value.facts.length === 0
    || value.facts.length > 256
  ) return null;
  const facts = value.facts.map(parseFact);
  if (facts.some((fact) => fact === null)) return null;
  return { ...value, facts } as unknown as RunCommit;
}

function applyCommit(snapshot: RunSnapshot, value: unknown): RunSnapshot | null {
  const commit = parseCommit(value);
  if (
    commit === null
    || commit.authority_id !== snapshot.authority_id
    || commit.run_id !== snapshot.run_id
    || commit.revision !== snapshot.through_revision + 1
  ) return null;

  const pendingSpans: Record<string, SpanSnapshot> = {};
  for (const fact of commit.facts) {
    if (!("span_started" in fact)) continue;
    const started = fact.span_started;
    if (!hasValidSpanLinks(started) || snapshot.spans[started.span_id] !== undefined || pendingSpans[started.span_id] !== undefined) return null;
    pendingSpans[started.span_id] = { started, ended: null };
  }
  if (!hasValidParentGraph({ ...snapshot.spans, ...pendingSpans })) return null;

  const next: RunSnapshot = {
    ...snapshot,
    spans: { ...snapshot.spans },
    events: [...snapshot.events],
    artifacts: { ...snapshot.artifacts },
  };
  const eventIds = new Set(next.events.map((event) => event.event_id));
  const maxEventAtBySpan = new Map<string, Timestamp>();
  const maxChildStartAtByParent = new Map<string, Timestamp>();
  for (const event of next.events) {
    if (event.span_id !== null) recordMaxTimestamp(maxEventAtBySpan, event.span_id, event.occurred_at);
  }
  for (const span of Object.values(next.spans)) {
    if (span.started.parent_span_id !== null) recordMaxTimestamp(maxChildStartAtByParent, span.started.parent_span_id, span.started.started_at);
  }

  for (const fact of commit.facts) {
    if ("span_started" in fact) {
      const started = fact.span_started;
      if (next.spans[started.span_id] !== undefined) return null;
      if (started.parent_span_id !== null) {
        const parent = next.spans[started.parent_span_id];
        if (parent === undefined || parent.ended !== null || compareTimestamp(started.started_at, parent.started.started_at) < 0) return null;
        recordMaxTimestamp(maxChildStartAtByParent, started.parent_span_id, started.started_at);
      }
      next.spans[started.span_id] = { started, ended: null };
      continue;
    }
    if ("span_ended" in fact) {
      const ended = fact.span_ended;
      const span = next.spans[ended.span_id];
      if (span === undefined || span.ended !== null) return null;
      if (compareTimestamp(ended.ended_at, span.started.started_at) < 0) return null;
      const latestEvent = maxEventAtBySpan.get(ended.span_id);
      if (latestEvent !== undefined && compareTimestamp(latestEvent, ended.ended_at) > 0) return null;
      const latestChild = maxChildStartAtByParent.get(ended.span_id);
      if (latestChild !== undefined && compareTimestamp(latestChild, ended.ended_at) > 0) return null;
      next.spans[ended.span_id] = { started: span.started, ended };
      continue;
    }
    if ("event_occurred" in fact) {
      const event = fact.event_occurred;
      if (eventIds.has(event.event_id) || (event.span_id !== null && next.spans[event.span_id] === undefined)) return null;
      if (event.span_id !== null) {
        const span = next.spans[event.span_id];
        if (span.ended !== null || compareTimestamp(event.occurred_at, span.started.started_at) < 0) return null;
        recordMaxTimestamp(maxEventAtBySpan, event.span_id, event.occurred_at);
      }
      eventIds.add(event.event_id);
      next.events.push(event);
      continue;
    }
    const artifact = fact.artifact_published;
    if (
      artifactRunId(artifact.metadata.uri) !== snapshot.run_id
      || next.artifacts[artifact.metadata.uri] !== undefined
      || (artifact.span_id !== null && next.spans[artifact.span_id] === undefined)
    ) return null;
    next.artifacts[artifact.metadata.uri] = artifact;
  }

  next.through_revision = commit.revision;
  return next;
}

function parseRunApiError(value: unknown): ErrorState | null {
  if (value === "not_found") return { message: "not_found", transient: false };
  if (value === "forbidden") return { message: "forbidden", transient: false };
  if (value === "idempotency_mismatch") return { message: "idempotency_mismatch", transient: false };
  if (!isRecord(value) || Object.keys(value).length !== 1) return null;
  const [variant] = Object.keys(value);
  const payload = value[variant];
  if (!isRecord(payload)) return null;
  if (variant === "unavailable" && typeof payload.code === "string") return { message: `unavailable: ${payload.code}`, transient: true };
  if (variant === "integrity" && typeof payload.code === "string") return { message: `integrity: ${payload.code}`, transient: false };
  if (variant === "invalid_reference" && typeof payload.code === "string") return { message: `invalid_reference: ${payload.code}`, transient: false };
  if (variant === "rejected" && typeof payload.code === "string") return { message: `rejected: ${payload.code}`, transient: false };
  if (variant === "authority_mismatch") return { message: "authority_mismatch", transient: false };
  if (variant === "history_gap") return { message: "history_gap", transient: false };
  if (variant === "cursor_ahead") return { message: "cursor_ahead", transient: false };
  return null;
}

function timestampText(timestamp: Timestamp | null): string {
  if (timestamp === null) return "-";
  return new Date(timestamp.unix_seconds * 1000 + Math.floor(timestamp.nanoseconds / 1_000_000)).toISOString();
}

function shortId(value: string): string {
  return value.length > 20 ? `${value.slice(0, 12)}...${value.slice(-6)}` : value;
}

function textNode(document: Document, tag: string, className: string, text: string): HTMLElement {
  const node = document.createElement(tag);
  node.className = className;
  node.textContent = text;
  return node;
}

function renderSnapshot(document: Document, snapshot: RunSnapshot): void {
  element(document, "main-label").textContent = `Run / ${shortId(snapshot.run_id)}`;
  element(document, "main-crumb").textContent = `revision ${snapshot.through_revision}`;
  element(document, "run-count").textContent = "1";
  element(document, "event-count").textContent = String(snapshot.events.length);
  element(document, "artifact-count").textContent = String(Object.keys(snapshot.artifacts).length);

  const runList = element(document, "run-list");
  runList.replaceChildren();
  const runRow = textNode(document, "div", "run-row active", snapshot.run_id);
  runRow.appendChild(textNode(document, "span", "row-meta", `through revision ${snapshot.through_revision}`));
  runList.appendChild(runRow);

  const spanTree = element(document, "span-tree");
  spanTree.replaceChildren();
  const spans = Object.values(snapshot.spans);
  if (spans.length === 0) spanTree.appendChild(textNode(document, "div", "run-row empty", "no spans"));
  for (const span of spans) {
    const row = document.createElement("div");
    row.className = "span-row";
    row.appendChild(textNode(document, "span", "span-name", span.started.name));
    row.appendChild(textNode(document, "span", "span-id", shortId(span.started.span_id)));
    row.appendChild(textNode(document, "span", "status-pill s-frozen", span.ended === null ? "open" : "closed"));
    row.title = `${timestampText(span.started.started_at)} / ${timestampText(span.ended?.ended_at ?? null)}`;
    spanTree.appendChild(row);
  }

  const eventList = element(document, "event-list");
  eventList.replaceChildren();
  for (const event of snapshot.events) {
    const row = document.createElement("div");
    row.className = "event-row";
    row.appendChild(textNode(document, "span", "event-name", `${event.schema.name} v${event.schema.version}`));
    row.appendChild(textNode(document, "pre", "event-payload", JSON.stringify(event.payload, null, 2)));
    eventList.appendChild(row);
  }

  const artifactList = element(document, "artifact-list");
  artifactList.replaceChildren();
  // TODO(inspect-artifact-transfer-v1): Task 12 supplies binary reads; this
  // viewer intentionally renders committed metadata only.
  for (const artifact of Object.values(snapshot.artifacts)) {
    const row = document.createElement("div");
    row.className = "artifact-row";
    row.appendChild(textNode(document, "div", "artifact-name", artifact.metadata.purpose));
    row.appendChild(textNode(document, "div", "artifact-meta", artifact.metadata.content_type));
    row.appendChild(textNode(document, "div", "artifact-uri", artifact.metadata.uri));
    artifactList.appendChild(row);
  }
}

function setConnection(document: Document, connected: boolean, endpoint: string): void {
  const pill = element(document, "conn");
  pill.className = connected ? "conn-pill live" : "conn-pill bad";
  element(document, "conn-label").textContent = connected ? "live" : "offline";
  element(document, "conn-endpoint").textContent = endpoint;
}

/** Mounts the snapshot-first Inspect viewer into the server document. */
export function mountInspectViewer(document: Document): void {
  const defaultView = document.defaultView;
  if (defaultView === null) throw new Error("viewer document has no default window");
  const window: Window = defaultView;
  const state: ViewerState = {
    runId: null,
    snapshot: null,
    source: null,
    generation: 0,
    retryTimer: null,
    stabilityTimer: null,
    recoveryAttempts: 0,
  };

  function clearStability(): void {
    if (state.stabilityTimer !== null) window.clearTimeout(state.stabilityTimer);
    state.stabilityTimer = null;
  }

  function clearRetry(): void {
    if (state.retryTimer !== null) window.clearTimeout(state.retryTimer);
    state.retryTimer = null;
  }

  function closeSource(): void {
    clearStability();
    state.source?.close();
    state.source = null;
  }

  function stopWith(message: string): void {
    state.generation += 1;
    clearRetry();
    closeSource();
    setConnection(document, false, message);
  }

  function scheduleRecovery(generation: number, reason: string): void {
    if (generation !== state.generation) return;
    closeSource();
    clearRetry();
    if (state.recoveryAttempts >= MAX_RECOVERY_ATTEMPTS) {
      stopWith(`recovery exhausted: ${reason}`);
      return;
    }
    const delay = state.recoveryAttempts === 0
      ? 0
      : Math.min(BASE_RETRY_MILLIS * 2 ** (state.recoveryAttempts - 1), MAX_RETRY_MILLIS);
    state.recoveryAttempts += 1;
    const recoveryGeneration = ++state.generation;
    setConnection(document, false, reason);

    /** Retries a transient snapshot or stream failure with bounded backoff. */
    function onRetryTimer(): void {
      state.retryTimer = null;
      if (recoveryGeneration === state.generation) void reloadSnapshot();
    }

    state.retryTimer = window.setTimeout(onRetryTimer, delay);
  }

  function handleTypedError(generation: number, value: unknown): void {
    const error = parseRunApiError(value);
    if (error === null) {
      stopWith("invalid error payload");
    } else if (error.transient) {
      scheduleRecovery(generation, error.message);
    } else {
      stopWith(error.message);
    }
  }

  function subscribeAfter(snapshot: RunSnapshot, generation: number): void {
    closeSource();
    const source = new EventSource(streamEndpoint(snapshot.run_id, snapshot.through_revision));
    state.source = source;

    /** Marks a current SSE connection live and starts its stability window. */
    function onOpen(): void {
      if (generation !== state.generation || state.source !== source) return;
      clearStability();
      setConnection(document, true, `revision ${snapshot.through_revision}`);
      const stabilityTimer = window.setTimeout(() => {
        if (state.stabilityTimer !== stabilityTimer) return;
        state.stabilityTimer = null;
        if (generation === state.generation && state.source === source) state.recoveryAttempts = 0;
      }, CONNECTION_STABILITY_MILLIS);
      state.stabilityTimer = stabilityTimer;
    }

    /** Applies one strictly validated commit or recovers from the snapshot. */
    function onCommit(message: MessageEvent<string>): void {
      if (generation !== state.generation || state.source !== source || state.snapshot === null) return;
      let value: unknown;
      try {
        value = JSON.parse(message.data);
      } catch {
        scheduleRecovery(generation, "malformed commit");
        return;
      }
      const next = applyCommit(state.snapshot, value);
      if (next === null) {
        scheduleRecovery(generation, "malformed commit");
        return;
      }
      state.snapshot = next;
      clearStability();
      state.recoveryAttempts = 0;
      renderSnapshot(document, next);
      setConnection(document, true, `revision ${next.through_revision}`);
    }

    /** Recovers an explicit retention gap from a fresh snapshot. */
    function onGap(): void {
      if (state.source !== source) return;
      scheduleRecovery(generation, "history gap");
    }

    /** Separates typed store failures from EventSource transport failures. */
    function onStreamError(event: Event): void {
      if (generation !== state.generation || state.source !== source) return;
      if (event instanceof MessageEvent && typeof event.data === "string") {
        try {
          handleTypedError(generation, JSON.parse(event.data) as unknown);
        } catch {
          stopWith("invalid error payload");
        }
        return;
      }
      scheduleRecovery(generation, "stream unavailable");
    }

    source.addEventListener("open", onOpen);
    source.addEventListener("commit", onCommit as EventListener);
    source.addEventListener("gap", onGap);
    source.addEventListener("error", onStreamError);
  }

  async function reloadSnapshot(): Promise<void> {
    const runId = state.runId;
    if (runId === null || runId.length === 0) return;
    const generation = ++state.generation;
    closeSource();
    clearRetry();
    setConnection(document, false, "loading snapshot");
    let response: Response;
    try {
      response = await fetch(snapshotEndpoint(runId), { headers: { Accept: RUN_MEDIA_TYPE } });
    } catch {
      if (generation !== state.generation) return;
      scheduleRecovery(generation, "snapshot transport unavailable");
      return;
    }
    if (generation !== state.generation) return;
    let value: unknown;
    try {
      value = await response.json() as unknown;
    } catch {
      if (generation !== state.generation) return;
      stopWith("invalid snapshot response");
      return;
    }
    if (generation !== state.generation) return;
    if (!response.ok) {
      const error = parseRunApiError(value);
      if (error?.transient === true) {
        scheduleRecovery(generation, error.message);
      } else {
        stopWith(error?.message ?? `HTTP ${response.status}`);
      }
      return;
    }
    const snapshot = validateSnapshot(value, runId);
    if (snapshot === null) {
      stopWith("invalid snapshot");
      return;
    }
    state.snapshot = snapshot;
    renderSnapshot(document, snapshot);
    subscribeAfter(snapshot, generation);
  }

  /** Selects the run entered in the viewer toolbar. */
  function onLoadRequested(): void {
    const input = element<HTMLInputElement>(document, "run-id-input");
    const runId = input.value.trim();
    if (!isUuid(runId)) {
      stopWith("invalid run id");
      return;
    }
    state.runId = runId;
    state.snapshot = null;
    state.recoveryAttempts = 0;
    const url = new URL(window.location.href);
    url.searchParams.set("run_id", runId);
    window.history.replaceState(null, "", url);
    void reloadSnapshot();
  }

  element<HTMLButtonElement>(document, "load-run").addEventListener("click", onLoadRequested);
  const initialRunId = new URL(window.location.href).searchParams.get("run_id");
  if (initialRunId !== null && isUuid(initialRunId)) {
    element<HTMLInputElement>(document, "run-id-input").value = initialRunId;
    state.runId = initialRunId;
    void reloadSnapshot();
  } else if (initialRunId !== null) {
    setConnection(document, false, "invalid run id");
  } else {
    setConnection(document, false, "no run selected");
  }
}
