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
}

function snapshotEndpoint(runId: string): string {
  return `/v1/runs/${encodeURIComponent(runId)}/snapshot`;
}

function streamEndpoint(runId: string, afterRevision: number): string {
  return `/v1/runs/${encodeURIComponent(runId)}/commits/stream?after_revision=${afterRevision}`;
}

function element<T extends HTMLElement>(document: Document, id: string): T {
  const value = document.getElementById(id);
  if (value === null) {
    throw new Error(`viewer element #${id} is missing`);
  }
  return value as T;
}

function timestampText(timestamp: Timestamp | null): string {
  if (timestamp === null) return "-";
  const millis = timestamp.unix_seconds * 1000 + Math.floor(timestamp.nanoseconds / 1_000_000);
  return new Date(millis).toISOString();
}

function shortId(value: string): string {
  return value.length > 20 ? `${value.slice(0, 12)}...${value.slice(-6)}` : value;
}

function clear(node: HTMLElement): void {
  node.replaceChildren();
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
  clear(runList);
  const runRow = textNode(document, "div", "run-row active", snapshot.run_id);
  runRow.appendChild(textNode(document, "span", "row-meta", `through revision ${snapshot.through_revision}`));
  runList.appendChild(runRow);

  const spanTree = element(document, "span-tree");
  clear(spanTree);
  const spans = Object.values(snapshot.spans);
  if (spans.length === 0) {
    spanTree.appendChild(textNode(document, "div", "run-row empty", "no spans"));
  }
  for (const span of spans) {
    const row = document.createElement("div");
    row.className = "span-row";
    row.appendChild(textNode(document, "span", "span-name", span.started.name));
    row.appendChild(textNode(document, "span", "span-id", shortId(span.started.span_id)));
    const lifecycle = span.ended === null ? "open" : "closed";
    row.appendChild(textNode(document, "span", "status-pill s-frozen", lifecycle));
    row.title = `${timestampText(span.started.started_at)} / ${timestampText(span.ended?.ended_at ?? null)}`;
    spanTree.appendChild(row);
  }

  const eventList = element(document, "event-list");
  clear(eventList);
  for (const event of snapshot.events) {
    const row = document.createElement("div");
    row.className = "event-row";
    row.appendChild(textNode(document, "span", "event-name", `${event.schema.name} v${event.schema.version}`));
    row.appendChild(textNode(document, "pre", "event-payload", JSON.stringify(event.payload, null, 2)));
    eventList.appendChild(row);
  }

  const artifactList = element(document, "artifact-list");
  clear(artifactList);
  // TODO(inspect-artifact-transfer-v1): Binary previews wait for Task 12's
  // resolver and read endpoints; Task 11 renders committed metadata only.
  for (const artifact of Object.values(snapshot.artifacts)) {
    const row = document.createElement("div");
    row.className = "artifact-row";
    row.appendChild(textNode(document, "div", "artifact-name", artifact.metadata.purpose));
    row.appendChild(textNode(document, "div", "artifact-meta", artifact.metadata.content_type));
    row.appendChild(textNode(document, "div", "artifact-uri", artifact.metadata.uri));
    artifactList.appendChild(row);
  }
}

function applyCommit(snapshot: RunSnapshot, commit: RunCommit): boolean {
  if (
    commit.authority_id !== snapshot.authority_id
    || commit.run_id !== snapshot.run_id
    || commit.revision !== snapshot.through_revision + 1
  ) {
    return false;
  }
  for (const fact of commit.facts) {
    if ("span_started" in fact) {
      snapshot.spans[fact.span_started.span_id] = { started: fact.span_started, ended: null };
    } else if ("span_ended" in fact) {
      const span = snapshot.spans[fact.span_ended.span_id];
      if (span === undefined || span.ended !== null) return false;
      span.ended = fact.span_ended;
    } else if ("event_occurred" in fact) {
      snapshot.events.push(fact.event_occurred);
    } else if ("artifact_published" in fact) {
      snapshot.artifacts[fact.artifact_published.metadata.uri] = fact.artifact_published;
    }
  }
  snapshot.through_revision = commit.revision;
  return true;
}

function setConnection(document: Document, connected: boolean, endpoint: string): void {
  const pill = element(document, "conn");
  pill.className = connected ? "conn-pill live" : "conn-pill bad";
  element(document, "conn-label").textContent = connected ? "live" : "offline";
  element(document, "conn-endpoint").textContent = endpoint;
}

