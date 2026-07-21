<template>
  <div class="shell">
    <header class="top-bar">
      <img class="logo" src="/assets/logo-mark.svg" alt="">
      <div class="brand-name">auv</div>
      <div class="crumb">/ inspect</div>
      <div class="run-picker">
        <input id="run-id-input" type="text" aria-label="Run ID" placeholder="run id">
        <button id="load-run" class="chip" type="button">Load</button>
      </div>
      <div class="spacer"></div>
      <div id="conn" class="conn-pill bad">
        <span class="dot"></span>
        <span id="conn-label">offline</span>
      </div>
      <div id="conn-endpoint" class="conn-endpoint">no run selected</div>
    </header>

    <div class="main-split">
      <aside class="sidebar">
        <div class="pane-header">
          <span class="label">Run</span>
          <span class="spacer"></span>
          <span id="run-count" class="right">0</span>
        </div>
        <div id="run-list" class="run-list">
          <div class="run-row empty">no runs recorded yet.</div>
        </div>
      </aside>

      <main class="main">
        <div class="pane-header">
          <span id="main-label" class="label">Run / -</span>
          <span class="spacer"></span>
          <span id="main-crumb" class="right"></span>
        </div>
        <div id="span-tree" class="span-tree">
          <div class="run-row empty">no spans</div>
        </div>
        <section class="events-rail">
          <div class="pane-header">
            <span class="label">Events</span>
            <span class="spacer"></span>
            <span id="event-count" class="right">0</span>
          </div>
          <div id="event-list" class="event-list"></div>
        </section>
      </main>

      <aside class="artifact-panel">
        <div class="pane-header">
          <span class="label">Artifacts</span>
          <span class="spacer"></span>
          <span id="artifact-count" class="right">0</span>
        </div>
        <div id="artifact-list" class="artifact-list"></div>
      </aside>
    </div>

    <div id="netease-select-proof-hint" hidden></div>
  </div>
</template>

<script setup lang="ts">
import { onMounted } from "vue";
import { mountInspectViewer } from "./viewer";

/**
 * Mounts the Inspect DOM adapter after Vue creates the shell.
 *
 * Triggering workflow:
 *
 * {@link onMounted}
 *   -> `vue.mounted`
 *     -> `inspect.viewer.mount`
 *       -> {@link onViewerMounted}
 *
 * Upstream:
 * - {@link onMounted}
 *
 * Downstream:
 * - {@link mountInspectViewer}
 */
function onViewerMounted(): void {
  mountInspectViewer(document);
}

onMounted(onViewerMounted);
</script>

<style scoped>
.run-picker {
  display: flex;
  align-items: center;
  gap: 6px;
  width: min(440px, 42vw);
}

.run-picker input {
  min-width: 0;
  flex: 1;
  height: 24px;
  box-sizing: border-box;
  border: 1px solid var(--shell-line);
  border-radius: 2px;
  background: var(--shell-2);
  color: var(--fg);
  padding: 0 8px;
  font-family: var(--font-mono), monospace;
  letter-spacing: 0;
}

.artifact-panel {
  display: flex;
}

@media (max-width: 780px) {
  .top-bar .crumb,
  .conn-endpoint,
  .sidebar {
    display: none;
  }

  .run-picker {
    width: auto;
    flex: 1;
  }

  .artifact-panel {
    width: 34%;
  }
}
</style>
