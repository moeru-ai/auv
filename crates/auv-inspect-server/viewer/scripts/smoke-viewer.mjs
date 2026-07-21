import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { Window } from "happy-dom";

const AUTHORITY = "019f8b1e-4b2d-7a00-8f00-0000000000aa";
const RUN = "019f8b1e-4b2d-7a00-8f00-000000000001";
const OTHER_RUN = "019f8b1e-4b2d-7a00-8f00-0000000000ff";
const STALE_RUN = "019f8b1e-4b2d-7a00-8f00-000000000008";
const NEWER_RUN = "019f8b1e-4b2d-7a00-8f00-000000000009";
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
const STABILITY_SETTLE_MILLIS = 125;

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
const invalidSnapshotRequests = new Map();
let staleSnapshotJson;

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function timestamp(seconds) {
  return { unix_seconds: seconds, nanoseconds: 0 };
}

function id(index) {
  return `019f8b1e-4b2d-7a00-8f00-${index.toString(16).padStart(12, "0")}`;
}

function spanStarted(spanId, { parent = null, remote = null, at = 2 } = {}) {
  return {
    span_id: spanId,
    parent_span_id: parent,
    remote_link: remote === null ? null : { span_id: remote },
    name: "auv.test.child",
    started_at: timestamp(at),
    attributes: {}
  };
}

function spanEnded(spanId, at) {
  return { span_id: spanId, ended_at: timestamp(at) };
}

function eventFor(eventId, { span = SPAN, at = 2 } = {}) {
  return {
    event_id: eventId,
    span_id: span,
    occurred_at: timestamp(at),
    schema: { name: "auv.test.event", version: 1 },
    payload: { value: "committed" }
  };
}

function artifactUri(runId, artifactId) {
  return `auv://runs/${runId}/artifacts/${artifactId}`;
}

function artifactFor(runId, artifactId, span = SPAN) {
  return {
    span_id: span,
    metadata: {
      uri: artifactUri(runId, artifactId),
      purpose: "auv.test.output",
      content_type: "text/plain",
      byte_length: 5,
      sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
      attributes: {}
    }
  };
}

function commitFor(facts) {
  return {
    authority_id: AUTHORITY,
    run_id: RUN,
    revision: 3,
    idempotency_key: KEY,
    committed_at: timestamp(3),
    facts
  };
}

function eventOccurred() {
  return eventFor(EVENT);
}

function snapshot(revision, includeEvent, runId = RUN) {
  return {
    authority_id: AUTHORITY,
    run_id: runId,
    through_revision: revision,
    spans: {
      [SPAN]: {
        started: {
          ...spanStarted(SPAN, { at: 1 }),
          name: "auv.test.root"
        },
        ended: null
      }
    },
    events: includeEvent ? [eventOccurred()] : [],
    artifacts: {}
  };
}

function invalidSnapshot(runId, mutate) {
  const candidate = snapshot(1, false, runId);
  mutate(candidate);
  return candidate;
}

