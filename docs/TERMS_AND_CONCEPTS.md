# Terms and Concepts

This document defines the working vocabulary for AUV runtime recording,
inspection, and future replay work. Terms marked as provisional are design
terms, not stable public API names.

## Trace

A trace is one complete inspectable workflow. Examples include one recipe
execution, one app probe, one app distillation, one validation pass, or one
ad-hoc command invocation.

A trace is the unit that inspection tools load as a whole. It should contain
enough structure to reconstruct what AUV attempted, what happened, what state
was observed, and which captured materials support that account.

## Run

A run is the user-visible top-level record for a trace. The `run_id` is the
stable handle used by CLI commands, storage paths, and viewer APIs.

For local storage, a run is expected to live under `.auv/runs/{run_id}/`.
Internally, a run may also carry an OpenTelemetry-compatible trace identifier so
the recorded data can later be exported to OTLP without treating the human
readable `run_id` as the telemetry trace id.

## Span

A span is a timed unit of work inside a run. Spans form a tree through
`parent_span_id`.

Expected span levels include workflow phases, case-matrix cases, recipe steps,
command invocations, and driver actions. A single ad-hoc command can be a run
with one root span and one command span. A recipe execution should be one run
with child spans for its steps and command invocations.

## Event

An event is a timestamped occurrence attached to a span. Events are small and
append-friendly. They should describe what happened, not carry large payloads.

Examples include `command.resolved`, `driver.invoke`, `action.started`,
`artifact.captured`, `assertion.passed`, and `assertion.failed`.

## Artifact

An artifact is persisted inspection material produced during a run. Artifacts
may be files, structured JSON documents, images, reports, logs, or media.

Examples include screenshots, click-overlay images, accessibility snapshots,
driver input/output JSON, distillation reports, validation reports, and video
segments.

Artifacts are referenced from spans or events by metadata. Large payloads should
remain as files or blobs rather than being embedded directly in events.

## Inspect Server

The inspect server is a read-only HTTP and WebSocket access layer over stored
and same-process live run data. It is not the runtime execution API.

The server exists so browser viewers, Android WebViews, IDE integrations, and
other tools can list runs, fetch run structure, load artifacts, and subscribe to
live run events.

The default CLI endpoint is `127.0.0.1:8765`. A standalone `inspect serve`
process can read historical runs from `.auv/runs/`; live streaming requires the
runtime and server to share the same in-process event sink.

Replay and mutation APIs are out of scope for the first inspect-server design.

## Viewer

A viewer is any UI that renders run data from the inspect server or directly
from the run store. The first viewer is expected to be browser-based so it can
work across desktop, remote, and mobile contexts.

The viewer should render spans, events, and artifacts as an inspectable
timeline.
