import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { Window } from "happy-dom";

const AUTHORITY = "019f8b1e-4b2d-7a00-8f00-0000000000aa";
const RUN = "019f8b1e-4b2d-7a00-8f00-000000000001";
const PERMANENT_CASES = new Map([
  ["019f8b1e-4b2d-7a00-8f00-000000000002", { status: 404, body: "not_found", message: "not_found" }],
  ["019f8b1e-4b2d-7a00-8f00-000000000003", {
    status: 400,
    body: { invalid_reference: { code: "auv.test.invalid" } },
    message: "invalid_reference: auv.test.invalid"
  }],
  ["019f8b1e-4b2d-7a00-8f00-000000000004", { status: 403, body: "forbidden", message: "forbidden" }],
  ["019f8b1e-4b2d-7a00-8f00-000000000005", {
    status: 500,
    body: { integrity: { code: "auv.test.integrity" } },
    message: "integrity: auv.test.integrity"
  }],
  ["019f8b1e-4b2d-7a00-8f00-000000000006", {
    status: 500,
    body: { unexpected: { code: "auv.test.unknown" } },
    message: "HTTP 500"
  }],
  ["019f8b1e-4b2d-7a00-8f00-000000000007", {
    status: 200,
    body: { authority_id: AUTHORITY, run_id: "malformed" },
    message: "invalid snapshot"
  }]
]);
const SPAN = "019f8b1e-4b2d-7a00-8f00-000000000011";
const EVENT = "019f8b1e-4b2d-7a00-8f00-000000000021";
const KEY = "019f8b1e-4b2d-7a00-8f00-000000000031";

const viewerRoot = resolve(import.meta.dirname, "..");
const html = await readFile(resolve(viewerRoot, "dist/index.html"), "utf8");
const window = new Window({ url: `http://127.0.0.1:8765/?run_id=${RUN}` });
const { document } = window;
document.write(html);
document.close();

const operations = [];
const sources = [];
let primarySnapshotRequests = 0;
const permanentSnapshotRequests = new Map();

function timestamp(seconds) {
  return { unix_seconds: seconds, nanoseconds: 0 };
}

function eventOccurred() {
  return {
    event_id: EVENT,
    span_id: SPAN,
    occurred_at: timestamp(2),
    schema: { name: "auv.test.event", version: 1 },
    payload: { value: "committed" }
  };
}

function snapshot(revision, includeEvent) {
  return {
    authority_id: AUTHORITY,
    run_id: RUN,
    through_revision: revision,
    spans: {
      [SPAN]: {
        started: {
          span_id: SPAN,
          parent_span_id: null,
          remote_link: null,
          name: "auv.test.root",
          started_at: timestamp(1),
          attributes: {}
        },
        ended: null
      }
    },
    events: includeEvent ? [eventOccurred()] : [],
    artifacts: {}
  };
}

const fetchStub = async (input) => {
  const url = String(input);
  operations.push(`fetch:${url}`);
  if (url.includes("/runs") && !url.startsWith("/v1/")) {
    throw new Error(`legacy run endpoint used: ${url}`);
  }
  if (url === `/v1/runs/${RUN}/snapshot`) {
    primarySnapshotRequests += 1;
    return Response.json(snapshot(primarySnapshotRequests === 1 ? 1 : 2, primarySnapshotRequests !== 1), {
      headers: { "content-type": "application/vnd.auv.run+json; version=1" }
    });
  }
  const permanentRun = [...PERMANENT_CASES.keys()].find((runId) => url === `/v1/runs/${runId}/snapshot`);
  if (permanentRun !== undefined) {
    const scenario = PERMANENT_CASES.get(permanentRun);
    permanentSnapshotRequests.set(permanentRun, (permanentSnapshotRequests.get(permanentRun) ?? 0) + 1);
    return Response.json(scenario.body, {
      status: scenario.status,
      headers: { "content-type": "application/vnd.auv.run+json; version=1" }
    });
  }
  throw new Error(`unexpected fetch: ${url}`);
};

class FakeEventSource {
  constructor(url) {
    this.url = String(url);
    this.closed = false;
    this.listeners = new Map();
    operations.push(`stream:${this.url}`);
    sources.push(this);
  }

  addEventListener(type, listener) {
    const listeners = this.listeners.get(type) ?? [];
    listeners.push(listener);
    this.listeners.set(type, listeners);
  }

  close() {
    this.closed = true;
  }

  emit(type, data) {
    const event = new window.MessageEvent(type, { data });
    for (const listener of this.listeners.get(type) ?? []) {
      listener(event);
    }
  }

