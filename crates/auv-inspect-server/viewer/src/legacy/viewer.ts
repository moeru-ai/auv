// @ts-nocheck
// NOTICE(inspect-viewer-vite-migration): this file is a mechanical port of the
// legacy inline viewer script. Internal type checking is deferred until the
// script is split into typed Vue components; the module boundary remains typed.

export function mountLegacyViewer(document: Document): void {
  const window = document.defaultView;
  if (window === null) {
    throw new Error("viewer document has no default window");
  }

  const originalAddEventListener = window.addEventListener.bind(window);
  void originalAddEventListener;

  copiedViewerMain(document, window);
}

function copiedViewerMain(document: Document, window: Window): void {
  "use strict";

  function el(tag, props, children) {
    const node = document.createElement(tag);
    if (props) {
      for (const key in props) {
        if (key === "className") node.className = props[key];
        else if (key === "onClick") node.addEventListener("click", props[key]);
        else if (key === "dataset") Object.assign(node.dataset, props[key]);
        else node.setAttribute(key, props[key]);
      }
    }
    if (children) for (const child of children) {
      if (child == null) continue;
      node.appendChild(typeof child === "string" ? document.createTextNode(child) : child);
    }
    return node;
  }

  function midTrunc(s, head, tail) {
    head = head || 22; tail = tail || 8;
    if (!s) return "";
    if (s.length <= head + tail + 1) return s;
    return s.slice(0, head) + "…" + s.slice(-tail);
  }

  function fmtDuration(startMs, endMs) {
    if (endMs == null) return "—";
    const ms = Math.max(0, endMs - startMs);
    if (ms < 1000) return ms + "ms";
    if (ms < 60000) return (ms / 1000).toFixed(2) + "s";
    const s = Math.floor(ms / 1000);
    return Math.floor(s / 60) + "m" + (s % 60) + "s";
  }

  function pillFor(statusCode, state) {
    if (state === "running") return { cls: "s-running", label: "running" };
    if (statusCode === "ok") return { cls: "s-validated", label: "ok" };
    if (statusCode === "error") return { cls: "s-failed", label: "error" };
    return { cls: "s-frozen", label: statusCode || "unset" };
  }

  function makeStatusPill(statusCode, state) {
    const p = pillFor(statusCode, state);
    return el("span", { className: "status-pill " + p.cls }, [
      el("span", { className: "dot" }),
      p.label,
    ]);
  }

  function makeTypeChip(runType) {
    return el("span", { className: "run-type-chip" }, [runType || "—"]);
  }

  function fmtSeconds(ms) {
    if (ms == null) return "—";
    return (Math.max(0, ms) / 1000).toFixed(2) + "s";
  }

  function statusLabel(statusCode, state) {
    if (state === "running") return "running";
    if (statusCode === "ok") return "ok";
    if (statusCode === "error") return "error";
    return statusCode || "unset";
  }

  function statusColor(statusCode, state) {
    if (state === "running") return "var(--running)";
    if (statusCode === "ok") return "var(--validated)";
    if (statusCode === "error") return "var(--failed)";
    if (statusCode === "unset") return "var(--fg-3)";
    return "var(--fg-3)";
  }

  function spanGlyph(span) {
    if (span.state === "running") {
      return { glyph: "●", color: "var(--running)", pulse: true };
    }
    if (span.status_code === "ok") {
      return { glyph: "●", color: "var(--validated)", pulse: false };
    }
    if (span.status_code === "error") {
      return { glyph: "×", color: "var(--failed)", pulse: false };
    }
    if (span.status_code === "unset") {
      return { glyph: "○", color: "var(--fg-3)", pulse: false };
    }
    return { glyph: "·", color: "var(--fg-3)", pulse: false };
  }

  function traceCrumb(run) {
    const trace = run && run.trace_id ? String(run.trace_id).slice(0, 12) + "…" : "—";
    const base = (run && run.run_type ? run.run_type : "—") + " · trace_id=" + trace;
    const readSide = summarizeRunReadSide(run);
    return readSide ? base + " · " + readSide : base;
  }

  function quantity(count, singular, plural) {
    return count + " " + (count === 1 ? singular : plural);
  }

  function verificationCount(run) {
    return Array.isArray(run && run.verifications) ? run.verifications.length : 0;
  }

  function observationSnapshotCount(run) {
    return Array.isArray(run && run.observation_snapshots)
      ? run.observation_snapshots.length
      : 0;
  }

  function summarizeRunReadSide(run) {
    const parts = [];
    const verifications = verificationCount(run);
    const observations = observationSnapshotCount(run);
    if (verifications) parts.push(quantity(verifications, "verification", "verifications"));
    if (observations) parts.push(quantity(observations, "observation", "observations"));
    return parts.join(" · ");
  }

  function runSummaryText(run) {
    const summary = run && run.summary ? run.summary : "";
    const readSide = summarizeRunReadSide(run);
    if (summary && readSide) return summary + " · " + readSide;
    return summary || readSide || "—";
  }

  function summarizeValues(values, limit) {
    const max = limit || 3;
    if (!values.length) return "—";
    const head = values.slice(0, max).join(" · ");
    if (values.length <= max) return head;
    return head + " +" + (values.length - max);
  }

  function formatVerificationMethod(method) {
    if (!method) return "unknown";
    if (typeof method === "string") return method;
    if (typeof method.kind === "string") {
      if (method.kind === "custom" && method.name) return "custom:" + method.name;
      return method.kind;
    }
    return stringifyAttr(method);
  }

  function mergeRunDetail(previous, incoming) {
    if (!incoming || typeof incoming !== "object") return incoming;
    const merged = Object.assign({}, incoming);
    if (!Array.isArray(merged.verifications) && previous && Array.isArray(previous.verifications)) {
      merged.verifications = previous.verifications.slice();
    }
    if (
      !Array.isArray(merged.observation_snapshots)
      && previous
      && Array.isArray(previous.observation_snapshots)
    ) {
      merged.observation_snapshots = previous.observation_snapshots.slice();
    }
    if (!merged.view_parser && previous && previous.view_parser) {
      merged.view_parser = previous.view_parser;
    }
    if (!merged.view_parser_summary && previous && previous.view_parser_summary) {
      merged.view_parser_summary = previous.view_parser_summary;
    }
    return merged;
  }


  const VIEW_MEMORY_ARTIFACT_ROLE = "view-memory";
  const PLAYLIST_SELECT_RESULT_ARTIFACT_ROLE = "netease-playlist-select-result";

  function clearViewParserProof() {
    const panel = document.getElementById("view-parser-proof");
    if (!panel) return;
    panel.innerHTML = "";
    panel.hidden = true;
  }

  function clearNeteaseSelectProofHint() {
    const panel = document.getElementById("netease-select-proof-hint");
    if (!panel) return;
    panel.innerHTML = "";
    panel.hidden = true;
  }

  function neteasePlaylistSelectProofVisible(run, spans, artifacts) {
    if (!run || !Array.isArray(artifacts) || !Array.isArray(spans)) return false;
    const hasArtifact = artifacts.some(function (artifact) {
      return artifact && artifact.role === PLAYLIST_SELECT_RESULT_ARTIFACT_ROLE;
    });
    if (!hasArtifact) return false;
    const rootSpan = spans.find(function (span) {
      return span && (span.parent_span_id == null || span.parent_span_id === "");
    });
    return !!(rootSpan && rootSpan.name === "auv.netease.playlist.select");
  }

  function renderNeteaseSelectProofHint(run, spans, artifacts) {
    const panel = document.getElementById("netease-select-proof-hint");
    if (!panel) return;
    if (!neteasePlaylistSelectProofVisible(run, spans, artifacts)) {
      clearNeteaseSelectProofHint();
      return;
    }
    panel.innerHTML = "";
    panel.hidden = false;
    panel.appendChild(el("span", { className: "netease-select-proof-hint-label" }, [
      "NetEase playlist select proof",
    ]));
    panel.appendChild(el("span", { className: "netease-select-proof-hint-secondary" }, [
      "Hermetic app pack proof — packaging lane only. Supplemental note for app-pack evidence only.",
    ]));
  }

  function hasViewParserProof(run) {
    const viewParser = run && run.view_parser;
    return !!(
      viewParser
      && Array.isArray(viewParser.resolution_summaries)
      && viewParser.resolution_summaries.length > 0
    );
  }

  function outcomePillClass(outcome) {
    const value = String(outcome || "").toLowerCase();
    if (value === "reacquired") return "s-validated";
    if (value === "stale") return "s-candidate";
    if (value === "not_found" || value === "failed") return "s-failed";
    return "s-frozen";
  }

  function verificationPillClass(status) {
    const value = String(status || "").toLowerCase();
    if (value === "passed") return "s-validated";
    if (value === "failed") return "s-failed";
    return "s-frozen";
  }

  function makeProofPill(label, cls) {
    return el("span", { className: "status-pill " + cls }, [
      el("span", { className: "dot" }),
      label || "—",
    ]);
  }

  function pairViewParserProofCards(run) {
    const viewParser = run && run.view_parser;
    if (!viewParser) return [];
    const summaries = Array.isArray(viewParser.resolution_summaries)
      ? viewParser.resolution_summaries
      : [];
    const selectResults = Array.isArray(viewParser.select_results)
      ? viewParser.select_results
      : [];
    const pairs = [];
    for (let index = 0; index < summaries.length; index++) {
      pairs.push({
        summary: summaries[index],
        selectResult: index < selectResults.length ? selectResults[index] : null,
      });
    }
    return pairs;
  }

  function artifactsWithRole(role) {
    return (state.artifacts || []).filter(function (artifact) {
      return artifact && artifact.role === role;
    });
  }

  function reacquireRecordCompositeKey(record) {
    if (!record) return "";
    return [
      record.scope_id || "",
      record.outcome || "",
      String(record.observation_count != null ? record.observation_count : 0),
      record.strategy_used || "",
      record.stale_reason || "",
    ].join("|");
  }

  function resolutionReacquireCompositeKey(summary, selectResult) {
    const resolution = summary && summary.resolution ? summary.resolution : {};
    return reacquireRecordCompositeKey({
      scope_id: resolution.span_scope_id || "",
      outcome: resolution.outcome || "",
      observation_count: resolution.observation_count != null
        ? resolution.observation_count
        : 0,
      strategy_used: resolution.strategy_used || null,
      stale_reason: resolution.stale_reason || null,
    });
  }

  function resolveSelectResultArtifactForPair(pairIndex) {
    const artifacts = artifactsWithRole(PLAYLIST_SELECT_RESULT_ARTIFACT_ROLE);
    if (pairIndex < 0 || pairIndex >= artifacts.length) return null;
    return artifacts[pairIndex];
  }

  function resolveUniqueViewMemoryArtifact(summary) {
    const viewMemoryArtifacts = artifactsWithRole(VIEW_MEMORY_ARTIFACT_ROLE);
    if (!viewMemoryArtifacts.length) return null;

    const memory = summary && summary.memory ? summary.memory : {};
    const resolution = summary && summary.resolution ? summary.resolution : {};
    const memoryId = memory.memory_id;
    const scopeId = resolution.span_scope_id;

    if (memoryId && scopeId) {
      const run = state.activeRun;
      const memoryWrites = run && run.view_parser && Array.isArray(run.view_parser.memory_writes)
        ? run.view_parser.memory_writes
        : [];
      const matches = memoryWrites.filter(function (write) {
        return write && write.memory_id === memoryId && write.scope_id === scopeId;
      });
      if (matches.length !== 1) return null;
      const writeIndex = memoryWrites.indexOf(matches[0]);
      if (writeIndex < 0
        || writeIndex >= viewMemoryArtifacts.length
        || memoryWrites.length !== viewMemoryArtifacts.length) {
        return null;
      }
      return viewMemoryArtifacts[writeIndex];
    }

    if (viewMemoryArtifacts.length === 1) return viewMemoryArtifacts[0];
    return null;
  }

  function resolveUniqueSpanIdByName(spanName) {
    if (!spanName) return null;
    const matches = (state.spans || []).filter(function (span) {
      return span && span.name === spanName;
    });
    if (matches.length !== 1) return null;
    return matches[0].span_id;
  }

  function resolveUniqueReacquireSpanId(summary, selectResult, run) {
    const viewParser = run && run.view_parser;
    const reacquisitions = viewParser && Array.isArray(viewParser.reacquisitions)
      ? viewParser.reacquisitions
      : [];
    const key = resolutionReacquireCompositeKey(summary, selectResult);
    if (!key) return null;
    const matches = reacquisitions.filter(function (record) {
      return reacquireRecordCompositeKey(record) === key;
    });
    if (matches.length !== 1) return null;
    return resolveUniqueSpanIdByName(matches[0].span_name);
  }

  function jumpToArtifactTarget(artifact) {
    if (!artifact) return;
    showArtifactPanel(true);
    state.activeArtifactRoleFilter = artifact.role || null;
    state.activeArtifactKey = artifactKey(artifact);
    renderArtifactList(state.artifacts);
    renderArtifactPreview(findArtifact(state.activeArtifactKey), state.artifacts);
    const list = document.getElementById("artifact-list");
    if (!list || !state.activeArtifactKey) return;
    const row = list.querySelector(
      '[data-artifact-key="' + state.activeArtifactKey + '"]'
    );
    if (row && row.scrollIntoView) row.scrollIntoView({ block: "nearest" });
  }

  function jumpToSelectResultArtifactForPair(pairIndex) {
    jumpToArtifactTarget(resolveSelectResultArtifactForPair(pairIndex));
  }

  function jumpToUniqueViewMemoryArtifact(summary) {
    jumpToArtifactTarget(resolveUniqueViewMemoryArtifact(summary));
  }

  function jumpToSpanId(spanId) {
    if (!spanId) return;
    state.activeSpanId = spanId;
    const span = findSpan(spanId);
    const preferredArtifactKey = preferredArtifactKeyForSpan(spanId, state.artifacts);
    if (preferredArtifactKey) state.activeArtifactKey = preferredArtifactKey;
    renderSpanTree(state.activeRun, state.spans);
    renderSpanDetail(span);
    renderArtifactList(state.artifacts);
    renderArtifactPreview(findArtifact(state.activeArtifactKey), state.artifacts);
    const body = document.getElementById("main-body");
    if (!body) return;
    const row = body.querySelector('[data-span-id="' + spanId + '"]');
    if (row && row.scrollIntoView) row.scrollIntoView({ block: "nearest" });
  }

  function scrollToProofCardSection(cardEl, sectionKey) {
    if (!cardEl || !sectionKey) return;
    const section = cardEl.querySelector('[data-proof-section="' + sectionKey + '"]');
    if (!section) return;
    if (section.scrollIntoView) section.scrollIntoView({ block: "nearest" });
    section.classList.add("view-parser-proof-section-highlight");
    window.setTimeout(function () {
      section.classList.remove("view-parser-proof-section-highlight");
    }, 1200);
  }

  function renderViewParserDiagnosticLinks(summary, selectResult, pairIndex, run, cardEl) {
    const chips = [];

    const knownLimits = selectResult && Array.isArray(selectResult.known_limits)
      ? selectResult.known_limits
      : [];
    if (knownLimits.length > 0) {
      chips.push(el("button", {
        className: "chip",
        type: "button",
        onClick: function () { scrollToProofCardSection(cardEl, "known_limits"); },
      }, ["known limits"]));
    }

    if (resolveSelectResultArtifactForPair(pairIndex)) {
      chips.push(el("button", {
        className: "chip",
        type: "button",
        onClick: function () { jumpToSelectResultArtifactForPair(pairIndex); },
      }, [PLAYLIST_SELECT_RESULT_ARTIFACT_ROLE]));
    }

    if (resolveUniqueViewMemoryArtifact(summary)) {
      chips.push(el("button", {
        className: "chip",
        type: "button",
        onClick: function () { jumpToUniqueViewMemoryArtifact(summary); },
      }, [VIEW_MEMORY_ARTIFACT_ROLE]));
    }

    const reacquireSpanId = resolveUniqueReacquireSpanId(summary, selectResult, run);
    if (reacquireSpanId) {
      chips.push(el("button", {
        className: "chip",
        type: "button",
        onClick: function () { jumpToSpanId(reacquireSpanId); },
      }, ["reacquire span"]));
    }

    const replay = summary && summary.replay ? summary.replay : {};
    const stepNames = Array.isArray(replay.step_names) ? replay.step_names : [];
    for (const stepName of stepNames) {
      const stepSpanId = resolveUniqueSpanIdByName(stepName);
      if (!stepSpanId) continue;
      chips.push(el("button", {
        className: "chip",
        type: "button",
        onClick: function () { jumpToSpanId(stepSpanId); },
      }, ["replay: " + stepName]));
    }

    if (!chips.length) return null;
    return el("div", { className: "view-parser-diagnostic-links" }, chips);
  }

  function formatViewParserLineageRow(memory, runId) {
    const memoryId = memory && memory.memory_id ? memory.memory_id : "—";
    const sourceRunId = memory && memory.source_run_id ? memory.source_run_id : "—";
    const currentRunId = runId || "—";
    return memoryId + " · " + sourceRunId + " · " + currentRunId;
  }

  function renderResolutionSummaryCard(summary, selectResult, pairIndex, run) {
    const runId = (run && run.run_id) || "";
    const identity = summary.identity || {};
    const memory = summary.memory || {};
    const resolution = summary.resolution || {};
    const replay = summary.replay || {};
    const verification = summary.verification || {};
    const geometry = summary.geometry_note || {};
    const stepNames = Array.isArray(replay.step_names) ? replay.step_names.join(", ") : "";
    const knownLimits = selectResult && Array.isArray(selectResult.known_limits)
      ? selectResult.known_limits
      : [];

    const header = el("div", { className: "view-parser-proof-card-head" }, [
      el("span", { className: "view-parser-proof-query" }, [summary.query || "—"]),
      makeProofPill(resolution.outcome || "—", outcomePillClass(resolution.outcome)),
      makeProofPill(verification.status || "—", verificationPillClass(verification.status)),
    ]);

    const cardEl = el("div", { className: "view-parser-proof-card" }, [header]);

    const diagnosticLinks = renderViewParserDiagnosticLinks(
      summary,
      selectResult,
      pairIndex,
      run,
      cardEl
    );
    if (diagnosticLinks) cardEl.appendChild(diagnosticLinks);

    cardEl.appendChild(el("div", { className: "view-parser-proof-geometry" }, [
      formatViewParserLineageRow(memory, runId),
    ]));

    function section(title, card, proofSection) {
      const sectionEl = el("div", { className: "view-parser-proof-section" }, [
        el("div", { className: "view-parser-proof-section-label" }, [title]),
        card,
      ]);
      if (proofSection) sectionEl.dataset.proofSection = proofSection;
      return sectionEl;
    }

    const sections = [
      section("Identity", renderKeyValueSummary([
        ["query", summary.query],
        ["label", identity.label],
        ["section_kind", identity.section_kind],
        ["anchor_id", identity.anchor_id],
      ])),
      section("Memory", renderKeyValueSummary([
        ["present", memory.present != null ? String(memory.present) : null],
        ["memory_id", memory.memory_id],
        ["source_run_id", memory.source_run_id],
        ["last_reconstructed_at_millis", memory.last_reconstructed_at_millis],
        ["anchor_count", memory.anchor_count],
      ])),
      section("Resolution", renderKeyValueSummary([
        ["outcome", resolution.outcome],
        ["strategy_used", resolution.strategy_used],
        ["observation_count", resolution.observation_count],
        ["stale_reason", resolution.stale_reason],
        ["span_scope_id", resolution.span_scope_id],
      ])),
      section("Replay", renderKeyValueSummary([
        ["step_names", stepNames || null],
        ["skipped_rescan_replay", replay.skipped_rescan_replay != null
          ? String(replay.skipped_rescan_replay)
          : null],
      ])),
      section("Verification", renderKeyValueSummary([
        ["status", verification.status],
        ["method", verification.method],
      ])),
    ];
    if (knownLimits.length) {
      sections.push(section("Known limits", el("div", { className: "view-parser-proof-geometry" }, [
        knownLimits.join(" · "),
      ]), "known_limits"));
    }

    const artifactChips = [];
    if (resolveSelectResultArtifactForPair(pairIndex)) {
      artifactChips.push(el("button", {
        className: "chip",
        type: "button",
        onClick: function () {
          jumpToSelectResultArtifactForPair(pairIndex);
        },
      }, [PLAYLIST_SELECT_RESULT_ARTIFACT_ROLE]));
    }
    if (resolveUniqueViewMemoryArtifact(summary)) {
      artifactChips.push(el("button", {
        className: "chip",
        type: "button",
        onClick: function () { jumpToUniqueViewMemoryArtifact(summary); },
      }, [VIEW_MEMORY_ARTIFACT_ROLE]));
    }
    if (artifactChips.length) {
      sections.push(section("Artifacts", el("div", { className: "filter-chips" }, artifactChips)));
    }

    for (const sec of sections) cardEl.appendChild(sec);
    if (geometry.note || geometry.has_ephemeral_target_bounds != null) {
      cardEl.appendChild(el("div", { className: "view-parser-proof-geometry" }, [
        "geometry: ephemeral_bounds="
          + String(!!geometry.has_ephemeral_target_bounds)
          + (geometry.note ? " · " + geometry.note : ""),
      ]));
    }
    return cardEl;
  }

  function renderViewParserProof(run) {
    const panel = document.getElementById("view-parser-proof");
    if (!panel) return;
    if (!hasViewParserProof(run)) {
      clearViewParserProof();
      return;
    }
    panel.innerHTML = "";
    panel.hidden = false;
    let pairIndex = 0;
    for (const pair of pairViewParserProofCards(run)) {
      panel.appendChild(
        renderResolutionSummaryCard(pair.summary, pair.selectResult, pairIndex, run)
      );
      pairIndex++;
    }
  }

  function appendJsonSummaryRow(grid, key, value) {
    grid.appendChild(el("span", { className: "k" }, [key]));
    grid.appendChild(el("span", { className: "v" }, [value != null && value !== "" ? String(value) : "—"]));
  }

  async function refreshViewParserProofFromRunDetail(runId) {
    try {
      const response = await fetch("/runs/" + encodeURIComponent(runId));
      if (!response.ok) throw new Error("/runs/" + runId + " HTTP " + response.status);
      const run = await response.json();
      if (state.activeRunId !== runId) return;
      state.activeRun = mergeRunDetail(state.activeRun, run);
      const runIndex = state.runs.findIndex(function (candidate) {
        return candidate.run_id === runId;
      });
      if (runIndex >= 0) state.runs[runIndex] = state.activeRun;
      setMainHeader(state.activeRun, false);
      renderViewParserProof(state.activeRun);
      renderNeteaseSelectProofHint(state.activeRun, state.spans, state.artifacts);
      renderRunList();
    } catch (err) {
      if (state.activeRunId !== runId) return;
      clearViewParserProof();
      clearNeteaseSelectProofHint();
    }
  }

  function runListFilterDefinitions() {
    return [
      { key: "failed", label: "failed" },
      { key: "stale", label: "stale" },
      { key: "limits", label: "limits" },
    ];
  }

  function runMatchesListFilterKey(run, key) {
    const summary = (run && run.view_parser_summary) || {};
    if (key === "failed") {
      return summary.latest_verification_status === "failed"
        || (run && run.status_code === "error");
    }
    if (key === "stale") {
      return summary.latest_outcome === "stale";
    }
    if (key === "limits") {
      return summary.has_known_limits === true;
    }
    return true;
  }

  function runMatchesListFilters(run, filters) {
    if (!filters || filters.size === 0) return true;
    for (const key of filters) {
      if (!runMatchesListFilterKey(run, key)) return false;
    }
    return true;
  }

  function visibleRunsForList(runs, filters) {
    const list = Array.isArray(runs) ? runs : [];
    return list.filter(function (run) {
      return runMatchesListFilters(run, filters);
    });
  }

  function activeRunHiddenByFilters(activeRunId, runs, filters) {
    if (!activeRunId || !filters || filters.size === 0) return false;
    const list = Array.isArray(runs) ? runs : [];
    if (!list.some(function (run) {
      return run && run.run_id === activeRunId;
    })) {
      return false;
    }
    return !visibleRunsForList(runs, filters).some(function (run) {
      return run && run.run_id === activeRunId;
    });
  }

  function renderRunListFilterChips(options) {
    const enabled = !options || options.enabled !== false;
    const container = document.getElementById("run-list-filters");
    if (!container) return;
    container.innerHTML = "";
    const allActive = state.runListFilters.size === 0;
    container.appendChild(el("button", {
      className: "chip" + (allActive && enabled ? " active" : ""),
      type: "button",
      disabled: !enabled,
      onClick: function () {
        if (enabled) clearRunListFilters();
      },
    }, ["all"]));
    for (const def of runListFilterDefinitions()) {
      const isActive = state.runListFilters.has(def.key);
      container.appendChild(el("button", {
        className: "chip" + (isActive && enabled ? " active" : ""),
        type: "button",
        disabled: !enabled,
        onClick: function () {
          if (enabled) toggleRunListFilter(def.key);
        },
      }, [def.label]));
    }
  }

  function toggleRunListFilter(key) {
    if (state.runListFilters.has(key)) {
      state.runListFilters.delete(key);
    } else {
      state.runListFilters.add(key);
    }
    renderRunListFilterChips({ enabled: true });
    renderRunList();
  }

  function clearRunListFilters() {
    state.runListFilters.clear();
    renderRunListFilterChips({ enabled: true });
    renderRunList();
  }

  function resetRunListFilterUiOnLoadFailure() {
    const banner = document.getElementById("run-list-filter-banner");
    if (banner) {
      banner.hidden = true;
      banner.innerHTML = "";
    }
    const count = document.getElementById("run-count");
    if (count) count.textContent = "—";
    renderRunListFilterChips({ enabled: false });
  }

  function renderRunListFilterBanner() {
    const banner = document.getElementById("run-list-filter-banner");
    if (!banner) return;
    if (!activeRunHiddenByFilters(state.activeRunId, state.runs, state.runListFilters)) {
      banner.hidden = true;
      banner.innerHTML = "";
      return;
    }
    banner.hidden = false;
    banner.innerHTML = "";
    banner.appendChild(el("span", null, ["Selected run hidden by filters."]));
    banner.appendChild(el("button", {
      className: "chip",
      type: "button",
      onClick: function () { clearRunListFilters(); },
    }, ["Clear filters"]));
  }

  function renderViewParserListBadges(summary) {
    if (!summary || !summary.has_proof) return null;
    const badges = [];
    if (summary.latest_outcome) {
      badges.push(
        makeProofPill(summary.latest_outcome, outcomePillClass(summary.latest_outcome))
      );
    }
    if (summary.latest_verification_status) {
      badges.push(
        makeProofPill(
          summary.latest_verification_status,
          verificationPillClass(summary.latest_verification_status)
        )
      );
    }
    if (summary.has_known_limits) {
      badges.push(makeProofPill("limits", "s-candidate"));
    }
    if (summary.resolution_count > 1) {
      badges.push(el("span", { className: "run-type-chip" }, ["×" + summary.resolution_count]));
    }
    if (badges.length === 0) return null;
    return el("div", { className: "row-proof-badges" }, badges);
  }

  function renderRun(run, isActive, onSelect) {
    const headerRow = el("div", { className: "row-head" }, [
      makeStatusPill(run.status_code, run.state),
      makeTypeChip(run.run_type),
    ]);
    const idRow = el("div", { className: "row-id" }, [midTrunc(run.run_id, 24, 8)]);
    const proofBadges = renderViewParserListBadges(run.view_parser_summary);
    const summary = el("div", { className: "row-summary" }, [runSummaryText(run)]);
    const meta = el("div", { className: "row-meta" }, [
      el("span", null, [fmtDuration(run.started_at_millis, run.finished_at_millis)]),
      el("span", { className: "right" }, [(run.run_type || "").toLowerCase()]),
    ]);
    const rowChildren = [headerRow, idRow];
    if (proofBadges) rowChildren.push(proofBadges);
    rowChildren.push(summary, meta);
    const row = el(
      "button",
      {
        className: "run-row" + (isActive ? " active" : ""),
        dataset: { runId: run.run_id },
        onClick: function () { onSelect(run.run_id); },
      },
      rowChildren
    );
    return row;
  }

  // Sparkle sprite reused by the empty-state span detail. Served from
  // /assets/sparkle.svg by auv-inspect-server's design asset route; both
  // the HTML scaffold and dynamic re-renders point at the same URL.
  const SPARKLE_IMG = '<img class="sparkle" src="/assets/sparkle.svg" alt="">';

  const state = {
    runs: [],
    activeRunId: null,
    activeRun: null,
    spans: [],
    events: [],
    artifacts: [],
    activeSpanId: null,
    activeArtifactKey: null,
    activeArtifactRoleFilter: null,
    activeSurfaceNodeArtifactKey: null,
    activeSurfaceNodeKey: null,
    activeEvidenceRequestId: 0,
    fetchedAt: null,
    // C.4: WebSocket live stream state. ws is the open socket (or
    // null); streamRunId is the run id the socket is bound to;
    // retryScheduled flips after a single reconnect attempt so we
    // don't spin on a flaky server.
    ws: null,
    streamRunId: null,
    retryScheduled: false,
    runListFilters: new Set(),
  };

  function renderRunList() {
    const list = document.getElementById("run-list");
    list.innerHTML = "";
    const total = state.runs.length;
    const visible = visibleRunsForList(state.runs, state.runListFilters);
    const countEl = document.getElementById("run-count");
    if (countEl) {
      if (state.runListFilters.size === 0) {
        countEl.textContent = String(total);
      } else {
        countEl.textContent = visible.length + " / " + total;
      }
    }
    if (total === 0) {
      list.appendChild(el("div", { className: "run-row empty" }, ["no runs recorded yet."]));
      renderRunListFilterBanner();
      return;
    }
    if (visible.length === 0) {
      list.appendChild(el("div", { className: "run-row empty" }, ["no runs match filters."]));
      renderRunListFilterBanner();
      return;
    }
    for (const run of visible) {
      list.appendChild(renderRun(run, run.run_id === state.activeRunId, selectRun));
    }
    renderRunListFilterBanner();
  }

  function setMainHeader(run, loading) {
    document.getElementById("main-label").textContent =
      run ? "Run · " + run.run_id : "Run · —";
    document.getElementById("main-crumb").textContent = run ? traceCrumb(run) : "";
    const status = document.getElementById("main-status");
    status.innerHTML = "";
    if (run) status.appendChild(makeStatusPill(run.status_code, run.state));
    if (loading) document.getElementById("main-crumb").textContent = "loading…";
  }

  function renderPlaceholder(lines) {
    const body = document.getElementById("main-body");
    body.className = "placeholder";
    body.innerHTML = "";
    for (const line of lines) {
      body.appendChild(el("div", null, [line]));
    }
  }

  async function selectRun(runId) {
    closeStream();
    clearViewParserProof();
    clearNeteaseSelectProofHint();
    state.activeRunId = runId;
    state.activeRun = state.runs.find(function (r) { return r.run_id === runId; }) || null;
    state.spans = [];
    state.events = [];
    state.artifacts = [];
    state.activeSpanId = null;
    state.activeArtifactKey = null;
    state.activeArtifactRoleFilter = null;
    state.activeSurfaceNodeArtifactKey = null;
    state.activeSurfaceNodeKey = null;
    const active = state.runs.find(function (r) { return r.run_id === runId; });
    setMainHeader(active, true);
    if (active) {
      renderPlaceholder([
        "loading " + active.run_id + "…",
        "fetching /runs/:id, /runs/:id/spans, /runs/:id/events, /runs/:id/artifacts.",
      ]);
    }
    showEventsRail(!!active);
    showArtifactPanel(!!active);
    renderSpanDetail(null);
    renderEventList([], null);
    renderArtifactList([]);
    renderArtifactPreview(null, []);
    renderRunList();
    await loadRunDetail(runId);
    if (state.activeRunId === runId && state.activeRun && isRunning(state.activeRun)) {
      openStream(runId);
    }
  }

  async function loadRunDetail(runId) {
    try {
      const [runResponse, spansResponse, eventsResponse, artifactsResponse] = await Promise.all([
        fetch("/runs/" + encodeURIComponent(runId)),
        fetch("/runs/" + encodeURIComponent(runId) + "/spans"),
        fetch("/runs/" + encodeURIComponent(runId) + "/events"),
        fetch("/runs/" + encodeURIComponent(runId) + "/artifacts"),
      ]);
      if (!runResponse.ok) throw new Error("/runs/" + runId + " HTTP " + runResponse.status);
      if (!spansResponse.ok) throw new Error("/runs/" + runId + "/spans HTTP " + spansResponse.status);
      if (!eventsResponse.ok) throw new Error("/runs/" + runId + "/events HTTP " + eventsResponse.status);
      if (!artifactsResponse.ok) throw new Error("/runs/" + runId + "/artifacts HTTP " + artifactsResponse.status);
      const run = await runResponse.json();
      const spans = await spansResponse.json();
      const events = await eventsResponse.json();
      const artifacts = await artifactsResponse.json();
      if (state.activeRunId !== runId) return;
      state.activeRun = mergeRunDetail(state.activeRun, run);
      const runIndex = state.runs.findIndex(function (candidate) { return candidate.run_id === runId; });
      if (runIndex >= 0) state.runs[runIndex] = state.activeRun;
      state.spans = Array.isArray(spans) ? spans : [];
      state.events = Array.isArray(events) ? events : [];
      state.artifacts = Array.isArray(artifacts) ? artifacts : [];
      state.activeArtifactKey = defaultArtifactKey(state.artifacts);
      setMainHeader(state.activeRun, false);
      renderRunList();
      renderSpanTree(state.activeRun, state.spans);
      renderSpanDetail(state.activeSpanId ? findSpan(state.activeSpanId) : null);
      renderEventList(state.events, state.activeRun);
      renderArtifactList(state.artifacts);
      renderArtifactPreview(findArtifact(state.activeArtifactKey), state.artifacts);
      renderViewParserProof(state.activeRun);
      renderNeteaseSelectProofHint(state.activeRun, state.spans, state.artifacts);
    } catch (err) {
      if (state.activeRunId !== runId) return;
      clearViewParserProof();
      clearNeteaseSelectProofHint();
      setConnection(false, "/runs/:id (" + err.message + ")");
      renderPlaceholder([
        "failed to load run detail.",
        err.message,
      ]);
      showEventsRail(false);
      showArtifactPanel(false);
    }
  }

  function findSpan(spanId) {
    return state.spans.find(function (s) { return s.span_id === spanId; }) || null;
  }

  function showEventsRail(visible) {
    const rail = document.getElementById("events-rail");
    if (visible) rail.removeAttribute("hidden");
    else rail.setAttribute("hidden", "");
  }

  function renderSpanDetail(span) {
    const node = document.getElementById("span-detail");
    node.innerHTML = "";
    if (!span) {
      const run = state.activeRun;
      const verifications = run && Array.isArray(run.verifications) ? run.verifications : [];
      const observations =
        run && Array.isArray(run.observation_snapshots) ? run.observation_snapshots : [];
      if (run && (verifications.length || observations.length)) {
        node.className = "span-detail";
        node.appendChild(el("div", { className: "head" }, [
          el("span", { className: "kind" }, ["run"]),
          el("span", { className: "name" }, ["stored evidence"]),
          el("span", { className: "span-id" }, ["run_id=" + run.run_id]),
        ]));
        const grid = el("div", { className: "attrs" }, []);
        const entries = [
          ["verifications", String(verifications.length)],
          [
            "verification_methods",
            summarizeValues(verifications.map(function (verification) {
              return formatVerificationMethod(verification.method);
            })),
          ],
          ["observation_snapshots", String(observations.length)],
          [
            "observation_sources",
            summarizeValues(observations.map(function (snapshot) {
              return snapshot && snapshot.source ? String(snapshot.source) : "unknown";
            })),
          ],
        ];
        for (const entry of entries) {
          grid.appendChild(el("span", { className: "k" }, [entry[0]]));
          grid.appendChild(el("span", { className: "v" }, [entry[1]]));
        }
        node.appendChild(grid);
        return;
      }
      node.className = "span-detail empty";
      // Re-mount sparkle <img> alongside the empty-state copy. The
      // browser handles SVG namespacing via the /assets/sparkle.svg
      // request, so no template gymnastics are needed.
      node.innerHTML = SPARKLE_IMG
        + '<span>Select a span to inspect its attributes.</span>';
      return;
    }
    node.className = "span-detail";
    const attrs = span.attributes || {};
    const headChildren = [
      el("span", { className: "kind" }, ["span"]),
      el("span", { className: "name" }, [span.name || ""]),
      el("span", { className: "span-id" }, ["span_id=" + span.span_id]),
    ];
    node.appendChild(el("div", { className: "head" }, headChildren));
    const attrKeys = Object.keys(attrs).sort();
    if (attrKeys.length === 0) {
      node.appendChild(el("div", { className: "attrs-empty" }, ["(no attributes)"]));
    } else {
      const grid = el("div", { className: "attrs" }, []);
      for (const k of attrKeys) {
        grid.appendChild(el("span", { className: "k" }, [k]));
        grid.appendChild(el("span", { className: "v" }, [String(stringifyAttr(attrs[k]))]));
      }
      node.appendChild(grid);
    }
  }

  function stringifyAttr(v) {
    if (v === null || v === undefined) return "";
    if (typeof v === "string" || typeof v === "number" || typeof v === "boolean") return v;
    try { return JSON.stringify(v); } catch (e) { return String(v); }
  }

  function fmtRelativeSeconds(ms) {
    if (ms == null || !isFinite(ms)) return "—";
    const seconds = ms / 1000;
    const sign = seconds >= 0 ? "+" : "";
    return sign + seconds.toFixed(2) + "s";
  }

  function classifyEventName(name) {
    const lower = (name || "").toLowerCase();
    if (lower.indexOf("failed") >= 0 || lower.indexOf("error") >= 0) return "failed";
    if (lower.indexOf("started") >= 0 || lower.indexOf("invoke") >= 0) return "started";
    return "";
  }

  function eventBody(event) {
    if (event.message) return event.message;
    const attrs = event.attributes || {};
    const keys = Object.keys(attrs);
    if (!keys.length) return "";
    return keys.sort().map(function (k) {
      return k + "=" + stringifyAttr(attrs[k]);
    }).join(" ");
  }

  function renderEventList(events, run) {
    const list = document.getElementById("event-list");
    const counter = document.getElementById("event-count");
    list.innerHTML = "";
    if (!events.length) {
      counter.textContent = "0 · tail";
      list.appendChild(el("div", { className: "event-empty" }, ["no events recorded for this run."]));
      return;
    }
    counter.textContent = events.length + " · tail";
    const runStarted = run ? (run.started_at_millis || 0) : 0;
    for (const event of events) {
      const cls = classifyEventName(event.name);
      const tRel = runStarted > 0 ? fmtRelativeSeconds(event.timestamp_millis - runStarted) : "—";
      const live = event._live ? " live" : "";
      const row = el("div", { className: "event-row" + live }, [
        el("span", { className: "t" }, [tRel]),
        el("span", { className: "name" + (cls ? " " + cls : "") }, [event.name || ""]),
        el("span", { className: "span" }, [midTrunc(event.span_id || "", 4, 4)]),
        el("span", { className: "body" }, [eventBody(event)]),
      ]);
      list.appendChild(row);
    }
    // Pin the tail of the rail so the latest event stays visible when
    // streaming.
    list.scrollTop = list.scrollHeight;
  }

  // -- Artifact panel (C.3b) -------------------------------------------------

  function showArtifactPanel(visible) {
    const panel = document.getElementById("artifact-panel");
    if (visible) panel.removeAttribute("hidden");
    else panel.setAttribute("hidden", "");
  }

  function mimeIconAssetName(mime) {
    if (!mime) return "icon-bin.svg";
    if (mime.indexOf("image/") === 0) return "icon-png.svg";
    if (mime === "application/json" || mime.indexOf("text/") === 0) return "icon-json.svg";
    return "icon-bin.svg";
  }

  function isTextLikeMime(mime) {
    if (!mime) return false;
    if (mime === "application/json") return true;
    if (mime.indexOf("text/") === 0) return true;
    if (mime.indexOf("+json") > 0 || mime.indexOf("+xml") > 0) return true;
    return false;
  }

  function isImageMime(mime) {
    return !!mime && mime.indexOf("image/") === 0;
  }

  function isClickOverlayArtifact(artifact) {
    return !!artifact && artifact.role === "click.overlay";
  }

  function isClickOverlayAnnotation(artifact) {
    return !!artifact && artifact.role === "click.overlay.annotation";
  }

  function isMinecraftTrainingPackageArtifact(artifact) {
    return !!artifact && artifact.role === "minecraft-3dgs-training-package";
  }

  function isMinecraftTrainingPackageInspectArtifact(artifact) {
    return !!artifact && artifact.role === "minecraft-3dgs-training-package-inspect";
  }

  function isMinecraftTrainingLaunchArtifact(artifact) {
    return !!artifact && artifact.role === "minecraft-3dgs-training-launch-plan";
  }

  function isMinecraftTrainingLaunchInspectArtifact(artifact) {
    return !!artifact && artifact.role === "minecraft-3dgs-training-launch-inspect";
  }

  function isMinecraftTrainingJobArtifact(artifact) {
    return !!artifact && artifact.role === "minecraft-3dgs-training-job";
  }

  function isMinecraftTrainingJobInspectArtifact(artifact) {
    return !!artifact && artifact.role === "minecraft-3dgs-training-job-inspect";
  }

  function isMinecraftTrainingResultArtifact(artifact) {
    return !!artifact && artifact.role === "minecraft-3dgs-training-result";
  }

  function isMinecraftTrainingResultInspectArtifact(artifact) {
    return !!artifact && artifact.role === "minecraft-3dgs-training-result-inspect";
  }

  function isMinecraftTrainingResultArtifactFetchManifestArtifact(artifact) {
    return !!artifact && artifact.role === "minecraft-3dgs-training-result-artifact-manifest";
  }


  function isMinecraftTrainingResultSemanticManifestArtifact(artifact) {
    return !!artifact && artifact.role === "minecraft-3dgs-training-result-semantic";
  }

  function isMinecraftTrainingResultSemanticInspectArtifact(artifact) {
    return !!artifact && artifact.role === "minecraft-3dgs-training-result-semantic-inspect";
  }

  function isMinecraftTrainingResultHoldoutPreviewManifestArtifact(artifact) {
    return !!artifact && artifact.role === "minecraft-3dgs-training-result-holdout-preview";
  }

  function isMinecraftTrainingResultHoldoutPreviewInspectArtifact(artifact) {
    return !!artifact && artifact.role === "minecraft-3dgs-training-result-holdout-preview-inspect";
  }


  const QUALITY_BASELINE_PROFILE_V1 = {
    profile_id: "mc17-d2-primary-v1",
    training_result_semantic_manifest_path: ".tmp/mc10-smoke-review/semantic/minecraft-3dgs-training-result-semantic.json",
    query_target_block: "511,73,728",
    query_target_face: "north",
    query_target_semantics: "hit_face_center",
    holdout_frame_index: 6,
    basis_checkpoint_suffix: "step-000001.ckpt",
    recorded_run_ids: {
      mc12: "run_1782594518255_60230_0",
      mc16: "run_1782594524936_60749_0",
      mc17: "run_1782594531314_61141_0",
    },
  };

  function isMinecraftHoldoutRenderQualityManifestArtifact(artifact) {
    return !!artifact && artifact.role === "minecraft-3dgs-holdout-render-quality";
  }

  function isMinecraftHoldoutRenderQualityInspectArtifact(artifact) {
    return !!artifact && artifact.role === "minecraft-3dgs-holdout-render-quality-inspect";
  }

  function blockLabelFromParsedTargetBlock(targetBlock) {
    if (!targetBlock) return null;
    if (typeof targetBlock === "string") return targetBlock;
    return [targetBlock.x, targetBlock.y, targetBlock.z].join(",");
  }

  function spatialQueryMatchesProfile(parsed, profile) {
    if (!parsed || !profile) return false;
    return parsed.training_result_semantic_manifest_path === profile.training_result_semantic_manifest_path
      && blockLabelFromParsedTargetBlock(parsed.target_block) === profile.query_target_block
      && (parsed.target_face || null) === (profile.query_target_face || null)
      && parsed.target_semantics === profile.query_target_semantics;
  }

  function holdoutPreviewMatchesProfile(parsed, profile) {
    if (!parsed || !profile) return false;
    const checkpoint = parsed.basis_checkpoint_path || "";
    return parsed.training_result_semantic_manifest_path === profile.training_result_semantic_manifest_path
      && parsed.holdout_frame_index === profile.holdout_frame_index
      && checkpoint.endsWith(profile.basis_checkpoint_suffix);
  }

  function holdoutRenderQualityMatchesProfile(parsed, profile) {
    if (!parsed || !profile) return false;
    const checkpoint = parsed.basis_checkpoint_path || "";
    return parsed.training_result_semantic_manifest_path === profile.training_result_semantic_manifest_path
      && parsed.holdout_frame_index === profile.holdout_frame_index
      && checkpoint.endsWith(profile.basis_checkpoint_suffix);
  }

  function buildQualityBaselineTrustNotes(renderQuality) {
    const notes = [
      "MC-12 projection_reference answers are scene-packet reference geometry only; they are not Gaussian-native inference",
      "MC-17 screenshot-copy render probe measures pipeline comparability only; it is not trained-splat usefulness evidence",
    ];
    if (renderQuality && Array.isArray(renderQuality.known_limits)) {
      renderQuality.known_limits.forEach(function (limit) {
        if (notes.indexOf(limit) < 0) notes.push(limit);
      });
    }
    return notes;
  }

  function deriveQualityBaselineReport(profile, spatialQuery, holdoutPreview, renderQuality) {
    const issues = [];
    if (spatialQuery && !spatialQueryMatchesProfile(spatialQuery, profile)) {
      issues.push("spatial query manifest does not match baseline profile pins");
    }
    if (holdoutPreview && !holdoutPreviewMatchesProfile(holdoutPreview, profile)) {
      issues.push("holdout preview manifest does not match baseline profile pins");
    }
    if (renderQuality && !holdoutRenderQualityMatchesProfile(renderQuality, profile)) {
      issues.push("holdout render quality manifest does not match baseline profile pins");
    }
    const stageCount = [spatialQuery, holdoutPreview, renderQuality].filter(Boolean).length;
    let evidenceCoverage = "partial";
    if (stageCount === 0) evidenceCoverage = "missing_stage";
    else if (stageCount === 3 && issues.length === 0) evidenceCoverage = "complete";

    const metrics = renderQuality && renderQuality.metrics ? renderQuality.metrics : null;
    return {
      profile_id: profile.profile_id,
      training_result_semantic_manifest_path: profile.training_result_semantic_manifest_path,
      evidence_coverage: evidenceCoverage,
      spatial_query_status: spatialQuery ? spatialQuery.status : "n/a",
      visibility: spatialQuery ? spatialQuery.visibility : "n/a",
      screen_point: spatialQuery && spatialQuery.screen_point
        ? [spatialQuery.screen_point.x, spatialQuery.screen_point.y].join(",")
        : "n/a",
      holdout_status: holdoutPreview ? holdoutPreview.status : "n/a",
      holdout_frame_index: holdoutPreview ? holdoutPreview.holdout_frame_index : "n/a",
      basis_checkpoint_path: holdoutPreview ? holdoutPreview.basis_checkpoint_path : "n/a",
      render_quality_status: renderQuality ? renderQuality.status : "n/a",
      verdict: renderQuality ? renderQuality.verdict : "n/a",
      image_size_match: renderQuality ? renderQuality.image_size_match : "n/a",
      l1_mean: metrics && metrics.l1_mean != null ? metrics.l1_mean : "n/a",
      mse: metrics && metrics.mse != null ? metrics.mse : "n/a",
      psnr: metrics && metrics.psnr != null ? metrics.psnr : "n/a",
      trust_notes: buildQualityBaselineTrustNotes(renderQuality),
      issue: issues.length ? issues.join(" | ") : "n/a",
    };
  }

  function collectQualityBaselineEvidenceFromCache(artifacts, cache) {
    let spatialQuery = null;
    let holdoutPreview = null;
    let renderQuality = null;
    (artifacts || []).forEach(function (artifact) {
      const parsed = cache[artifactKey(artifact)];
      if (!parsed) return;
      if (isMinecraftTrainingResultSpatialQueryManifestArtifact(artifact)
        && spatialQueryMatchesProfile(parsed, QUALITY_BASELINE_PROFILE_V1)) {
        spatialQuery = parsed;
      }
      if (isMinecraftTrainingResultHoldoutPreviewManifestArtifact(artifact)
        && holdoutPreviewMatchesProfile(parsed, QUALITY_BASELINE_PROFILE_V1)) {
        holdoutPreview = parsed;
      }
      if (isMinecraftHoldoutRenderQualityManifestArtifact(artifact)) {
        if (!renderQuality || holdoutRenderQualityMatchesProfile(parsed, QUALITY_BASELINE_PROFILE_V1)) {
          renderQuality = parsed;
        }
      }
    });
    return { spatialQuery: spatialQuery, holdoutPreview: holdoutPreview, renderQuality: renderQuality };
  }

  function formatQualityVerdictStageSummary(verdict) {
    if (!verdict || !verdict.stage_checks) return "n/a";
    return verdict.stage_checks.map(function (check) {
      const reason = check.reasons && check.reasons.length ? " reason=" + check.reasons[0] : "";
      return check.stage + "=" + check.outcome + reason;
    }).join(" ");
  }

  function qualityVerdictToSummaryRows(verdict) {
    if (!verdict) return [];
    return [
      ["render_evidence_mode", verdict.render_evidence_mode || "n/a"],
      ["quality_verdict", verdict.quality_verdict || "n/a"],
      ["stage_checks", formatQualityVerdictStageSummary(verdict)],
      ["verdict_issue", verdict.issue || "n/a"],
      ["trust_notes", verdict.trust_notes && verdict.trust_notes.length
        ? verdict.trust_notes.join(" | ")
        : "n/a"],
    ];
  }

  function qualityBaselineReportToSummaryRows(report) {
    if (!report || report.evidence_coverage === "missing_stage") return null;
    const spatial = report.spatial_query || {};
    const holdout = report.holdout_witness || {};
    const render = report.render_quality || {};
    const spatialQueryStatus = spatial.status || report.spatial_query_status || "n/a";
    const visibility = spatial.visibility || report.visibility || "n/a";
    const screenPoint = spatial.screen_point || report.screen_point || "n/a";
    const holdoutStatus = holdout.status || report.holdout_status || "n/a";
    const holdoutFrameIndex = holdout.holdout_frame_index != null
      ? holdout.holdout_frame_index
      : (report.holdout_frame_index != null ? report.holdout_frame_index : "n/a");
    const basisCheckpointPath = holdout.basis_checkpoint_path || report.basis_checkpoint_path || "n/a";
    const renderQualityStatus = render.status || report.render_quality_status || "n/a";
    const renderVerdict = render.verdict || report.verdict || "n/a";
    const imageSizeMatch = render.image_size_match != null
      ? render.image_size_match
      : (report.image_size_match != null ? report.image_size_match : "n/a");
    const l1Mean = render.l1_mean != null ? render.l1_mean : (report.l1_mean != null ? report.l1_mean : "n/a");
    const mse = render.mse != null ? render.mse : (report.mse != null ? report.mse : "n/a");
    const psnr = render.psnr != null ? render.psnr : (report.psnr != null ? report.psnr : "n/a");
    const entries = [
      ["kind", "MC-17 quality baseline report"],
      ["profile_id", report.profile_id],
      ["evidence_coverage", report.evidence_coverage],
      ["spatial_query_status", spatialQueryStatus],
      ["visibility", visibility],
      ["screen_point", screenPoint],
      ["holdout_status", holdoutStatus],
      ["holdout_frame_index", holdoutFrameIndex],
      ["basis_checkpoint_path", basisCheckpointPath],
      ["render_quality_status", renderQualityStatus],
      ["verdict", renderVerdict],
      ["image_size_match", imageSizeMatch],
      ["l1_mean", l1Mean],
      ["mse", mse],
      ["psnr", psnr],
      ["issue", report.issue || "n/a"],
      ["trust_notes", (report.trust_notes || []).join(" | ")],
    ];
    if (report.verdicts) {
      entries.push(["kind", "MC-17 quality verdict (probe)"]);
      entries.push.apply(entries, qualityVerdictToSummaryRows(report.verdicts.probe));
      entries.push(["kind", "MC-17 quality verdict (trained_render)"]);
      entries.push.apply(entries, qualityVerdictToSummaryRows(report.verdicts.trained_render));
    }
    return renderKeyValueSummary(entries);
  }

  function renderQualityBaselineSummaryCardFromServerReport(report) {
    return qualityBaselineReportToSummaryRows(report);
  }

  function renderQualityBaselineSummaryCardFallback(artifacts, cache) {
    const evidence = collectQualityBaselineEvidenceFromCache(artifacts, cache);
    if (!evidence.renderQuality && !evidence.spatialQuery && !evidence.holdoutPreview) {
      return null;
    }
    const report = deriveQualityBaselineReport(
      QUALITY_BASELINE_PROFILE_V1,
      evidence.spatialQuery,
      evidence.holdoutPreview,
      evidence.renderQuality,
    );
    return qualityBaselineReportToSummaryRows(report);
  }

  function loadQualityBaselineSummaryCard(preview, insertBeforeNode, requestId) {
    if (!state.activeRunId) return;
    fetch("/runs/" + encodeURIComponent(state.activeRunId) + "/minecraft-quality-baseline-report")
      .then(function (response) {
        if (!response.ok) throw new Error("HTTP " + response.status);
        return response.json();
      })
      .then(function (report) {
        if (requestId !== state.activeEvidenceRequestId) return;
        const card = renderQualityBaselineSummaryCardFromServerReport(report);
        if (!card) return;
        if (insertBeforeNode && insertBeforeNode.parentNode === preview) {
          preview.insertBefore(card, insertBeforeNode);
        } else {
          preview.insertBefore(card, preview.firstChild);
        }
      })
      .catch(function () {
        if (requestId !== state.activeEvidenceRequestId) return;
        const card = renderQualityBaselineSummaryCardFallback(state.artifacts, state.artifactJsonCache);
        if (!card) return;
        if (insertBeforeNode && insertBeforeNode.parentNode === preview) {
          preview.insertBefore(card, insertBeforeNode);
        } else {
          preview.insertBefore(card, preview.firstChild);
        }
      });
  }

  function isMinecraftTrainingResultSpatialQueryManifestArtifact(artifact) {
    return !!artifact && artifact.role === "minecraft-3dgs-training-result-query";
  }



  function isOperationResultArtifact(artifact) {
    return !!artifact && artifact.role === "operation-result";
  }

  function isMc19QueryWiredLiveActionResult(parsed) {
    return !!parsed && parsed.operation_id === "auv.minecraft.query_wired_live_action";
  }

  // NOTICE(core-c2-d1): reader-side vocabulary only — keep local to viewer in D1.
  function mapActionEligibilityToReadinessClass(donor) {
    if (donor === "click_ready") return "ready";
    if (donor === "answer_non_clickable") return "non_actionable";
    if (donor === "not_consumable") return "not_consumable";
    return null;
  }


  // NOTICE(core-c2-d2): reader-side provenance only — Core-C1 source_readiness_ref.
  function formatSourceReadinessRef(parts) {
    return parts
      .filter(function (entry) { return entry[1]; })
      .map(function (entry) { return entry[0] + "=" + entry[1]; })
      .join(" ");
  }

  function formatQueryManifestSourceReadinessRef(artifactId, runId) {
    return formatSourceReadinessRef([
      ["kind", "query_manifest"],
      ["artifact_id", artifactId],
      ["run_id", runId],
    ]);
  }

  function formatDerivedReadinessSourceReadinessRef(queryArtifactId, runId) {
    return formatSourceReadinessRef([
      ["kind", "derived_readiness"],
      ["query_artifact_id", queryArtifactId],
      ["run_id", runId],
    ]);
  }

  function formatOutcomeEventSourceReadinessRef(eventName, operationResultArtifactId) {
    const parts = [["kind", "outcome_event"], ["event", eventName]];
    if (operationResultArtifactId) {
      parts.push(["operation_result_artifact_id", operationResultArtifactId]);
    }
    return formatSourceReadinessRef(parts);
  }

  // NOTICE(core-c2-d2): parity with Rust manifest.is_some() — MC-19 required serde fields.
  function nonEmptyString(value) {
    return typeof value === "string" && value.length > 0;
  }

  function isCachedMc19QueryManifestProvenanceReady(manifest) {
    if (!manifest || typeof manifest !== "object") return false;
    if (typeof manifest.schema_version !== "number") return false;
    if (typeof manifest.generated_at_millis !== "number") return false;
    const status = manifest.status;
    if (status !== "answered" && status !== "blocked" && status !== "failed") return false;
    if (!nonEmptyString(manifest.query_kind)) return false;
    if (!nonEmptyString(manifest.target_semantics)) return false;
    if (!nonEmptyString(manifest.trainer_backend)) return false;
    if (!nonEmptyString(manifest.job_backend)) return false;
    if (!nonEmptyString(manifest.normalized_result_dir)) return false;
    if (!nonEmptyString(manifest.training_result_semantic_manifest_path)) return false;
    const sourcePaths = [
      "source_training_result_artifact_manifest_path",
      "source_training_result_manifest_path",
      "source_training_job_manifest_path",
      "source_training_launch_plan_path",
      "source_training_package_manifest_path",
      "source_scene_packet_manifest_path",
    ];
    for (let i = 0; i < sourcePaths.length; i++) {
      if (!nonEmptyString(manifest[sourcePaths[i]])) return false;
    }
    if (!Array.isArray(manifest.source_bundle_manifest_paths)) return false;
    if (!Array.isArray(manifest.source_run_ids)) return false;
    if (!Array.isArray(manifest.known_limits)) return false;
    const targetBlock = manifest.target_block;
    if (!targetBlock || typeof targetBlock !== "object") return false;
    if (typeof targetBlock.x !== "number" || typeof targetBlock.y !== "number" || typeof targetBlock.z !== "number") {
      return false;
    }
    return true;
  }

  function mc19SelfTestManifestFixture() {
    return {
      schema_version: 1,
      generated_at_millis: 1,
      training_result_semantic_manifest_path: "/tmp/semantic.json",
      source_training_result_artifact_manifest_path: "/tmp/artifact.json",
      source_training_result_manifest_path: "/tmp/result.json",
      source_training_job_manifest_path: "/tmp/job.json",
      source_training_launch_plan_path: "/tmp/launch.json",
      source_training_package_manifest_path: "/tmp/package.json",
      source_scene_packet_manifest_path: "/tmp/scene-packet.json",
      source_bundle_manifest_paths: ["/tmp/bundle.json"],
      source_run_ids: ["run-a"],
      trainer_backend: "nerfstudio.splatfacto",
      job_backend: "remote",
      normalized_result_dir: "/tmp/normalized",
      query_kind: "block_projection",
      target_block: { x: 511, y: 73, z: 728 },
      target_semantics: "hit_face_center",
      status: "answered",
      visibility: "visible",
      screen_point: { x: 1, y: 2 },
      known_limits: [],
    };
  }

  function classifyCachedQueryManifestSourceReadiness(queryArtifactId, queryRole, runArtifacts, artifactJsonCache) {
    if (!queryArtifactId || !Array.isArray(runArtifacts)) return null;
    const queryArtifact = runArtifacts.find(function (artifact) {
      return artifact && artifact.artifact_id === queryArtifactId && artifact.role === queryRole;
    });
    if (!queryArtifact) return "clean_miss";
    if (!artifactJsonCache) return null;
    const cacheKey = artifactKey(queryArtifact);
    if (!artifactJsonCache[cacheKey]) return null;
    const manifest = artifactJsonCache[cacheKey];
    if (!isCachedMc19QueryManifestProvenanceReady(manifest)) return "matched_parse_failure";
    return "matched_valid_manifest";
  }

  function resolveQueryWiredLiveActionSourceReadinessRef(params) {
    const queryArtifactId = params.queryArtifactId;
    const operationResultArtifactId = params.operationResultArtifactId;
    const runId = params.runId;
    const hasOutcomeEvent = params.hasOutcomeEvent;
    const runArtifacts = params.runArtifacts;
    const artifactJsonCache = params.artifactJsonCache;
    const outcomeEventName = "minecraft.query_wired_live_action.outcome";

    if (queryArtifactId) {
      const lookup = classifyCachedQueryManifestSourceReadiness(
        queryArtifactId,
        "minecraft-3dgs-training-result-query",
        runArtifacts,
        artifactJsonCache
      );
      if (lookup === null) return null;
      if (lookup === "matched_valid_manifest") {
        return formatQueryManifestSourceReadinessRef(queryArtifactId, runId);
      }
      if (lookup === "clean_miss") {
        return formatDerivedReadinessSourceReadinessRef(queryArtifactId, runId);
      }
      return null;
    }
    if (hasOutcomeEvent) {
      return formatOutcomeEventSourceReadinessRef(outcomeEventName, operationResultArtifactId);
    }
    return null;
  }

  function parseEventMessageField(message, key) {
    if (!message) return null;
    const prefix = key + "=";
    if (key === "refusal_reason") {
      const start = message.indexOf(prefix);
      if (start < 0) return null;
      let rest = message.slice(start + prefix.length);
      const marker = " query_manifest_path=";
      const idx = rest.indexOf(marker);
      if (idx >= 0) rest = rest.slice(0, idx);
      rest = rest.trim();
      return rest || null;
    }
    const tokens = message.split(/\s+/);
    for (let i = 0; i < tokens.length; i++) {
      if (tokens[i].indexOf(prefix) === 0) {
        return tokens[i].slice(prefix.length);
      }
    }
    return null;
  }

  function queryArtifactIdFromOperationResult(parsed) {
    if (!parsed) return null;
    const evidence = Array.isArray(parsed.evidence_artifacts) ? parsed.evidence_artifacts : [];
    if (evidence.length && evidence[0].artifact_id) return evidence[0].artifact_id;
    const basis = parsed.freshness_basis && parsed.freshness_basis.source_artifact;
    return basis && basis.artifact_id ? basis.artifact_id : null;
  }


  // NOTICE(core-c3-d2): reader-side Layer 3 summary only — verification_outcome projection.
  function formatOperationResultVerificationSource(artifactId, runId) {
    return formatSourceReadinessRef([
      ["kind", "operation_result"],
      ["artifact_id", artifactId],
      ["run_id", runId],
    ]);
  }

  function operationResultVerificationClaims(parsed) {
    if (!parsed || typeof parsed !== "object") return [];
    if (Array.isArray(parsed.verifications) && parsed.verifications.length > 0) {
      return parsed.verifications;
    }
    if (parsed.output && parsed.output.kind === "verification" && parsed.output.verification) {
      return [parsed.output.verification];
    }
    return [];
  }

  function isActivationOnlyVerification(verification) {
    return !!verification
      && verification.method
      && verification.method.kind === "custom"
      && verification.method.name === "activation_only";
  }

  function verificationClaimReasonSnippet(verification) {
    if (!verification) return null;
    if (typeof verification.observed_label === "string" && verification.observed_label.length > 0) {
      return verification.observed_label;
    }
    if (verification.failure_layer) return verification.failure_layer;
    return null;
  }

  function buildVerificationReasonFromClaims(claims) {
    const parts = [];
    claims.forEach(function (claim) {
      const snippet = verificationClaimReasonSnippet(claim);
      if (snippet && parts.indexOf(snippet) === -1) parts.push(snippet);
    });
    return parts.length ? parts.join("; ") : null;
  }

  function projectVerificationOutcomeFromClaims(claims) {
    const semanticClaims = claims.filter(function (claim) {
      return !isActivationOnlyVerification(claim);
    });
    const focus = semanticClaims.length ? semanticClaims : claims;

    for (let i = 0; i < focus.length; i++) {
      if (focus[i].failure_layer === "verification_unreliable") {
        return { verification_outcome: "unreliable", verification_reason: buildVerificationReasonFromClaims(focus) };
      }
    }
    for (let i = 0; i < focus.length; i++) {
      const claim = focus[i];
      if (claim.failure_layer === "semantic_mismatch"
        || claim.failure_layer === "state_changed_no_match"
        || claim.semantic_matched === false) {
        return { verification_outcome: "failed", verification_reason: buildVerificationReasonFromClaims(focus) };
      }
    }
    if (focus.length && focus.every(isActivationOnlyVerification)) {
      return {
        verification_outcome: "activation_only",
        verification_reason: buildVerificationReasonFromClaims(focus)
          || "input delivery recorded; no semantic post-action assertion",
      };
    }
    if (focus.some(function (claim) { return claim.semantic_matched === true; })) {
      return { verification_outcome: "passed", verification_reason: buildVerificationReasonFromClaims(focus) };
    }
    if (focus.some(function (claim) { return claim.state_changed && claim.semantic_matched == null; })) {
      return { verification_outcome: "inconclusive", verification_reason: buildVerificationReasonFromClaims(focus) };
    }
    return {
      verification_outcome: "absent",
      verification_reason: "verification claims present but not mappable to a read-side outcome",
    };
  }

  function resolveQueryWiredLiveActionVerificationProjection(options) {
    const attempted = !!options.attempted;
    const parsed = options.parsed;
    const operationResultArtifactId = options.operationResultArtifactId || null;
    const runId = options.runId || null;
    const refusalReason = options.refusalReason || null;

    if (!attempted) {
      return {
        verification_outcome: "not_attempted",
        verification_source: formatSourceReadinessRef([["kind", "layer1_no_dispatch"]]),
        verification_reason: refusalReason
          || "post-action verification N/A; action not dispatched",
      };
    }
    if (!parsed) {
      return {
        verification_outcome: "absent",
        verification_source: null,
        verification_reason: "attempted=true but operation-result artifact missing on read path",
      };
    }
    const verificationSource = operationResultArtifactId && runId
      ? formatOperationResultVerificationSource(operationResultArtifactId, runId)
      : null;
    const claims = operationResultVerificationClaims(parsed);
    if (!claims.length) {
      const knownLimits = Array.isArray(parsed.known_limits) ? parsed.known_limits : [];
      return {
        verification_outcome: "absent",
        verification_source: verificationSource,
        verification_reason: knownLimits[0]
          || "no VerificationResult on operation-result; Layer 3 evidence absent",
      };
    }
    const projected = projectVerificationOutcomeFromClaims(claims);
    return {
      verification_outcome: projected.verification_outcome,
      verification_source: verificationSource,
      verification_reason: projected.verification_reason,
    };
  }

  function deriveQueryWiredLiveActionCard(parsed, runEvents, runArtifacts, runId) {
    const events = Array.isArray(runEvents) ? runEvents : [];
    const outcomeEvent = events.find(function (event) {
      return event && event.name === "minecraft.query_wired_live_action.outcome";
    });
    const inputsEvent = events.find(function (event) {
      return event && event.name === "minecraft.query_wired_live_action.inputs";
    });
    if (!outcomeEvent && !(parsed && isMc19QueryWiredLiveActionResult(parsed))) {
      return null;
    }

    const outcomeMessage = outcomeEvent && outcomeEvent.message ? outcomeEvent.message : "";
    const attemptedRaw = parseEventMessageField(outcomeMessage, "attempted");
    const attempted = attemptedRaw === "true";
    const actionEligibility = parseEventMessageField(outcomeMessage, "action_eligibility") || "n/a";
    let refusalReason = parseEventMessageField(outcomeMessage, "refusal_reason");
    if (refusalReason === "none") refusalReason = null;

    const inputsMessage = inputsEvent && inputsEvent.message ? inputsEvent.message : "";
    const targetApp = parseEventMessageField(inputsMessage, "target_app");
    const targetTitle = parseEventMessageField(inputsMessage, "target_title");

    let dispatchCommand = null;
    let dispatchOutcome = null;
    events.forEach(function (event) {
      if (!event) return;
      if (event.name === "command.resolved" && event.message === "resolved input.clickWindowPoint") {
        dispatchCommand = "input.clickWindowPoint";
        dispatchOutcome = "resolved";
      }
      if (event.name === "command.failed" && dispatchCommand) {
        dispatchOutcome = "failed: " + (event.message || "unknown");
      }
    });

    const queryArtifactId = parsed ? queryArtifactIdFromOperationResult(parsed) : null;
    let operationResultArtifactId = null;
    if (parsed && Array.isArray(runArtifacts)) {
      const opArtifact = runArtifacts.find(function (artifact) {
        if (!isOperationResultArtifact(artifact)) return false;
        const key = artifactKey(artifact);
        const cached = state.artifactJsonCache && state.artifactJsonCache[key];
        return cached === parsed;
      });
      if (opArtifact) operationResultArtifactId = opArtifact.artifact_id;
    }
    const operationStatus = parsed && parsed.status ? parsed.status : null;
    let operationMessage = null;
    if (parsed && parsed.output && parsed.output.kind === "acknowledged") {
      operationMessage = parsed.output.message || null;
    }

    let mc14ActionEligibility = null;
    let windowPoint = null;
    if (queryArtifactId && Array.isArray(runArtifacts)) {
      const queryArtifact = runArtifacts.find(function (artifact) {
        return artifact && artifact.artifact_id === queryArtifactId
          && artifact.role === "minecraft-3dgs-training-result-query";
      });
      if (queryArtifact && state.artifactJsonCache && state.artifactJsonCache[artifactKey(queryArtifact)]) {
        const manifest = state.artifactJsonCache[artifactKey(queryArtifact)];
        const readiness = deriveSpatialQueryActionReadiness(manifest);
        mc14ActionEligibility = readiness.action_eligibility;
        windowPoint = readiness.window_point;
      }
    }

    const readinessDonor = mc14ActionEligibility || actionEligibility;
    const readinessClass = mapActionEligibilityToReadinessClass(readinessDonor);
    const effectiveRunId = runId || state.activeRunId || null;
    const sourceReadinessRef = effectiveRunId
      ? resolveQueryWiredLiveActionSourceReadinessRef({
          queryArtifactId: queryArtifactId,
          operationResultArtifactId: operationResultArtifactId,
          runId: effectiveRunId,
          hasOutcomeEvent: !!outcomeEvent,
          runArtifacts: runArtifacts,
          artifactJsonCache: state.artifactJsonCache,
        })
      : null;
    const verificationProjection = resolveQueryWiredLiveActionVerificationProjection({
      attempted: attempted,
      parsed: parsed,
      operationResultArtifactId: operationResultArtifactId,
      runId: effectiveRunId,
      refusalReason: refusalReason,
    });

    return {
      attempted: attempted,
      action_eligibility: actionEligibility,
      window_point: windowPoint,
      refusal_reason: refusalReason,
      operation_status: operationStatus,
      operation_message: operationMessage,
      dispatch_command: dispatchCommand,
      dispatch_outcome: dispatchOutcome,
      query_artifact_id: queryArtifactId,
      target_app: targetApp,
      target_title: targetTitle,
      mc14_action_eligibility: mc14ActionEligibility,
      readiness_class: readinessClass,
      source_readiness_ref: sourceReadinessRef,
      verification_outcome: verificationProjection.verification_outcome,
      verification_source: verificationProjection.verification_source,
      verification_reason: verificationProjection.verification_reason,
    };
  }

  function renderQueryWiredLiveActionSummaryCard(artifact, parsed, runEvents, runArtifacts) {
    if (!isOperationResultArtifact(artifact) || !isMc19QueryWiredLiveActionResult(parsed)) {
      return null;
    }
    const card = deriveQueryWiredLiveActionCard(parsed, runEvents, runArtifacts);
    if (!card) return null;
    return renderKeyValueSummary([
      ["kind", "MC-19 query wired live action"],
      ["operation_id", parsed.operation_id],
      ["query_artifact_id", card.query_artifact_id],
      ["attempted", card.attempted],
      ["action_eligibility", card.action_eligibility],
      ["window_point", card.window_point],
      ["refusal_reason", card.refusal_reason],
      ["operation_status", card.operation_status],
      ["operation_message", card.operation_message],
      ["dispatch_command", card.dispatch_command],
      ["dispatch_outcome", card.dispatch_outcome],
      ["target_app", card.target_app],
      ["target_title", card.target_title],
      ["mc14_action_eligibility", card.mc14_action_eligibility],
      ["readiness_class", card.readiness_class],
      ["source_readiness_ref", card.source_readiness_ref],
      ["verification_outcome", card.verification_outcome],
      ["verification_source", card.verification_source],
      ["verification_reason", card.verification_reason],
      ["known_limits", countArray(parsed.known_limits)],
    ]);
  }

  function wiredActionRowsForQueryManifest(artifact, runEvents, runArtifacts) {
    if (!artifact || !Array.isArray(runArtifacts)) return [];
    const opArtifact = runArtifacts.find(function (candidate) {
      if (!isOperationResultArtifact(candidate)) return false;
      const key = artifactKey(candidate);
      const cached = state.artifactJsonCache && state.artifactJsonCache[key];
      if (cached && isMc19QueryWiredLiveActionResult(cached)) return true;
      return !state.artifactJsonCache;
    }) || runArtifacts.find(function (candidate) {
      if (!isOperationResultArtifact(candidate)) return false;
      const key = artifactKey(candidate);
      const cached = state.artifactJsonCache && state.artifactJsonCache[key];
      return cached && isMc19QueryWiredLiveActionResult(cached);
    });
    if (!opArtifact) return [];
    const cacheKey = artifactKey(opArtifact);
    const parsed = state.artifactJsonCache && state.artifactJsonCache[cacheKey];
    if (!parsed || !isMc19QueryWiredLiveActionResult(parsed)) return [];
    const linkedQueryId = queryArtifactIdFromOperationResult(parsed);
    if (!linkedQueryId || linkedQueryId !== artifact.artifact_id) return [];
    const card = deriveQueryWiredLiveActionCard(parsed, runEvents, runArtifacts);
    if (!card) return [];
    return [
      ["wired_action_attempted", card.attempted],
      ["wired_action_eligibility", card.action_eligibility],
      ["wired_action_refusal", card.refusal_reason],
      ["wired_action_dispatch_command", card.dispatch_command],
      ["wired_action_dispatch_outcome", card.dispatch_outcome],
      ["wired_action_operation_status", card.operation_status],
      ["wired_action_operation_message", card.operation_message],
      ["wired_action_source_readiness_ref", card.source_readiness_ref],
      ["wired_action_verification_outcome", card.verification_outcome],
      ["wired_action_verification_source", card.verification_source],
      ["wired_action_verification_reason", card.verification_reason],
    ];
  }


  function deriveSpatialQueryActionReadiness(parsed) {
    if (!parsed || typeof parsed !== "object") {
      return {
        action_eligibility: "n/a",
        window_point: null,
        refusal_reason: null,
      };
    }
    const status = parsed.status;
    if (status !== "answered") {
      const reason = parsed.reason ? ` reason=${parsed.reason}` : "";
      return {
        action_eligibility: "not_consumable",
        window_point: null,
        refusal_reason: `status=${status || "unknown"}${reason}`,
      };
    }
    const visibility = parsed.visibility;
    if (!visibility) {
      return {
        action_eligibility: "answer_non_clickable",
        window_point: null,
        refusal_reason: "answered query missing visibility witness",
      };
    }
    if (visibility === "visible" && parsed.screen_point && typeof parsed.screen_point.x === "number" && typeof parsed.screen_point.y === "number") {
      return {
        action_eligibility: "click_ready",
        window_point: `${parsed.screen_point.x},${parsed.screen_point.y}`,
        refusal_reason: null,
      };
    }
    if (visibility !== "visible") {
      return {
        action_eligibility: "answer_non_clickable",
        window_point: null,
        refusal_reason: `visibility=${visibility}`,
      };
    }
    return {
      action_eligibility: "answer_non_clickable",
      window_point: null,
      refusal_reason: "visibility=visible missing_screen_point",
    };
  }

  function isMinecraftTrainingResultSpatialQueryInspectArtifact(artifact) {
    return !!artifact && artifact.role === "minecraft-3dgs-training-result-query-inspect";
  }

  function isMinecraftTrainingResultArtifactFetchInspectArtifact(artifact) {
    return !!artifact && artifact.role === "minecraft-3dgs-training-result-artifact-inspect";
  }

  function artifactStem(artifact) {
    const basename = pathBasename(artifact && artifact.path);
    const idx = basename.lastIndexOf(".");
    const stem = idx > 0 ? basename.slice(0, idx) : basename;
    return stem.replace(/^artifact_\d+_/, "");
  }

  function pairedOverlayStem(artifact) {
    return artifactStem(artifact).replace(/\.annotation$/, "");
  }

  function findClickOverlayAnnotationArtifact(overlayArtifact, artifacts) {
    if (!overlayArtifact || !artifacts) return null;
    const stem = pairedOverlayStem(overlayArtifact);
    return artifacts.find(function (candidate) {
      return isClickOverlayAnnotation(candidate)
        && candidate.span_id === overlayArtifact.span_id
        && pairedOverlayStem(candidate) === stem;
    }) || null;
  }

  function findClickOverlayImageArtifact(annotationArtifact, artifacts) {
    if (!annotationArtifact || !artifacts) return null;
    const stem = pairedOverlayStem(annotationArtifact);
    return artifacts.find(function (candidate) {
      return isClickOverlayArtifact(candidate)
        && candidate.span_id === annotationArtifact.span_id
        && pairedOverlayStem(candidate) === stem;
    }) || null;
  }

  function artifactKey(artifact) {
    if (!artifact) return "";
    return (artifact.span_id || "") + "::" + (artifact.artifact_id || "");
  }

  function defaultArtifactKey(artifacts) {
    if (!artifacts || !artifacts.length) return null;
    const overlay = artifacts.slice().reverse().find(isClickOverlayArtifact);
    return overlay ? artifactKey(overlay) : null;
  }

  function preferredArtifactKeyForSpan(spanId, artifacts) {
    if (!spanId || !artifacts || !artifacts.length) return null;
    const spanArtifacts = artifacts.filter(function (artifact) {
      return artifact.span_id === spanId;
    });
    if (!spanArtifacts.length) return null;
    const overlay = spanArtifacts.slice().reverse().find(isClickOverlayArtifact);
    return artifactKey(overlay || spanArtifacts[spanArtifacts.length - 1]);
  }

  function artifactBadge(artifact, artifacts) {
    if (isClickOverlayArtifact(artifact)) return "evidence";
    if (isClickOverlayAnnotation(artifact)) return "annotation";
    return "";
  }

  function pathBasename(path) {
    if (!path) return "";
    const idx = path.lastIndexOf("/");
    return idx >= 0 ? path.slice(idx + 1) : path;
  }

  function findArtifact(key) {
    return state.artifacts.find(function (artifact) {
      return artifactKey(artifact) === key;
    }) || null;
  }

  function findArtifactByRole(artifacts, role) {
    return (artifacts || []).find(function (artifact) {
      return artifact && artifact.role === role;
    }) || null;
  }

  function visibleArtifacts(artifacts) {
    const list = Array.isArray(artifacts) ? artifacts : [];
    if (!state.activeArtifactRoleFilter) return list;
    return list.filter(function (artifact) {
      return artifact && artifact.role === state.activeArtifactRoleFilter;
    });
  }

  function jumpToViewParserArtifactRole(role) {
    if (!role) return;
    showArtifactPanel(true);
    state.activeArtifactRoleFilter = role;
    const target = findArtifactByRole(state.artifacts, role);
    if (target) state.activeArtifactKey = artifactKey(target);
    renderArtifactList(state.artifacts);
    renderArtifactPreview(findArtifact(state.activeArtifactKey), state.artifacts);
    const list = document.getElementById("artifact-list");
    if (!list || !state.activeArtifactKey) return;
    const row = list.querySelector(
      '[data-artifact-key="' + state.activeArtifactKey + '"]'
    );
    if (row && row.scrollIntoView) row.scrollIntoView({ block: "nearest" });
  }

  function artifactUrl(artifact) {
    if (!artifact) return "";
    const params = new URLSearchParams();
    if (artifact.span_id) params.set("spanId", artifact.span_id);
    const suffix = params.toString();
    return "/runs/" + encodeURIComponent(state.activeRunId)
      + "/artifacts/" + encodeURIComponent(artifact.artifact_id)
      + (suffix ? "?" + suffix : "");
  }

  function renderArtifactList(artifacts) {
    const list = document.getElementById("artifact-list");
    const counter = document.getElementById("artifact-count");
    const all = Array.isArray(artifacts) ? artifacts : [];
    const visible = visibleArtifacts(all);
    list.innerHTML = "";
    counter.textContent = state.activeArtifactRoleFilter
      ? visible.length + " · " + state.activeArtifactRoleFilter
      : visible.length.toString();
    if (!visible.length) {
      const emptyMessage = state.activeArtifactRoleFilter
        ? "no artifacts with role " + state.activeArtifactRoleFilter + " on this run."
        : "no artifacts recorded for this run.";
      list.appendChild(el("div", { className: "artifact-empty" }, [emptyMessage]));
      return;
    }
    for (const artifact of visible) {
      const icon = el("img", {
        className: "mime-icon",
        src: "/assets/" + mimeIconAssetName(artifact.mime_type),
        alt: "",
      });
      const meta = el("div", { className: "meta" }, [
        el("span", { className: "role" }, [artifact.role || artifact.artifact_id]),
        el("span", { className: "file" }, [pathBasename(artifact.path)]),
      ]);
      const active = artifactKey(artifact) === state.activeArtifactKey;
      const badge = artifactBadge(artifact, artifacts);
      const rowChildren = badge
        ? [icon, meta, el("span", { className: "badge" }, [badge])]
        : [icon, meta];
      const row = el(
        "button",
        {
          className: "artifact-row"
            + (isClickOverlayArtifact(artifact) ? " evidence" : "")
            + (isClickOverlayAnnotation(artifact) ? " annotation" : "")
            + (active ? " active" : ""),
          dataset: { artifactKey: artifactKey(artifact) },
          onClick: function () {
            state.activeArtifactKey = artifactKey(artifact);
            renderArtifactList(state.artifacts);
            renderArtifactPreview(findArtifact(state.activeArtifactKey), state.artifacts);
          },
        },
        rowChildren
      );
      list.appendChild(row);
    }
  }

  function renderArtifactPreviewEmpty(count) {
    const preview = document.getElementById("artifact-preview");
    preview.className = "artifact-preview empty";
    preview.innerHTML = "";
    preview.appendChild(el("img", {
      className: "inspector",
      src: "/assets/sprite-inspector.svg",
      alt: "",
    }));
    preview.appendChild(el("div", { className: "empty-line" }, [
      "Select an artifact to preview.",
    ]));
    preview.appendChild(el("div", { className: "count" }, [
      count + (count === 1 ? " artifact on this run" : " artifacts on this run"),
    ]));
  }

  function renderArtifactMeta(artifact) {
    const entries = [
      ["role", artifact.role || ""],
      ["mime", artifact.mime_type || ""],
      ["path", artifact.path || ""],
      ["span_id", artifact.span_id || ""],
    ];
    if (artifact.event_id) entries.push(["event_id", artifact.event_id]);
    if (artifact.sha256) entries.push(["sha256", artifact.sha256]);
    if (artifact.summary) entries.push(["summary", artifact.summary]);
    if (isClickOverlayArtifact(artifact)) entries.push(["evidence", "click overlay"]);
    if (isClickOverlayAnnotation(artifact)) entries.push(["evidence", "click overlay annotation"]);
    const grid = el("div", { className: "attrs" }, []);
    for (const [k, v] of entries) {
      grid.appendChild(el("span", { className: "k" }, [k]));
      grid.appendChild(el("span", { className: "v" }, [String(v)]));
    }
    return grid;
  }

  function parseJsonObject(text) {
    try {
      const value = JSON.parse(text);
      return value && typeof value === "object" && !Array.isArray(value) ? value : null;
    } catch (err) {
      return null;
    }
  }

  function isSurfaceNodeLike(value) {
    return !!value
      && typeof value === "object"
      && !!value.node_ref
      && !!value.box;
  }

  function surfaceNodeKey(node) {
    if (!node || !node.node_ref) return "";
    return String(node.node_ref.node_id || "");
  }

  function surfaceNodeSelectionKey(artifactKey, node) {
    return artifactKey + "::" + surfaceNodeKey(node);
  }

  function surfaceNodeLabel(node) {
    if (!node) return "surface node";
    if (node.label) return node.label;
    if (node.kind) return node.kind;
    const ref = node.node_ref || {};
    return ref.node_id || "surface node";
  }

  function surfaceNodeMeta(node) {
    if (!node) return "";
    const ref = node.node_ref || {};
    const parts = [];
    if (ref.node_id) parts.push("node_id=" + ref.node_id);
    if (node.kind) parts.push("kind=" + node.kind);
    if (node.recognition_id) parts.push("recognition_id=" + node.recognition_id);
    if (node.recognition_source) parts.push("source=" + node.recognition_source);
    if (node.recognized_item_id) parts.push("item=" + node.recognized_item_id);
    if (node.provider_score != null) {
      parts.push("score=" + Number(node.provider_score).toFixed(2));
    }
    if (node.box) {
      parts.push(
        "box=" + node.box.x + "," + node.box.y + " "
          + node.box.width + "x" + node.box.height
      );
    }
    return parts.join(" · ");
  }

  function renderSurfaceNodeDetail(node) {
    const detail = el("div", { className: "surface-node-detail" }, []);
    if (!node) {
      detail.className = "surface-node-detail empty";
      detail.textContent = "select a node to inspect its provenance.";
      return detail;
    }

    const nodeRef = node.node_ref || {};
    const entries = [
      ["node_ref", nodeRef.node_id ? JSON.stringify(nodeRef) : ""],
      ["kind", node.kind || ""],
      ["label", node.label || ""],
      ["box", node.box ? JSON.stringify(node.box) : ""],
      ["recognition_id", node.recognition_id || ""],
      ["recognition_source", node.recognition_source || ""],
      ["recognition_surface", node.recognition_surface || ""],
      ["recognized_item_id", node.recognized_item_id || ""],
      ["recognized_item_kind", node.recognized_item_kind || ""],
      ["provider_score", node.provider_score == null ? "" : String(Number(node.provider_score).toFixed(2))],
      ["source_artifacts", Array.isArray(node.source_artifacts) ? node.source_artifacts.join(" · ") : ""],
    ];
    detail.appendChild(el("div", { className: "surface-node-detail-head" }, [
      el("span", { className: "k" }, ["selected node"]),
      el("span", { className: "v" }, [surfaceNodeLabel(node)]),
    ]));
    const grid = el("div", { className: "surface-node-detail-grid" }, []);
    for (const [k, v] of entries) {
      if (!v) continue;
      grid.appendChild(el("span", { className: "k" }, [k]));
      grid.appendChild(el("span", { className: "v" }, [String(v)]));
    }
    if (grid.childNodes.length) detail.appendChild(grid);
    if (node.detail != null) {
      detail.appendChild(el("pre", { className: "surface-node-detail-json" }, [
        stringifyJsonPretty(node.detail),
      ]));
    }
    return detail;
  }

  function stringifyJsonPretty(value) {
    try {
      return JSON.stringify(value, null, 2);
    } catch (err) {
      return String(value);
    }
  }


  function renderKeyValueSummary(entries) {
    const card = el("div", { className: "json-summary" }, []);
    for (const [k, v] of entries) {
      if (v == null || v === "") continue;
      card.appendChild(el("span", { className: "k" }, [String(k)]));
      card.appendChild(el("span", { className: "v" }, [String(v)]));
    }
    return card;
  }

  function trainingCompatibilityViewSummary(parsed) {
    const views = Array.isArray(parsed.compatibility_views) ? parsed.compatibility_views : [];
    return views.length ? views[0] : null;
  }

  function trainingTransformsPresence(view) {
    return view && view.transforms_path ? "present" : "none";
  }

  function countArray(value) {
    return Array.isArray(value) ? value.length : 0;
  }

  function transformsPresenceFromPath(path) {
    return path ? "present" : "none";
  }

  function renderNormalizedArtifactRows(parsed) {
    const artifacts = Array.isArray(parsed && parsed.normalized_artifacts)
      ? parsed.normalized_artifacts
      : [];
    if (!artifacts.length) return "—";
    return artifacts.map(function (artifact) {
      const byteSize = artifact && artifact.byte_size != null ? artifact.byte_size : "n/a";
      return (artifact.kind || "unknown")
        + " · " + (artifact.relative_path || "—")
        + " · readable=" + String(!!artifact.readable)
        + " · bytes=" + byteSize;
    }).join(" | ");
  }

  function renderTrainerLineageSummaryCard(artifact, parsed) {
    if (isMinecraftTrainingLaunchArtifact(artifact)) {
      return renderKeyValueSummary([
        ["kind", "training launch plan"],
        ["schema", parsed.schema_version],
        ["source_training_package", parsed.source_training_package_manifest_path],
        ["scene_packet", parsed.source_scene_packet_manifest_path],
        ["source_runs", countArray(parsed.source_run_ids)],
        ["frames", parsed.counts && parsed.counts.frames],
        ["images", parsed.counts && parsed.counts.images],
        ["trainer_backend", parsed.trainer_backend],
        ["training_data_dir", parsed.training_data_dir],
        ["transforms", transformsPresenceFromPath(parsed.transforms_path)],
        ["launch_command", parsed.launch_command],
        ["known_limits", countArray(parsed.known_limits)],
      ]);
    }
    if (isMinecraftTrainingLaunchInspectArtifact(artifact)) {
      return renderKeyValueSummary([
        ["kind", "training launch inspect report"],
        ["schema", parsed.schema_version],
        ["manifest_path", parsed.training_launch_manifest_path],
        ["scene_packet", parsed.source_scene_packet_manifest_path],
        ["source_runs", countArray(parsed.source_run_ids)],
        ["compatibility_status", parsed.compatibility_status],
        ["trainer_readiness", parsed.trainer_readiness],
        ["readiness_blocker", parsed.readiness_blocker],
        ["probe_succeeded", parsed.probe_succeeded],
        ["exported", parsed.exported_frame_count],
        ["skipped", parsed.skipped_frame_count],
        ["transforms", parsed.transforms_present ? "present" : "none"],
        ["warnings", countArray(parsed.warnings)],
        ["known_limits", countArray(parsed.known_limits)],
      ]);
    }
    if (isMinecraftTrainingJobArtifact(artifact)) {
      return renderKeyValueSummary([
        ["kind", "training job manifest"],
        ["schema", parsed.schema_version],
        ["source_training_launch_plan", parsed.source_training_launch_plan_path],
        ["source_runs", countArray(parsed.source_run_ids)],
        ["trainer_backend", parsed.trainer_backend],
        ["job_backend", parsed.job_backend],
        ["status", parsed.status],
        ["job_id", parsed.job_id],
        ["job_submission_endpoint", parsed.job_submission_endpoint],
        ["exported", parsed.counts && parsed.counts.compatibility_exported_frames],
        ["skipped", parsed.counts && parsed.counts.compatibility_skipped_frames],
        ["transforms", transformsPresenceFromPath(parsed.transforms_path)],
        ["known_limits", countArray(parsed.known_limits)],
      ]);
    }
    if (isMinecraftTrainingJobInspectArtifact(artifact)) {
      return renderKeyValueSummary([
        ["kind", "training job inspect report"],
        ["schema", parsed.schema_version],
        ["source_training_launch_plan", parsed.source_training_launch_plan_path],
        ["source_runs", countArray(parsed.source_run_ids)],
        ["trainer_backend", parsed.trainer_backend],
        ["job_backend", parsed.job_backend],
        ["status", parsed.status],
        ["job_id", parsed.job_id],
        ["readiness_blocker", parsed.readiness_blocker],
        ["job_submission_endpoint", parsed.job_submission_endpoint],
        ["job_submission_command", parsed.job_submission_command],
        ["probe_succeeded", parsed.probe_succeeded],
        ["exported", parsed.exported_frame_count],
        ["skipped", parsed.skipped_frame_count],
        ["transforms", parsed.transforms_present ? "present" : "none"],
        ["warnings", countArray(parsed.warnings)],
        ["known_limits", countArray(parsed.known_limits)],
      ]);
    }
    if (isMinecraftTrainingResultArtifact(artifact)) {
      return renderKeyValueSummary([
        ["kind", "training result manifest"],
        ["schema", parsed.schema_version],
        ["source_training_job_manifest", parsed.source_training_job_manifest_path],
        ["source_runs", countArray(parsed.source_run_ids)],
        ["trainer_backend", parsed.trainer_backend],
        ["job_backend", parsed.job_backend],
        ["source_job_status", parsed.source_job_status],
        ["status", parsed.status],
        ["job_id", parsed.job_id],
        ["result_dir", parsed.result_dir],
        ["result_artifacts", countArray(parsed.result_artifacts)],
        ["exported", parsed.exported_frame_count],
        ["skipped", parsed.skipped_frame_count],
        ["known_limits", countArray(parsed.known_limits)],
      ]);
    }
    if (isMinecraftTrainingResultInspectArtifact(artifact)) {
      return renderKeyValueSummary([
        ["kind", "training result inspect report"],
        ["schema", parsed.schema_version],
        ["source_training_job_manifest", parsed.source_training_job_manifest_path],
        ["source_runs", countArray(parsed.source_run_ids)],
        ["trainer_backend", parsed.trainer_backend],
        ["job_backend", parsed.job_backend],
        ["source_job_status", parsed.source_job_status],
        ["status", parsed.status],
        ["status_reason", parsed.status_reason],
        ["job_id", parsed.job_id],
        ["result_dir", parsed.result_dir],
        ["result_dir_exists", parsed.result_dir_exists],
        ["key_result_artifacts_present", parsed.key_result_artifacts_present],
        ["result_artifact_count", parsed.result_artifact_count],
        ["warnings", countArray(parsed.warnings)],
        ["known_limits", countArray(parsed.known_limits)],
      ]);
    }
    if (isMinecraftTrainingResultArtifactFetchManifestArtifact(artifact)) {
      return renderKeyValueSummary([
        ["kind", "training result artifact fetch manifest"],
        ["schema", parsed.schema_version],
        ["source_training_result_manifest", parsed.source_training_result_manifest_path],
        ["source_runs", countArray(parsed.source_run_ids)],
        ["trainer_backend", parsed.trainer_backend],
        ["job_backend", parsed.job_backend],
        ["source_job_status", parsed.source_job_status],
        ["source_result_status", parsed.source_result_status],
        ["source_result_status_reason", parsed.source_result_status_reason],
        ["normalized_result_dir", parsed.normalized_result_dir],
        ["normalized_artifacts", renderNormalizedArtifactRows(parsed)],
        ["known_limits", countArray(parsed.known_limits)],
      ]);
    }
    if (isMinecraftTrainingResultArtifactFetchInspectArtifact(artifact)) {
      return renderKeyValueSummary([
        ["kind", "training result artifact fetch inspect report"],
        ["schema", parsed.schema_version],
        ["manifest_path", parsed.training_result_artifact_fetch_manifest_path],
        ["source_runs", countArray(parsed.source_run_ids)],
        ["trainer_backend", parsed.trainer_backend],
        ["job_backend", parsed.job_backend],
        ["source_job_status", parsed.source_job_status],
        ["source_result_status", parsed.source_result_status],
        ["source_result_status_reason", parsed.source_result_status_reason],
        ["fetch_status", parsed.fetch_status],
        ["fetch_reason", parsed.fetch_reason],
        ["source_result_dir_exists", parsed.source_result_dir_exists],
        ["required_artifacts_present", parsed.required_artifacts_present],
        ["normalized_artifact_count", parsed.normalized_artifact_count],
        ["warnings", countArray(parsed.warnings)],
        ["known_limits", countArray(parsed.known_limits)],
      ]);
    }

    if (isMinecraftTrainingResultHoldoutPreviewManifestArtifact(artifact)) {
      return renderKeyValueSummary([
        ["kind", "training result holdout preview manifest"],
        ["schema", parsed.schema_version],
        ["training_result_semantic_manifest", parsed.training_result_semantic_manifest_path],
        ["holdout_frame_index", parsed.holdout_frame_index],
        ["spatial_frame_id", parsed.holdout_frame && parsed.holdout_frame.spatial_frame_id],
        ["status", parsed.status],
        ["reason", parsed.reason],
        ["basis_checkpoint_path", parsed.basis_checkpoint_path],
        ["holdout_screenshot_path", parsed.holdout_screenshot_path],
        ["reference_overlay_path", parsed.reference_overlay_path],
        ["known_limits", countArray(parsed.known_limits)],
      ]);
    }
    if (isMinecraftTrainingResultHoldoutPreviewInspectArtifact(artifact)) {
      return renderKeyValueSummary([
        ["kind", "training result holdout preview inspect report"],
        ["schema", parsed.schema_version],
        ["manifest_path", parsed.training_result_holdout_preview_manifest_path],
        ["holdout_frame_index", parsed.holdout_frame_index],
        ["status", parsed.status],
        ["reason", parsed.reason],
        ["holdout_frame_selection", parsed.holdout_frame_selection],
        ["checkpoint_count", parsed.checkpoint_count],
        ["scene_packet_frame_count", parsed.scene_packet_frame_count],
        ["warnings", countArray(parsed.warnings)],
        ["known_limits", countArray(parsed.known_limits)],
      ]);
    }
    if (isMinecraftTrainingResultSpatialQueryManifestArtifact(artifact)) {
      const actionReadiness = deriveSpatialQueryActionReadiness(parsed);
      const entries = [
        ["kind", "training result spatial query manifest"],
        ["schema", parsed.schema_version],
        ["training_result_semantic_manifest", parsed.training_result_semantic_manifest_path],
        ["target_block", parsed.target_block && [parsed.target_block.x, parsed.target_block.y, parsed.target_block.z].join(",")],
        ["target_face", parsed.target_face],
        ["target_semantics", parsed.target_semantics],
        ["query_kind", parsed.query_kind],
        ["selected_backend", parsed.selected_backend],
        ["status", parsed.status],
        ["reason", parsed.reason],
        ["visibility", parsed.visibility],
        ["screen_point", parsed.screen_point && [parsed.screen_point.x, parsed.screen_point.y].join(",")],
        ["basis_frame_id", parsed.basis_frame_id],
        ["comparison_verdict", parsed.comparison_verdict],
        ["action_eligibility", actionReadiness.action_eligibility],
        ["window_point", actionReadiness.window_point],
        ["refusal_reason", actionReadiness.refusal_reason],
        ["known_limits", countArray(parsed.known_limits)],
      ];
      entries.push.apply(entries, wiredActionRowsForQueryManifest(artifact, state.events, state.artifacts));
      return renderKeyValueSummary(entries);
    }
    if (isMinecraftTrainingResultSpatialQueryInspectArtifact(artifact)) {
      return renderKeyValueSummary([
        ["kind", "training result spatial query inspect report"],
        ["schema", parsed.schema_version],
        ["manifest_path", parsed.training_result_spatial_query_manifest_path],
        ["provider_status", parsed.provider_status],
        ["reference_status", parsed.reference_status],
        ["comparison_verdict", parsed.comparison_verdict],
        ["visibility", parsed.visibility],
        ["scene_packet_frame_count", parsed.scene_packet_frame_count],
        ["warnings", countArray(parsed.warnings)],
        ["known_limits", countArray(parsed.known_limits)],
      ]);
    }

    if (isMinecraftHoldoutRenderQualityManifestArtifact(artifact)) {
      const metrics = parsed.metrics || {};
      return renderKeyValueSummary([
        ["kind", "holdout render quality manifest"],
        ["schema", parsed.schema_version],
        ["training_result_semantic_manifest", parsed.training_result_semantic_manifest_path],
        ["holdout_preview_manifest", parsed.holdout_preview_manifest_path],
        ["holdout_frame_index", parsed.holdout_frame_index],
        ["status", parsed.status],
        ["verdict", parsed.verdict],
        ["image_size_match", parsed.image_size_match],
        ["l1_mean", metrics.l1_mean],
        ["mse", metrics.mse],
        ["psnr", metrics.psnr],
        ["known_limits", countArray(parsed.known_limits)],
      ]);
    }
    if (isMinecraftHoldoutRenderQualityInspectArtifact(artifact)) {
      const metrics = parsed.metrics || {};
      return renderKeyValueSummary([
        ["kind", "holdout render quality inspect report"],
        ["schema", parsed.schema_version],
        ["manifest_path", parsed.training_result_holdout_render_quality_manifest_path],
        ["status", parsed.status],
        ["verdict", parsed.verdict],
        ["image_size_match", parsed.image_size_match],
        ["l1_mean", metrics.l1_mean],
        ["mse", metrics.mse],
        ["psnr", metrics.psnr],
        ["warnings", countArray(parsed.warnings)],
        ["known_limits", countArray(parsed.known_limits)],
      ]);
    }
    if (isMinecraftTrainingResultSemanticManifestArtifact(artifact)) {
      return renderKeyValueSummary([
        ["kind", "training result semantic manifest"],
        ["schema", parsed.schema_version],
        ["source_training_result_artifact_manifest", parsed.source_training_result_artifact_manifest_path],
        ["source_training_result_manifest", parsed.source_training_result_manifest_path],
        ["source_runs", countArray(parsed.source_run_ids)],
        ["trainer_backend", parsed.trainer_backend],
        ["job_backend", parsed.job_backend],
        ["source_result_status", parsed.source_result_status],
        ["normalized_result_dir", parsed.normalized_result_dir],
        ["semantic_status", parsed.semantic_status],
        ["semantic_reason", parsed.semantic_reason],
        ["config_path", parsed.config_path],
        ["models_dir_path", parsed.models_dir_path],
        ["status_snapshot_path", parsed.status_snapshot_path],
        ["config_trainer", parsed.config_trainer],
        ["checkpoint_count", parsed.checkpoint_count],
        ["checkpoint_files", countArray(parsed.checkpoint_files)],
        ["known_limits", countArray(parsed.known_limits)],
      ]);
    }
    if (isMinecraftTrainingResultSemanticInspectArtifact(artifact)) {
      return renderKeyValueSummary([
        ["kind", "training result semantic inspect report"],
        ["schema", parsed.schema_version],
        ["manifest_path", parsed.training_result_semantic_manifest_path],
        ["source_training_result_artifact_manifest", parsed.source_training_result_artifact_manifest_path],
        ["source_runs", countArray(parsed.source_run_ids)],
        ["trainer_backend", parsed.trainer_backend],
        ["job_backend", parsed.job_backend],
        ["source_result_status", parsed.source_result_status],
        ["normalized_result_dir", parsed.normalized_result_dir],
        ["semantic_status", parsed.semantic_status],
        ["semantic_reason", parsed.semantic_reason],
        ["config_yaml_parsed", parsed.config_yaml_parsed],
        ["config_trainer", parsed.config_trainer],
        ["config_backend_matches", parsed.config_backend_matches],
        ["models_dir_readable", parsed.models_dir_readable],
        ["status_snapshot_present", parsed.status_snapshot_present],
        ["checkpoint_count", parsed.checkpoint_count],
        ["warnings", countArray(parsed.warnings)],
        ["known_limits", countArray(parsed.known_limits)],
      ]);
    }
    return null;
  }

  function renderTrainingPackageSummaryCard(artifact, parsed) {
    const view = trainingCompatibilityViewSummary(parsed);
    if (isMinecraftTrainingPackageArtifact(artifact)) {
      return renderKeyValueSummary([
        ["kind", "training package manifest"],
        ["schema", parsed.schema_version],
        ["source_scene_packet", parsed.source_scene_packet_manifest_path],
        ["source_runs", Array.isArray(parsed.source_run_ids) ? parsed.source_run_ids.length : 0],
        ["frames", parsed.counts && parsed.counts.frames],
        ["images", parsed.counts && parsed.counts.images],
        ["compatibility_view", view && view.view_name],
        ["compatibility_status", view && view.status],
        ["exported", view && view.exported_frame_count],
        ["skipped", view && view.skipped_frame_count],
        ["transforms", trainingTransformsPresence(view)],
        ["warnings", view && Array.isArray(view.warnings) ? view.warnings.length : 0],
        ["known_limits", Array.isArray(parsed.known_limits) ? parsed.known_limits.length : 0],
      ]);
    }
    if (isMinecraftTrainingPackageInspectArtifact(artifact)) {
      return renderKeyValueSummary([
        ["kind", "training package inspect report"],
        ["schema", parsed.schema_version],
        ["manifest_path", parsed.training_package_manifest_path],
        ["scene_packet", parsed.scene_packet_manifest_path],
        ["source_runs", Array.isArray(parsed.source_run_ids) ? parsed.source_run_ids.length : 0],
        ["frames", parsed.counts && parsed.counts.frames],
        ["images", parsed.counts && parsed.counts.images],
        ["compatibility_view", view && view.view_name],
        ["compatibility_status", view && view.status],
        ["exported", view && view.exported_frame_count],
        ["skipped", view && view.skipped_frame_count],
        ["transforms", trainingTransformsPresence(view)],
        ["warnings", Array.isArray(parsed.warnings) ? parsed.warnings.length : 0],
        ["known_limits", Array.isArray(parsed.known_limits) ? parsed.known_limits.length : 0],
      ]);
    }
    return null;
  }

  function renderSurfaceNodeRow(node, artifactKey, nodes, detailNode, buttonMap) {
    const button = el("button", { className: "surface-node-button" }, [
      el("div", { className: "surface-node-title" }, [surfaceNodeLabel(node)]),
      el("div", { className: "surface-node-meta" }, [surfaceNodeMeta(node)]),
    ]);
    button.addEventListener("click", function () {
      const selectedKey = surfaceNodeKey(node);
      state.activeSurfaceNodeArtifactKey = artifactKey;
      state.activeSurfaceNodeKey = selectedKey;
      detailNode.innerHTML = "";
      detailNode.appendChild(renderSurfaceNodeDetail(node));
      for (const key in buttonMap) {
        buttonMap[key].classList.toggle("active", key === selectedKey);
      }
    });
    buttonMap[surfaceNodeKey(node)] = button;
    return button;
  }

  function renderSurfaceNodesPanel(artifact, nodes) {
    const panel = el("div", { className: "surface-nodes" }, []);
    const artifactKeyValue = artifactKey(artifact);
    const previewNodes = nodes.slice(0, 4);
    const remaining = nodes.length - previewNodes.length;
    const nodeMap = {};
    let selectedNode = null;
    if (state.activeSurfaceNodeArtifactKey === artifactKeyValue && state.activeSurfaceNodeKey) {
      selectedNode = nodes.find(function (node) {
        return surfaceNodeKey(node) === state.activeSurfaceNodeKey;
      }) || null;
    }
    if (!selectedNode) {
      selectedNode = nodes[0] || null;
      state.activeSurfaceNodeArtifactKey = artifactKeyValue;
      state.activeSurfaceNodeKey = selectedNode ? surfaceNodeKey(selectedNode) : null;
    }
    panel.appendChild(el("div", { className: "surface-nodes-head" }, [
      el("span", { className: "k" }, ["nodes"]),
      el("span", { className: "v" }, [
        nodes.length + " node" + (nodes.length === 1 ? "" : "s")
          + " in " + (artifact.role || "artifact"),
      ]),
    ]));
    const detailNode = renderSurfaceNodeDetail(selectedNode);
    panel.appendChild(detailNode);
    const list = el("div", { className: "surface-node-list" }, []);
    for (const node of previewNodes) {
      const button = renderSurfaceNodeRow(node, artifactKeyValue, nodes, detailNode, nodeMap);
      if (surfaceNodeKey(node) === state.activeSurfaceNodeKey) {
        button.classList.add("active");
      }
      list.appendChild(el("div", { className: "surface-node" }, [button]));
    }
    if (remaining > 0) {
      list.appendChild(el("div", { className: "surface-node more" }, ["+" + remaining + " more"]));
    }
    panel.appendChild(list);
    return panel;
  }

  function renderEvidenceSummaryContainer() {
    return el("div", { className: "evidence-summary loading" }, [
      el("span", { className: "k" }, ["evidence"]),
      el("span", { className: "v" }, ["loading click overlay annotation…"]),
    ]);
  }

  function pointSummary(point) {
    if (!point) return "—";
    const sx = Number(point.screenshot_x);
    const sy = Number(point.screenshot_y);
    const lx = Number(point.logical_x);
    const ly = Number(point.logical_y);
    return "screen=(" + sx.toFixed(1) + "," + sy.toFixed(1)
      + ") logical=(" + lx.toFixed(1) + "," + ly.toFixed(1) + ")";
  }

  function renderEvidenceSummary(container, payload, annotationArtifact) {
    container.className = "evidence-summary";
    container.innerHTML = "";
    const rows = [
      ["kind", payload.kind || "click.overlay"],
      ["query", payload.query || "—"],
      ["strategy", payload.strategy || "—"],
      ["fallback", payload.fallback_used == null ? "—" : String(payload.fallback_used)],
      ["cursor", payload.cursor_disturbance || "—"],
      ["press", payload.press_mechanism || "—"],
      ["overlay", payload.overlay_presentation || "—"],
      ["expected", pointSummary(payload.expected_target)],
      ["actual", pointSummary(payload.action_point)],
    ];
    if (payload.ocr_match) rows.push([
      "ocr",
      (payload.ocr_match.text || "—") + " · confidence="
        + Number(payload.ocr_match.confidence || 0).toFixed(2),
    ]);
    if (payload.row) rows.push([
      "row",
      "#" + (Number(payload.row.row_index || 0) + 1)
        + " · " + (payload.row.source || "—")
        + " · " + ((payload.row.text_fragments || []).join(" | ") || "—"),
    ]);
    if (payload.decision) rows.push([
      "decision",
      (payload.decision.primary_strategy || "—") + " -> "
        + (payload.decision.selected_strategy || "—"),
    ]);
    if (payload.decision && payload.decision.primary_error) rows.push([
      "primary_error",
      payload.decision.primary_error,
    ]);
    rows.push(["annotation", annotationArtifact ? annotationArtifact.artifact_id : "—"]);

    for (const [k, v] of rows) {
      container.appendChild(el("span", { className: "k" }, [k]));
      container.appendChild(el("span", { className: "v" }, [String(v)]));
    }
  }

  function loadEvidenceSummary(container, overlayArtifact, artifacts, requestId) {
    const annotation = findClickOverlayAnnotationArtifact(overlayArtifact, artifacts);
    if (!annotation) {
      container.className = "evidence-summary error";
      container.innerHTML = "";
      container.appendChild(el("span", { className: "k" }, ["evidence"]));
      container.appendChild(el("span", { className: "v" }, ["missing click.overlay.annotation artifact"]));
      return;
    }
    const annotationUrl = artifactUrl(annotation);
    fetch(annotationUrl)
      .then(function (response) {
        if (!response.ok) throw new Error("HTTP " + response.status);
        return response.json();
      })
      .then(function (payload) {
        if (state.activeEvidenceRequestId !== requestId) return;
        renderEvidenceSummary(container, payload || {}, annotation);
      })
      .catch(function (err) {
        if (state.activeEvidenceRequestId !== requestId) return;
        container.className = "evidence-summary error";
        container.innerHTML = "";
        container.appendChild(el("span", { className: "k" }, ["evidence"]));
        container.appendChild(el("span", { className: "v" }, [
          "failed to load click overlay annotation: " + err.message,
        ]));
      });
  }

  function renderArtifactPreview(artifact, artifacts) {
    const preview = document.getElementById("artifact-preview");
    state.activeEvidenceRequestId += 1;
    if (!artifact) {
      renderArtifactPreviewEmpty(artifacts ? artifacts.length : 0);
      return;
    }
    if (isClickOverlayAnnotation(artifact)) {
      const overlay = findClickOverlayImageArtifact(artifact, artifacts);
      if (overlay) {
        state.activeArtifactKey = artifactKey(overlay);
        renderArtifactList(state.artifacts);
        renderArtifactPreview(overlay, artifacts);
        return;
      }
    }
    preview.className = "artifact-preview detail";
    preview.innerHTML = "";
    preview.appendChild(renderArtifactMeta(artifact));

    const url = artifactUrl(artifact);
    const surface = el("div", { className: "surface" }, []);
    preview.appendChild(surface);

    if (isImageMime(artifact.mime_type)) {
      surface.className = "surface image";
      surface.appendChild(el("img", { src: url, alt: artifact.role || "" }));
      if (isClickOverlayArtifact(artifact)) {
        const summary = renderEvidenceSummaryContainer();
        preview.appendChild(summary);
        loadEvidenceSummary(summary, artifact, artifacts, state.activeEvidenceRequestId);
      }
      return;
    }

    if (isTextLikeMime(artifact.mime_type)) {
      surface.appendChild(el("pre", null, ["loading…"]));
      const expectedArtifactKey = artifactKey(artifact);
      fetch(url)
        .then(function (response) {
          if (!response.ok) throw new Error("HTTP " + response.status);
          return response.text();
        })
        .then(function (text) {
          if (state.activeArtifactKey !== expectedArtifactKey) return;
          const parsed = parseJsonObject(text);
          surface.innerHTML = "";
          if (parsed && Array.isArray(parsed.nodes)) {
            const nodes = parsed.nodes.filter(isSurfaceNodeLike);
            if (nodes.length) {
              preview.insertBefore(renderSurfaceNodesPanel(artifact, nodes), surface);
            }
          }
          if (parsed) {
            state.artifactJsonCache[expectedArtifactKey] = parsed;
          }

          if (
            isMinecraftHoldoutRenderQualityManifestArtifact(artifact)
            || isMinecraftHoldoutRenderQualityInspectArtifact(artifact)
            || isMinecraftTrainingResultSpatialQueryManifestArtifact(artifact)
            || isMinecraftTrainingResultHoldoutPreviewManifestArtifact(artifact)
          ) {
            loadQualityBaselineSummaryCard(preview, surface, state.activeEvidenceRequestId);
          }
          const mc19Card = renderQueryWiredLiveActionSummaryCard(artifact, parsed, state.events, state.artifacts);
          if (mc19Card) {
            preview.insertBefore(mc19Card, surface);
          }
          if (parsed && (
            isMinecraftTrainingPackageArtifact(artifact)
            || isMinecraftTrainingPackageInspectArtifact(artifact)
            || isMinecraftTrainingLaunchArtifact(artifact)
            || isMinecraftTrainingLaunchInspectArtifact(artifact)
            || isMinecraftTrainingJobArtifact(artifact)
            || isMinecraftTrainingJobInspectArtifact(artifact)
            || isMinecraftTrainingResultArtifact(artifact)
            || isMinecraftTrainingResultInspectArtifact(artifact)
            || isMinecraftTrainingResultArtifactFetchManifestArtifact(artifact)
            || isMinecraftTrainingResultArtifactFetchInspectArtifact(artifact)
            || isMinecraftTrainingResultSemanticManifestArtifact(artifact)
            || isMinecraftTrainingResultSemanticInspectArtifact(artifact)
            || isMinecraftTrainingResultHoldoutPreviewManifestArtifact(artifact)
            || isMinecraftTrainingResultHoldoutPreviewInspectArtifact(artifact)
            || isMinecraftTrainingResultSpatialQueryManifestArtifact(artifact)
            || isMinecraftTrainingResultSpatialQueryInspectArtifact(artifact)
            || isMinecraftHoldoutRenderQualityManifestArtifact(artifact)
            || isMinecraftHoldoutRenderQualityInspectArtifact(artifact)
          )) {
            const summaryCard = renderTrainingPackageSummaryCard(artifact, parsed)
              || renderTrainerLineageSummaryCard(artifact, parsed);
            if (summaryCard) {
              preview.insertBefore(summaryCard, surface);
            }
          }

          if (isMinecraftTrainingResultSpatialQueryManifestArtifact(artifact) && state.artifacts) {
            state.artifacts.forEach(function (candidate) {
              if (!isOperationResultArtifact(candidate)) return;
              const key = artifactKey(candidate);
              if (state.artifactJsonCache[key]) return;
              fetch(artifactUrl(candidate))
                .then(function (response) {
                  if (!response.ok) return null;
                  return response.text();
                })
                .then(function (body) {
                  if (!body || state.activeArtifactKey !== expectedArtifactKey) return;
                  const opParsed = parseJsonObject(body);
                  if (!opParsed) return;
                  state.artifactJsonCache[key] = opParsed;
                  const summaryCard = renderTrainerLineageSummaryCard(artifact, parsed);
                  if (summaryCard) {
                    const existingCards = preview.querySelectorAll(".json-summary");
                    existingCards.forEach(function (node) { node.remove(); });
                    preview.insertBefore(summaryCard, surface);
                  }
                })
                .catch(function () {});
            });
          }

          surface.appendChild(el("pre", null, [text]));
        })
        .catch(function (err) {
          if (state.activeArtifactKey !== expectedArtifactKey) return;
          surface.className = "surface error";
          surface.textContent = "failed to load artifact bytes: " + err.message;
        });
      return;
    }

    surface.className = "surface binary";
    surface.textContent = "binary · " + (artifact.mime_type || "unknown mime");
  }

  // -- C.4 WebSocket live streaming -----------------------------------------

  function isRunning(run) {
    return !!run && (run.state === "running" || run.state === "started");
  }

  function streamEndpoint(runId) {
    const proto = (location.protocol === "https:") ? "wss:" : "ws:";
    return proto + "//" + location.host + "/runs/" + encodeURIComponent(runId) + "/stream";
  }

  function openStream(runId) {
    // The viewer keeps a single live connection at a time; selectRun
    // closes any prior socket before reaching here, so this is just an
    // open call.
    let socket;
    try {
      socket = new WebSocket(streamEndpoint(runId));
    } catch (err) {
      setConnection(false, "/runs (ws open failed: " + err.message + ")");
      return;
    }
    state.ws = socket;
    state.streamRunId = runId;
    setConnection(true, streamEndpoint(runId));

    socket.addEventListener("message", function (event) {
      if (state.streamRunId !== runId) return;
      try {
        onStreamMessage(JSON.parse(event.data));
      } catch (err) {
        // ignore malformed frames; keep the stream alive.
      }
    });
    socket.addEventListener("close", function () {
      if (state.streamRunId !== runId) return;
      onStreamClosed(runId, false);
    });
    socket.addEventListener("error", function () {
      if (state.streamRunId !== runId) return;
      onStreamClosed(runId, true);
    });
  }

  function closeStream() {
    if (state.ws) {
      try { state.ws.close(); } catch (e) { /* socket already torn */ }
    }
    state.ws = null;
    state.streamRunId = null;
    state.retryScheduled = false;
    setConnection(true, location.host + "/runs");
  }

  function onStreamClosed(runId, isError) {
    if (state.streamRunId !== runId) return;
    state.ws = null;
    if (isError && !state.retryScheduled) {
      state.retryScheduled = true;
      setConnection(false, "/runs/:id/stream (reconnecting in 2s)");
      window.setTimeout(function () {
        if (state.activeRunId !== runId) return;
        if (state.activeRun && isRunning(state.activeRun)) {
          openStream(runId);
        } else {
          setConnection(true, location.host + "/runs");
        }
      }, 2000);
      return;
    }
    state.retryScheduled = false;
    state.streamRunId = null;
    if (state.activeRunId === runId && state.activeRun && isRunning(state.activeRun)) {
      setConnection(false, "/runs/:id/stream (disconnected)");
    } else {
      setConnection(true, location.host + "/runs");
    }
  }

  function onStreamMessage(frame) {
    if (!frame || typeof frame !== "object") return;
    const run = state.activeRun;
    switch (frame.type) {
      case "span_started":
      case "span_finished":
        upsertSpan(frame.span);
        if (run) {
          renderSpanTree(run, state.spans);
          if (state.activeSpanId) {
            renderSpanDetail(findSpan(state.activeSpanId));
          }
        }
        break;
      case "event_appended": {
        const ev = frame.event;
        if (ev) {
          ev._live = true;
          state.events.push(ev);
          renderEventList(state.events, run);
        }
        break;
      }
      case "artifact_created": {
        const artifact = frame.artifact;
        if (artifact) {
          state.artifacts.push(artifact);
          if (state.activeSpanId && artifact.span_id === state.activeSpanId) {
            const preferredArtifactKey = preferredArtifactKeyForSpan(
              state.activeSpanId,
              state.artifacts
            );
            if (preferredArtifactKey) state.activeArtifactKey = preferredArtifactKey;
          } else if (isClickOverlayArtifact(artifact)) {
            state.activeArtifactKey = artifactKey(artifact);
          }
          renderArtifactList(state.artifacts);
          renderArtifactPreview(findArtifact(state.activeArtifactKey), state.artifacts);
        }
        break;
      }
      case "run_finished":
        if (frame.run) {
          state.activeRun = mergeRunDetail(state.activeRun, frame.run);
          // Reflect the finished state in the sidebar + run header.
          const idx = state.runs.findIndex(function (r) { return r.run_id === frame.run.run_id; });
          if (idx >= 0) state.runs[idx] = state.activeRun;
          setMainHeader(state.activeRun, false);
          if (!state.activeSpanId) renderSpanDetail(null);
          renderRunList();
          if (frame.run.run_id === state.activeRunId) {
            refreshViewParserProofFromRunDetail(frame.run.run_id);
          }
        }
        // Server closes the socket after RunFinished; tear our side down too.
        closeStream();
        break;
      default:
        break;
    }
  }

  function upsertSpan(span) {
    if (!span || !span.span_id) return;
    const idx = state.spans.findIndex(function (s) { return s.span_id === span.span_id; });
    if (idx >= 0) state.spans[idx] = span;
    else state.spans.push(span);
  }

  function orderedSpans(run, spans) {
    const byParent = new Map();
    for (const span of spans) {
      const parent = span.parent_span_id || "";
      if (!byParent.has(parent)) byParent.set(parent, []);
      byParent.get(parent).push(span);
    }
    for (const list of byParent.values()) {
      list.sort(function (a, b) {
        return (a.started_at_millis || 0) - (b.started_at_millis || 0);
      });
    }
    const seen = new Set();
    const out = [];
    function visit(span) {
      if (!span || seen.has(span.span_id)) return;
      seen.add(span.span_id);
      out.push(span);
      const children = byParent.get(span.span_id) || [];
      for (const child of children) visit(child);
    }
    const root = spans.find(function (span) { return span.span_id === run.root_span_id; });
    if (root) visit(root);
    const roots = (byParent.get("") || []).filter(function (span) { return span.span_id !== run.root_span_id; });
    for (const span of roots) visit(span);
    for (const span of spans) visit(span);
    return out;
  }

  function depthMapFor(spans) {
    const byId = new Map(spans.map(function (span) { return [span.span_id, span]; }));
    const cache = new Map();
    function depthOf(span) {
      if (!span) return 0;
      if (cache.has(span.span_id)) return cache.get(span.span_id);
      if (!span.parent_span_id || !byId.has(span.parent_span_id)) {
        cache.set(span.span_id, 0);
        return 0;
      }
      const depth = depthOf(byId.get(span.parent_span_id)) + 1;
      cache.set(span.span_id, depth);
      return depth;
    }
    for (const span of spans) depthOf(span);
    return cache;
  }

  function renderSpanTree(run, spans) {
    const body = document.getElementById("main-body");
    body.className = "span-tree";
    body.innerHTML = "";
    body.appendChild(el("div", { className: "span-head" }, [
      el("span", { className: "span-sigil" }),
      el("span", { className: "span-name" }, ["span · name / step_id"]),
      el("span", { className: "span-status" }, ["status"]),
      el("span", { className: "span-dur" }, ["dur"]),
      el("span", { className: "span-timing-head" }, ["timing"]),
    ]));

    if (!spans.length) {
      body.appendChild(el("div", { className: "span-empty" }, ["no spans recorded for this run."]));
      return;
    }

    const ordered = orderedSpans(run, spans);
    const depthById = depthMapFor(spans);
    const runStarted = run.started_at_millis || 0;
    const runFinished = run.finished_at_millis;
    const knownRunDuration = runFinished == null ? null : Math.max(1, runFinished - runStarted);
    const maxSpanDuration = Math.max(1, ...spans.map(function (span) {
      return span.finished_at_millis == null ? 0 : Math.max(0, span.finished_at_millis - span.started_at_millis);
    }));

    ordered.forEach(function (span, index) {
      const glyph = spanGlyph(span);
      const duration = span.finished_at_millis == null ? null : Math.max(0, span.finished_at_millis - span.started_at_millis);
      const widthPct = duration == null ? 0 : Math.max(2, Math.min(100, (duration / maxSpanDuration) * 100));
      const offsetPct = knownRunDuration == null
        ? Math.min(60, index * 5)
        : Math.max(0, Math.min(98, ((span.started_at_millis - runStarted) / knownRunDuration) * 100));
      const depth = depthById.get(span.span_id) || 0;
      const attrs = span.attributes || {};
      const meta = [];
      if (attrs.step_id) meta.push("step_id=" + attrs.step_id);
      if (attrs.command_id) meta.push(String(attrs.command_id));

      const row = el("button", {
        className: "span-row" + (state.activeSpanId === span.span_id ? " active" : ""),
        dataset: { spanId: span.span_id },
        onClick: function () {
          state.activeSpanId = span.span_id;
          const preferredArtifactKey = preferredArtifactKeyForSpan(span.span_id, state.artifacts);
          if (preferredArtifactKey) {
            state.activeArtifactKey = preferredArtifactKey;
          }
          renderSpanTree(run, spans);
          renderSpanDetail(span);
          renderArtifactList(state.artifacts);
          renderArtifactPreview(findArtifact(state.activeArtifactKey), state.artifacts);
        },
      }, [
        el("span", {
          className: "span-sigil",
          style: "color: " + glyph.color + ";" + (glyph.pulse ? " animation: auv-pulse 1.2s linear infinite;" : ""),
        }, [glyph.glyph]),
        el("span", {
          className: "span-name",
          style: "padding-left: " + (depth * 16) + "px",
        }, [
          el("span", { className: "primary" }, [span.name || span.span_id]),
          meta.length ? el("span", { className: "meta" }, ["  " + meta.join("  ")]) : null,
        ]),
        el("span", {
          className: "span-status",
          style: "color: " + statusColor(span.status_code, span.state),
        }, [statusLabel(span.status_code, span.state)]),
        el("span", { className: "span-dur" }, [fmtSeconds(duration)]),
        el("span", { className: "span-timing" }, [
          el("span", {
            className: "span-timing-fill",
            style: "left:" + offsetPct.toFixed(2) + "%;width:" + widthPct.toFixed(2) + "%;background:" + glyph.color + ";" + (duration == null ? "opacity:0.18;" : ""),
          }),
        ]),
      ]);
      body.appendChild(row);
    });
  }

  function setConnection(ok, endpoint) {
    const pill = document.getElementById("conn");
    const label = document.getElementById("conn-label");
    const endpointNode = document.getElementById("conn-endpoint");
    if (ok) {
      pill.classList.remove("bad");
      pill.classList.add("live");
      label.textContent = "live";
    } else {
      pill.classList.remove("live");
      pill.classList.add("bad");
      label.textContent = "disconnected";
    }
    if (endpoint) endpointNode.textContent = endpoint;
  }

  async function loadRuns() {
    try {
      const response = await fetch("/runs");
      if (!response.ok) throw new Error("HTTP " + response.status);
      const runs = await response.json();
      // Sort newest first by started_at_millis.
      runs.sort(function (a, b) {
        return (b.started_at_millis || 0) - (a.started_at_millis || 0);
      });
      state.runs = runs;
      state.fetchedAt = Date.now();
      setConnection(true, location.host + "/runs");
      renderRunListFilterChips({ enabled: true });
      renderRunList();
    } catch (err) {
      resetRunListFilterUiOnLoadFailure();
      setConnection(false, "/runs (" + err.message + ")");
      const list = document.getElementById("run-list");
      list.innerHTML = "";
      list.appendChild(el("div", { className: "run-row empty" }, [
        "failed to load /runs: " + err.message,
      ]));
    }
  }


  // NOTICE(core-c2-d2): cache miss must not fabricate query_manifest or derived_readiness.
  function selfTestSourceReadinessRef() {
    const runId = "run_viewer_self_test";
    const queryRole = "minecraft-3dgs-training-result-query";
    const queryArtifactId = "artifact_query_self_test";
    const runArtifacts = [{
      artifact_id: queryArtifactId,
      role: queryRole,
      span_id: "span_test",
      path: "/tmp/query.json",
    }];
    const baseParams = {
      queryArtifactId: queryArtifactId,
      operationResultArtifactId: null,
      runId: runId,
      hasOutcomeEvent: true,
      runArtifacts: runArtifacts,
    };

    const cacheMissRef = resolveQueryWiredLiveActionSourceReadinessRef(
      Object.assign({}, baseParams, { artifactJsonCache: {} })
    );
    console.assert(cacheMissRef === null, "cache miss should yield null source_readiness_ref");

    const cacheKey = "span_test::" + queryArtifactId;
    const invalidObjectRef = resolveQueryWiredLiveActionSourceReadinessRef(
      Object.assign({}, baseParams, { artifactJsonCache: { [cacheKey]: {} } })
    );
    console.assert(
      invalidObjectRef === null,
      "empty cached object should not yield query_manifest provenance"
    );

    const validManifestRef = resolveQueryWiredLiveActionSourceReadinessRef(
      Object.assign({}, baseParams, {
        artifactJsonCache: {
          [cacheKey]: mc19SelfTestManifestFixture(),
        },
      })
    );
    console.assert(
      validManifestRef === "kind=query_manifest artifact_id=" + queryArtifactId + " run_id=" + runId,
      "valid cached manifest should yield query_manifest provenance"
    );

    const partialManifestRef = resolveQueryWiredLiveActionSourceReadinessRef(
      Object.assign({}, baseParams, {
        artifactJsonCache: { [cacheKey]: { schema_version: 1, status: "answered" } },
      })
    );
    console.assert(
      partialManifestRef === null,
      "partial manifest without required MC-19 fields should not yield query_manifest provenance"
    );

    const cleanMissRef = resolveQueryWiredLiveActionSourceReadinessRef(
      Object.assign({}, baseParams, {
        queryArtifactId: "artifact_absent_from_run",
        artifactJsonCache: {},
      })
    );
    console.assert(
      cleanMissRef === "kind=derived_readiness query_artifact_id=artifact_absent_from_run run_id=" + runId,
      "clean miss should yield derived_readiness provenance"
    );
  }
  selfTestSourceReadinessRef();

  function selfTestRunListFilters() {
    const emptyFilters = new Set();
    const anyRun = { run_id: "r0", status_code: "ok", view_parser_summary: {} };
    console.assert(runMatchesListFilters(anyRun, emptyFilters), "empty filters should match any run");

    const staleRun = {
      run_id: "r1",
      status_code: "ok",
      view_parser_summary: { latest_outcome: "stale" },
    };
    const staleOnly = new Set(["stale"]);
    console.assert(runMatchesListFilters(staleRun, staleOnly), "stale filter should match stale outcome");
    console.assert(
      !runMatchesListFilters({
        run_id: "r2",
        status_code: "ok",
        view_parser_summary: { latest_outcome: "reacquired" },
      }, staleOnly),
      "stale filter should not match reacquired outcome"
    );

    const limitsOnly = new Set(["limits"]);
    console.assert(
      runMatchesListFilters({
        run_id: "r3",
        status_code: "ok",
        view_parser_summary: { has_known_limits: true },
      }, limitsOnly),
      "limits filter should match has_known_limits"
    );

    const failedOnly = new Set(["failed"]);
    console.assert(
      runMatchesListFilters({
        run_id: "r4",
        status_code: "ok",
        view_parser_summary: { latest_verification_status: "failed" },
      }, failedOnly),
      "failed filter should match verification failed"
    );
    console.assert(
      runMatchesListFilters({
        run_id: "r5",
        status_code: "error",
        view_parser_summary: {},
      }, failedOnly),
      "failed filter should match run status_code error"
    );
    console.assert(
      !runMatchesListFilters({
        run_id: "r6",
        status_code: "ok",
        view_parser_summary: {
          latest_outcome: "not_found",
          latest_verification_status: "passed",
        },
      }, failedOnly),
      "failed filter should not match not_found with passed verification"
    );

    const staleAndLimits = new Set(["stale", "limits"]);
    console.assert(
      runMatchesListFilters({
        run_id: "r7",
        status_code: "ok",
        view_parser_summary: { latest_outcome: "stale", has_known_limits: true },
      }, staleAndLimits),
      "AND filters should match when both predicates hold"
    );
    console.assert(
      !runMatchesListFilters({
        run_id: "r8",
        status_code: "ok",
        view_parser_summary: { latest_outcome: "stale", has_known_limits: false },
      }, staleAndLimits),
      "AND filters should not match when one predicate fails"
    );
    console.assert(
      !runMatchesListFilters({
        run_id: "r9",
        status_code: "ok",
        view_parser_summary: {},
      }, staleOnly),
      "degraded summary should not match stale filter"
    );

    console.assert(
      visibleRunsForList([staleRun, anyRun], staleOnly).length === 1,
      "visibleRunsForList should filter in place"
    );
    const runs = [staleRun, anyRun];
    console.assert(
      activeRunHiddenByFilters("r0", runs, staleOnly),
      "active run should be hidden when filters exclude it"
    );
    console.assert(
      !activeRunHiddenByFilters("r0", runs, emptyFilters),
      "active run should not be hidden with empty filters"
    );
    console.assert(
      !activeRunHiddenByFilters("missing", runs, staleOnly),
      "unknown active run id should not trigger filter-hidden banner"
    );
    console.assert(
      !activeRunHiddenByFilters("missing", runs, emptyFilters),
      "unknown active run id should not be hidden with empty filters"
    );
  }
  selfTestRunListFilters();

  function selfTestNeteaseSelectProofHint() {
    const hintPanel = document.getElementById("netease-select-proof-hint");
    console.assert(!!hintPanel, "netease-select-proof-hint panel should exist in DOM");
    const bannedHintWords = [
      "seam",
      "resolver",
      "driver_result",
      "verification_outcome",
      "graduation",
      "passed",
      "action transition lineage",
      "primary read surface",
      "core action facts",
    ];

    const run = { run_id: "run_netease_hint" };
    const spans = [{
      span_id: "span_root",
      parent_span_id: null,
      name: "auv.netease.playlist.select",
    }];
    const artifacts = [{ role: PLAYLIST_SELECT_RESULT_ARTIFACT_ROLE, artifact_id: "art_1" }];

    renderNeteaseSelectProofHint(run, spans, artifacts);
    console.assert(!hintPanel.hidden, "hint should show for netease select proof run");
    console.assert(
      hintPanel.textContent.indexOf("NetEase playlist select proof") >= 0,
      "generic hint label should render"
    );
    console.assert(
      hintPanel.textContent.indexOf("packaging lane only") >= 0,
      "secondary disambiguation should render"
    );
    console.assert(
      hintPanel.textContent.toLowerCase().indexOf("selectproof") < 0,
      "hint must not include invoke-specific wording"
    );
    for (const word of bannedHintWords) {
      console.assert(
        hintPanel.textContent.toLowerCase().indexOf(word) < 0,
        "hint must not contain banned seam vocabulary: " + word
      );
    }

    clearNeteaseSelectProofHint();
    console.assert(hintPanel.hidden, "hint should hide after clear");
  }
  selfTestNeteaseSelectProofHint();

  loadRuns();
}