const SNAPSHOT_INVARIANT_CASES = new Map([
  [id(0x101), invalidSnapshot(id(0x101), (value) => { value.spans[SPAN].ended = spanEnded(SPAN, 0); })],
  [id(0x102), invalidSnapshot(id(0x102), (value) => { value.events = [eventFor(id(0x201), { at: 0 })]; })],
  [id(0x103), invalidSnapshot(id(0x103), (value) => {
    value.spans[SPAN].ended = spanEnded(SPAN, 2);
    value.events = [eventFor(id(0x202), { at: 3 })];
  })],
  [id(0x104), invalidSnapshot(id(0x104), (value) => { value.spans[id(0x301)] = { started: spanStarted(id(0x301), { parent: SPAN, at: 0 }), ended: null }; })],
  [id(0x105), invalidSnapshot(id(0x105), (value) => {
    value.spans[SPAN].ended = spanEnded(SPAN, 2);
    value.spans[id(0x302)] = { started: spanStarted(id(0x302), { parent: SPAN, at: 3 }), ended: null };
  })],
  [id(0x106), invalidSnapshot(id(0x106), (value) => { value.spans[id(0x303)] = { started: spanStarted(id(0x303), { parent: id(0x303) }), ended: null }; })],
  [id(0x107), invalidSnapshot(id(0x107), (value) => { value.spans[id(0x304)] = { started: spanStarted(id(0x304), { remote: id(0x304) }), ended: null }; })],
  [id(0x108), invalidSnapshot(id(0x108), (value) => { value.spans[id(0x305)] = { started: spanStarted(id(0x305), { parent: SPAN, remote: SPAN }), ended: null }; })],
  [id(0x109), invalidSnapshot(id(0x109), (value) => { value.spans[id(0x306)] = { started: spanStarted(id(0x306), { parent: id(0x399) }), ended: null }; })],
  [id(0x10a), invalidSnapshot(id(0x10a), (value) => {
    value.spans[id(0x307)] = { started: spanStarted(id(0x307), { parent: id(0x308) }), ended: null };
    value.spans[id(0x308)] = { started: spanStarted(id(0x308), { parent: id(0x307) }), ended: null };
  })],
  [id(0x10b), invalidSnapshot(id(0x10b), (value) => {
    value.artifacts[artifactUri(OTHER_RUN, id(0x401))] = artifactFor(OTHER_RUN, id(0x401));
  })],
  [id(0x10c), invalidSnapshot(id(0x10c), (value) => {
    value.spans[id(0x309)] = value.spans[SPAN];
    delete value.spans[SPAN];
  })],
  [id(0x10d), invalidSnapshot(id(0x10d), (value) => {
    value.artifacts[artifactUri(value.run_id, id(0x402))] = artifactFor(value.run_id, id(0x403));
  })],
  [id(0x10e), invalidSnapshot(id(0x10e), (value) => { value.through_revision = 2; })]
]);

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
  if (url === `/v1/runs/${STALE_RUN}/snapshot`) {
    staleSnapshotJson = deferred();
    return {
      ok: true,
      status: 200,
      json: () => staleSnapshotJson.promise
    };
  }
  if (url === `/v1/runs/${NEWER_RUN}/snapshot`) {
    return Response.json(snapshot(1, false, NEWER_RUN), {
      headers: { "content-type": "application/vnd.auv.run+json; version=1" }
    });
  }
  const invalidSnapshotRun = [...SNAPSHOT_INVARIANT_CASES.keys()].find((runId) => url === `/v1/runs/${runId}/snapshot`);
  if (invalidSnapshotRun !== undefined) {
    invalidSnapshotRequests.set(invalidSnapshotRun, (invalidSnapshotRequests.get(invalidSnapshotRun) ?? 0) + 1);
    return Response.json(SNAPSHOT_INVARIANT_CASES.get(invalidSnapshotRun), {
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

  emitOpen() {
    const event = new window.Event("open");
    for (const listener of this.listeners.get("open") ?? []) {
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

async function waitMillis(milliseconds) {
  await new Promise((resolveAfterDelay) => window.setTimeout(resolveAfterDelay, milliseconds));
}

async function selectPrimaryRun(label, { open = true } = {}) {
  const selectedSourceCount = sources.length;
  document.getElementById("run-id-input").value = RUN;
  document.getElementById("load-run").click();
  await waitFor(() => sources.length === selectedSourceCount + 1, `${label} selected-run snapshot`);
  const source = sources.at(-1);
  if (open) {
    source.emitOpen();
    await waitFor(() => document.getElementById("conn-label")?.textContent === "live", `${label} stream open`);
  }
  return source;
}

async function assertMalformedCommitRecovers(commit, label) {
  await selectPrimaryRun(label);
  const sourceCount = sources.length;
  const requestCount = primarySnapshotRequests;
  sources.at(-1).emit("commit", JSON.stringify(commit));
  await waitFor(() => sources.length === sourceCount + 1, `${label} snapshot recovery`);
  sources.at(-1).emitOpen();
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
assert(document.getElementById("conn-label")?.textContent === "offline", "stream was labeled live before its open event");
assert(document.getElementById("conn-endpoint")?.textContent === "loading snapshot", "validated snapshot did not remain connecting before open");
sources[0].emitOpen();
await waitFor(() => document.getElementById("conn-label")?.textContent === "live", "initial stream open");
assert(document.getElementById("conn-endpoint")?.textContent === "revision 1", "open stream did not display its snapshot revision");

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
assert(document.getElementById("conn-label")?.textContent === "live", "accepted commit did not keep the stream live");
assert(document.getElementById("conn-endpoint")?.textContent === "revision 2", "accepted commit did not advance the live cursor");

sources[0].emit("gap", JSON.stringify({ requested_after: 2, earliest_available: 3 }));
await waitFor(() => sources.length === 2, "gap snapshot recovery");
sources[1].emitOpen();
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

const reducerInvariantCases = [
  ["end before start", [{ span_ended: spanEnded(SPAN, 0) }]],
  ["event before start", [{ event_occurred: eventFor(id(0x501), { at: 0 }) }]],
  ["event after end", [{ span_ended: spanEnded(SPAN, 2) }, { event_occurred: eventFor(id(0x502), { at: 2 }) }]],
  ["child before parent", [{ span_started: spanStarted(id(0x503), { parent: SPAN, at: 0 }) }]],
  ["child after parent end", [{ span_ended: spanEnded(SPAN, 2) }, { span_started: spanStarted(id(0x504), { parent: SPAN, at: 2 }) }]],
  ["self parent", [{ span_started: spanStarted(id(0x505), { parent: id(0x505) }) }]],
  ["self remote link", [{ span_started: spanStarted(id(0x506), { remote: id(0x506) }) }]],
  ["duplicate parent and remote link", [{ span_started: spanStarted(id(0x507), { parent: SPAN, remote: SPAN }) }]],
  ["missing local parent", [{ span_started: spanStarted(id(0x508), { parent: id(0x599) }) }]],
  ["same-commit parent cycle", [
    { span_started: spanStarted(id(0x509), { parent: id(0x50a) }) },
    { span_started: spanStarted(id(0x50a), { parent: id(0x509) }) }
  ]],
  ["same-commit forward parent", [
    { span_started: spanStarted(id(0x50b), { parent: id(0x50c) }) },
    { span_started: spanStarted(id(0x50c)) }
  ]],
  ["artifact run mismatch", [{ artifact_published: artifactFor(OTHER_RUN, id(0x50d)) }]]
];
for (const [index, [label, invalidFacts]] of reducerInvariantCases.entries()) {
  const validPrefix = { artifact_published: artifactFor(RUN, id(0x600 + index)) };
  await assertMalformedCommitRecovers(commitFor([validPrefix, ...invalidFacts]), label);
}

await selectPrimaryRun("typed unavailable");
{
  const sourceCount = sources.length;
  const requestCount = primarySnapshotRequests;
  sources.at(-1).emit("error", JSON.stringify({ unavailable: { code: "auv.test.unavailable" } }));
  await waitFor(() => sources.length === sourceCount + 1, "typed unavailable recovery");
  sources.at(-1).emitOpen();
  assert(primarySnapshotRequests === requestCount + 1, "typed unavailable did not reload the snapshot");
}

await selectPrimaryRun("transport failure");
{
  const sourceCount = sources.length;
  const requestCount = primarySnapshotRequests;
  sources.at(-1).emitTransportError();
  await waitFor(() => sources.length === sourceCount + 1, "transport failure recovery");
  sources.at(-1).emitOpen();
  assert(primarySnapshotRequests === requestCount + 1, "transport failure did not reload the snapshot");
}

const staleTimerSource = await selectPrimaryRun("stale stability timer setup");
await selectPrimaryRun("commit activity resets flapping budget", { open: false });
staleTimerSource.emitOpen();
staleTimerSource.emitTransportError();
{
  const sourceCount = sources.length;
  sources.at(-1).emitTransportError();
  await waitFor(() => sources.length === sourceCount + 1, "pre-commit transport recovery");
}
sources.at(-1).emit("commit", JSON.stringify(commitFor([
  { artifact_published: artifactFor(RUN, id(0x701)) }
])));
await waitFor(() => document.getElementById("artifact-count")?.textContent === "1", "activity-proving commit");
assert(document.getElementById("conn-label")?.textContent === "live", "activity-proving commit did not mark the stream live");
assert(document.getElementById("conn-endpoint")?.textContent === "revision 3", "activity-proving commit did not advance the live cursor");
for (let attempt = 1; attempt <= 5; attempt += 1) {
  const sourceCount = sources.length;
  sources.at(-1).emitTransportError();
  await waitFor(() => sources.length === sourceCount + 1, `rapid open-error recovery ${attempt}`, 2500);
  sources.at(-1).emitOpen();
}
{
  const sourceCount = sources.length;
  sources.at(-1).emitTransportError();
  await waitFor(
    () => document.getElementById("conn-endpoint")?.textContent === "recovery exhausted: stream unavailable",
    "rapid open-error recovery exhaustion"
  );
  await waitMillis(25);
  assert(sources.length === sourceCount, "rapid open-error flapping constructed another stream after exhaustion");
}

await selectPrimaryRun("quiet recovery cycles");
await waitMillis(STABILITY_SETTLE_MILLIS);
for (let cycle = 1; cycle <= 7; cycle += 1) {
  const sourceCount = sources.length;
  const requestCount = primarySnapshotRequests;
  sources.at(-1).emitTransportError();
  await waitFor(() => sources.length === sourceCount + 1, `quiet recovery cycle ${cycle}`, 2500);
  sources.at(-1).emitOpen();
  await waitMillis(STABILITY_SETTLE_MILLIS);
  assert(primarySnapshotRequests === requestCount + 1, `quiet recovery cycle ${cycle} did not reload the snapshot`);
  assert(document.getElementById("conn-label")?.textContent === "live", `quiet recovery cycle ${cycle} did not return live`);
  assert(document.getElementById("conn-endpoint")?.textContent === "revision 2", `quiet recovery cycle ${cycle} lost the snapshot revision`);
}

sources.at(-1).emit("error", JSON.stringify({ integrity: { code: "auv.test.integrity" } }));
await waitFor(() => document.getElementById("conn-endpoint")?.textContent === "integrity: auv.test.integrity", "typed stream error state");
const requestsAfterIntegrity = primarySnapshotRequests;
await new Promise((resolveAfterDelay) => window.setTimeout(resolveAfterDelay, 650));
assert(primarySnapshotRequests === requestsAfterIntegrity, "permanent typed stream error retried");

await selectPrimaryRun("pre-open retry exhaustion", { open: false });
for (let attempt = 1; attempt <= 5; attempt += 1) {
  const sourceCount = sources.length;
  sources.at(-1).emitTransportError();
  await waitFor(() => sources.length === sourceCount + 1, `pre-open retry ${attempt}`, 2500);
  assert(document.getElementById("conn-label")?.textContent === "offline", `pre-open retry ${attempt} was labeled live`);
}
{
  const sourceCount = sources.length;
  sources.at(-1).emitTransportError();
  await waitFor(
    () => document.getElementById("conn-endpoint")?.textContent === "recovery exhausted: stream unavailable",
    "pre-open retry exhaustion"
  );
  await new Promise((resolveAfterDelay) => window.setTimeout(resolveAfterDelay, 25));
  assert(sources.length === sourceCount, "pre-open retry exhaustion constructed another stream");
}

const input = document.getElementById("run-id-input");
for (const [runId, scenario] of PERMANENT_CASES) {
  input.value = runId;
  document.getElementById("load-run").click();
  await waitFor(() => document.getElementById("conn-endpoint")?.textContent === scenario.message, `${scenario.status} snapshot error state`);
  await new Promise((resolveAfterDelay) => window.setTimeout(resolveAfterDelay, 650));
  assert(permanentSnapshotRequests.get(runId) === 1, `${scenario.status} permanent snapshot error retried`);
}

for (const runId of SNAPSHOT_INVARIANT_CASES.keys()) {
  const sourceCount = sources.length;
  input.value = runId;
  document.getElementById("load-run").click();
  await waitFor(
    () => invalidSnapshotRequests.get(runId) === 1 && document.getElementById("conn-endpoint")?.textContent === "invalid snapshot",
    `snapshot invariant ${runId}`
  );
  await new Promise((resolveAfterDelay) => window.setTimeout(resolveAfterDelay, 25));
  assert(invalidSnapshotRequests.get(runId) === 1, `invalid snapshot ${runId} retried`);
  assert(sources.length === sourceCount, `invalid snapshot ${runId} constructed a stream`);
}

input.value = STALE_RUN;
document.getElementById("load-run").click();
await waitFor(() => staleSnapshotJson !== undefined, "stale snapshot JSON continuation");
input.value = NEWER_RUN;
document.getElementById("load-run").click();
await waitFor(() => sources.at(-1)?.url.includes(NEWER_RUN), "newer selected run stream");
const newerSource = sources.at(-1);
newerSource.emitOpen();
await waitFor(() => document.getElementById("conn-label")?.textContent === "live", "newer selected run open");
staleSnapshotJson.reject(new Error("stale JSON body failed"));
await new Promise((resolveAfterDelay) => window.setTimeout(resolveAfterDelay, 25));
assert(!newerSource.closed, "stale snapshot JSON failure closed the newer selected run stream");
assert(sources.at(-1) === newerSource, "stale snapshot JSON failure replaced the newer selected run stream");
const staleSource = sources.find((source) => source !== newerSource && source.closed);
staleSource.emitOpen();
staleSource.emitTransportError();
await new Promise((resolveAfterDelay) => window.setTimeout(resolveAfterDelay, 25));
assert(!newerSource.closed, "stale stream event closed the newer selected run stream");
assert(sources.at(-1) === newerSource, "stale stream event replaced the newer selected run stream");
assert(document.getElementById("conn-label")?.textContent === "live", "stale stream event changed the newer connection state");
assert(document.getElementById("conn-endpoint")?.textContent === "revision 1", "stale stream event changed the newer revision");
assert(operations.every((operation) => !operation.includes("/write")), `legacy write endpoint used: ${operations.join(", ")}`);