  emitTransportError() {
    const event = new window.Event("error");
    for (const listener of this.listeners.get("error") ?? []) {
      listener(event);
    }
  }
}

class ForbiddenWebSocket {
  constructor() {
    throw new Error("legacy WebSocket transport was constructed");
  }
}

function exposeGlobal(name, value) {
  Object.defineProperty(globalThis, name, {
    configurable: true,
    value
  });
}

for (const [name, value] of Object.entries({
  window,
  document,
  location: window.location,
  navigator: window.navigator,
  Node: window.Node,
  Text: window.Text,
  Element: window.Element,
  HTMLElement: window.HTMLElement,
  SVGElement: window.SVGElement,
  Event: window.Event,
  MessageEvent: window.MessageEvent,
  CustomEvent: window.CustomEvent,
  MutationObserver: window.MutationObserver,
  fetch: fetchStub,
  EventSource: FakeEventSource,
  WebSocket: ForbiddenWebSocket
})) {
  exposeGlobal(name, value);
}
window.fetch = fetchStub;
window.EventSource = FakeEventSource;
window.WebSocket = ForbiddenWebSocket;

await import(`${pathToFileURL(resolve(viewerRoot, "dist/assets/viewer.js")).href}?smoke=${Date.now()}`);

async function waitFor(predicate, label, timeout = 1500) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (predicate()) return;
    await new Promise((resolveAfterDelay) => window.setTimeout(resolveAfterDelay, 10));
  }
  throw new Error(`timed out waiting for ${label}`);
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

async function selectPrimaryRun(label) {
  const selectedSourceCount = sources.length;
  document.getElementById("run-id-input").value = RUN;
  document.getElementById("load-run").click();
  await waitFor(() => sources.length === selectedSourceCount + 1, `${label} selected-run snapshot`);
}

async function assertMalformedCommitRecovers(commit, label) {
  await selectPrimaryRun(label);
  const sourceCount = sources.length;
  const requestCount = primarySnapshotRequests;
  sources.at(-1).emit("commit", JSON.stringify(commit));
  await waitFor(() => sources.length === sourceCount + 1, `${label} snapshot recovery`);
  assert(primarySnapshotRequests === requestCount + 1, `${label} did not reload exactly one snapshot`);
  assert(document.getElementById("main-crumb")?.textContent === "revision 2", `${label} advanced the rendered cursor`);
  assert(document.getElementById("artifact-count")?.textContent === "0", `${label} partially published an artifact`);
  assert(document.querySelector(".status-pill")?.textContent === "open", `${label} partially ended a span`);
  assert(
    sources.at(-1).url === `/v1/runs/${RUN}/commits/stream?after_revision=2`,
    `${label} recovery resumed from the wrong cursor: ${sources.at(-1).url}`
  );
}

await waitFor(() => sources.length === 1, "initial revision stream");
assert(operations[0] === `fetch:/v1/runs/${RUN}/snapshot`, `snapshot was not first: ${operations.join(", ")}`);
assert(
  operations[1] === `stream:/v1/runs/${RUN}/commits/stream?after_revision=1`,
  `stream did not use snapshot cursor: ${operations.join(", ")}`
);

sources[0].emit("commit", JSON.stringify({
  authority_id: AUTHORITY,
  run_id: RUN,
  revision: 2,
  idempotency_key: KEY,
  committed_at: timestamp(2),
  facts: [{ event_occurred: eventOccurred() }]
}));
await waitFor(() => document.getElementById("event-count")?.textContent === "1", "commit DOM update");
assert(document.getElementById("main-crumb")?.textContent === "revision 2", "commit did not advance the rendered revision");

sources[0].emit("gap", JSON.stringify({ requested_after: 2, earliest_available: 3 }));
await waitFor(() => sources.length === 2, "gap snapshot recovery");
assert(primarySnapshotRequests === 2, "gap did not reload exactly one snapshot");
assert(
  sources[1].url === `/v1/runs/${RUN}/commits/stream?after_revision=2`,
  `gap stream did not resume from snapshot cursor: ${sources[1].url}`
);

await assertMalformedCommitRecovers({
  authority_id: AUTHORITY,
  run_id: RUN,
  revision: 3,
  idempotency_key: KEY,
  committed_at: timestamp(3),
  facts: [
    {
      artifact_published: {
        span_id: SPAN,
        metadata: {
          uri: `auv://runs/${RUN}/artifacts/019f8b1e-4b2d-7a00-8f00-000000000041`,
          purpose: "auv.test.output",
          content_type: "text/plain",
          byte_length: 5,
          sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
          attributes: {}
        }
      }
    },
    { unknown_fact: { value: true } }
  ]
}, "unknown fact");