/**
 * Mounts the snapshot-first Inspect viewer into the server document.
 *
 * Triggering workflow:
 *
 * `App.onMounted`
 *   -> {@link mountInspectViewer}
 *     -> `viewer.mount`
 *       -> {@link reloadSnapshot}
 *
 * Upstream:
 * - Vue `onMounted`
 *
 * Downstream:
 * - `GET /v1/runs/{run_id}/snapshot`
 */
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
  };

  function closeSource(): void {
    state.source?.close();
    state.source = null;
  }

  function scheduleReload(generation: number): void {
    if (state.retryTimer !== null) window.clearTimeout(state.retryTimer);
    state.retryTimer = window.setTimeout(retrySnapshot, 500);

    /**
     * Re-loads after the current SSE connection can no longer recover itself.
     *
     * Triggering workflow:
     *
     * `Window.setTimeout`
     *   -> {@link scheduleReload}
     *     -> `viewer.retry`
     *       -> {@link retrySnapshot}
     *
     * Upstream:
     * - {@link scheduleReload}
     *
     * Downstream:
     * - {@link reloadSnapshot}
     */
    function retrySnapshot(): void {
      state.retryTimer = null;
      if (generation === state.generation) void reloadSnapshot();
    }
  }

  function subscribeAfter(snapshot: RunSnapshot, generation: number): void {
    closeSource();
    const source = new EventSource(streamEndpoint(snapshot.run_id, snapshot.through_revision));
    state.source = source;

    /**
     * Applies the next canonical commit and rejects any revision gap.
     *
     * Triggering workflow:
     *
     * {@link EventSource}
     *   -> {@link subscribeAfter}
     *     -> `commit`
     *       -> {@link onCommit}
     *
     * Upstream:
     * - {@link subscribeAfter}
     *
     * Downstream:
     * - {@link applyCommit}
     */
    function onCommit(message: MessageEvent<string>): void {
      if (generation !== state.generation || state.snapshot === null) return;
      try {
        const commit = JSON.parse(message.data) as RunCommit;
        if (!applyCommit(state.snapshot, commit)) {
          closeSource();
          void reloadSnapshot();
          return;
        }
        renderSnapshot(document, state.snapshot);
        setConnection(document, true, `revision ${state.snapshot.through_revision}`);
      } catch {
        closeSource();
        void reloadSnapshot();
      }
    }

    /**
     * Recovers an explicit retention gap from the snapshot authority.
     *
     * Triggering workflow:
     *
     * {@link EventSource}
     *   -> {@link subscribeAfter}
     *     -> `gap`
     *       -> {@link onGap}
     *
     * Upstream:
     * - {@link subscribeAfter}
     *
     * Downstream:
     * - {@link reloadSnapshot}
     */
    function onGap(): void {
      if (generation !== state.generation) return;
      closeSource();
      void reloadSnapshot();
    }

    /**
     * Re-establishes snapshot authority after a stream transport failure.
     *
     * Triggering workflow:
     *
     * {@link EventSource}
     *   -> {@link subscribeAfter}
     *     -> `error`
     *       -> {@link onStreamError}
     *
     * Upstream:
     * - {@link subscribeAfter}
     *
     * Downstream:
     * - {@link scheduleReload}
     */
    function onStreamError(): void {
      if (generation !== state.generation) return;
      closeSource();
      setConnection(document, false, "stream disconnected");
      scheduleReload(generation);
    }

    source.addEventListener("commit", onCommit as EventListener);
    source.addEventListener("gap", onGap);
    source.addEventListener("error", onStreamError);
  }

  async function reloadSnapshot(): Promise<void> {
    const runId = state.runId;
    if (runId === null || runId.length === 0) return;
    const generation = ++state.generation;
    closeSource();
    setConnection(document, false, "loading snapshot");
    try {
      const response = await fetch(snapshotEndpoint(runId), {
        headers: { Accept: "application/vnd.auv.run+json; version=1" },
      });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const snapshot = await response.json() as RunSnapshot;
      if (generation !== state.generation) return;
      state.snapshot = snapshot;
      renderSnapshot(document, snapshot);
      subscribeAfter(snapshot, generation);
    } catch (error) {
      if (generation !== state.generation) return;
      state.snapshot = null;
      setConnection(document, false, error instanceof Error ? error.message : "snapshot unavailable");
      scheduleReload(generation);
    }
  }

  /**
   * Selects the run entered in the viewer toolbar.
   *
   * Triggering workflow:
   *
   * `HTMLButtonElement.click`
   *   -> `#load-run`
   *     -> `viewer.load`
   *       -> {@link onLoadRequested}
   *
   * Upstream:
   * - `#load-run`
   *
   * Downstream:
   * - {@link reloadSnapshot}
   */
  function onLoadRequested(): void {
    const input = element<HTMLInputElement>(document, "run-id-input");
    const runId = input.value.trim();
    if (runId.length === 0) return;
    state.runId = runId;
    const url = new URL(window.location.href);
    url.searchParams.set("run_id", runId);
    window.history.replaceState(null, "", url);
    void reloadSnapshot();
  }

  element<HTMLButtonElement>(document, "load-run").addEventListener("click", onLoadRequested);
  const initialRunId = new URL(window.location.href).searchParams.get("run_id");
  if (initialRunId !== null && initialRunId.length > 0) {
    element<HTMLInputElement>(document, "run-id-input").value = initialRunId;
    state.runId = initialRunId;
    void reloadSnapshot();
  } else {
    setConnection(document, false, "no run selected");
  }
}