await assertMalformedCommitRecovers({
  authority_id: AUTHORITY,
  run_id: RUN,
  revision: 3,
  idempotency_key: KEY,
  committed_at: timestamp(3),
  facts: [{
    span_ended: { span_id: SPAN, ended_at: timestamp(3) },
    event_occurred: eventOccurred()
  }]
}, "multi-variant fact");

await assertMalformedCommitRecovers({
  authority_id: AUTHORITY,
  run_id: RUN,
  revision: 3,
  idempotency_key: KEY,
  committed_at: timestamp(3),
  facts: [{
    span_started: {
      span_id: SPAN,
      parent_span_id: null,
      remote_link: null,
      name: "auv.test.duplicate",
      started_at: timestamp(3),
      attributes: {}
    }
  }]
}, "duplicate span start");

await assertMalformedCommitRecovers({
  authority_id: AUTHORITY,
  run_id: RUN,
  revision: 3,
  idempotency_key: KEY,
  committed_at: timestamp(3),
  facts: [{ event_occurred: eventOccurred() }]
}, "duplicate event");

const artifactFact = {
  artifact_published: {
    span_id: SPAN,
    metadata: {
      uri: `auv://runs/${RUN}/artifacts/019f8b1e-4b2d-7a00-8f00-000000000041`,
      purpose: "auv.test.output",
      content_type: "text/plain",
      byte_length: 5,
      sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
      attributes: {}
    }
  }
};
await assertMalformedCommitRecovers({
  authority_id: AUTHORITY,
  run_id: RUN,
  revision: 3,
  idempotency_key: KEY,
  committed_at: timestamp(3),
  facts: [artifactFact, artifactFact]
}, "duplicate artifact");

await assertMalformedCommitRecovers({
  authority_id: AUTHORITY,
  run_id: RUN,
  revision: 3,
  idempotency_key: KEY,
  committed_at: timestamp(3),
  facts: [{ span_ended: { span_id: "019f8b1e-4b2d-7a00-8f00-000000000099", ended_at: timestamp(3) } }]
}, "mismatched span end");

for (const [label, overrides] of [
  ["invalid authority", { authority_id: "019f8b1e-4b2d-7a00-8f00-0000000000ab" }],
  ["invalid run", { run_id: "019f8b1e-4b2d-7a00-8f00-000000000099" }],
  ["invalid revision", { revision: 4 }]
]) {
  await assertMalformedCommitRecovers({
    authority_id: AUTHORITY,
    run_id: RUN,
    revision: 3,
    idempotency_key: KEY,
    committed_at: timestamp(3),
    facts: [{ span_ended: { span_id: SPAN, ended_at: timestamp(3) } }],
    ...overrides
  }, label);
}

await selectPrimaryRun("typed unavailable");
{
  const sourceCount = sources.length;
  const requestCount = primarySnapshotRequests;
  sources.at(-1).emit("error", JSON.stringify({ unavailable: { code: "auv.test.unavailable" } }));
  await waitFor(() => sources.length === sourceCount + 1, "typed unavailable recovery");
  assert(primarySnapshotRequests === requestCount + 1, "typed unavailable did not reload the snapshot");
}

await selectPrimaryRun("transport failure");
{
  const sourceCount = sources.length;
  const requestCount = primarySnapshotRequests;
  sources.at(-1).emitTransportError();
  await waitFor(() => sources.length === sourceCount + 1, "transport failure recovery");
  assert(primarySnapshotRequests === requestCount + 1, "transport failure did not reload the snapshot");
}

sources.at(-1).emit("error", JSON.stringify({ integrity: { code: "auv.test.integrity" } }));
await waitFor(() => document.getElementById("conn-endpoint")?.textContent === "integrity: auv.test.integrity", "typed stream error state");
const requestsAfterIntegrity = primarySnapshotRequests;
await new Promise((resolveAfterDelay) => window.setTimeout(resolveAfterDelay, 650));
assert(primarySnapshotRequests === requestsAfterIntegrity, "permanent typed stream error retried");

const input = document.getElementById("run-id-input");
for (const [runId, scenario] of PERMANENT_CASES) {
  input.value = runId;
  document.getElementById("load-run").click();
  await waitFor(() => document.getElementById("conn-endpoint")?.textContent === scenario.message, `${scenario.status} snapshot error state`);
  await new Promise((resolveAfterDelay) => window.setTimeout(resolveAfterDelay, 650));
  assert(permanentSnapshotRequests.get(runId) === 1, `${scenario.status} permanent snapshot error retried`);
}
assert(operations.every((operation) => !operation.includes("/write")), `legacy write endpoint used: ${operations.join(", ")}`);
