<template>
<div class="shell">
  <div class="top-bar">
    <!-- Logo mark — served from /assets/logo-mark.svg (design system) -->
    <img class="logo" src="/assets/logo-mark.svg" alt="">
    <div class="brand-name">auv</div>
    <div class="crumb">/ inspect viewer</div>
    <div class="spacer"></div>
    <div id="conn" class="conn-pill bad">
      <span class="dot"></span>
      <span id="conn-label">connecting</span>
    </div>
    <div class="conn-endpoint" id="conn-endpoint">—</div>
  </div>

  <div class="main-split">
    <aside class="sidebar">
      <div class="pane-header">
        <span class="label">Runs · /runs</span>
        <span class="spacer"></span>
        <span class="right" id="run-count">—</span>
      </div>
      <div class="filter-chips" id="run-list-filters"></div>
      <div class="run-list" id="run-list">
        <div class="run-row empty">loading runs…</div>
      </div>
    </aside>
    <main class="main">
      <div class="pane-header">
        <span class="label" id="main-label">Run · —</span>
        <span class="spacer"></span>
        <span class="right" id="main-crumb"></span>
        <span id="main-status"></span>
      </div>
      <div id="netease-select-proof-hint" hidden></div>
      <div id="view-parser-proof" hidden></div>
      <div id="run-list-filter-banner" class="run-list-filter-banner" hidden></div>
      <div class="placeholder" id="main-body">
        <div>select a run from the sidebar.</div>
        <div>this viewer reads <code>/runs</code> and renders the AUV trace contract.</div>
      </div>
      <div class="events-rail" id="events-rail" hidden>
        <div class="span-detail empty" id="span-detail">
          <img class="sparkle" src="/assets/sparkle.svg" alt="">
          <span>Select a span to inspect its attributes.</span>
        </div>
        <div class="pane-header">
          <span class="label">Events · events.jsonl</span>
          <span class="spacer"></span>
          <span class="right" id="event-count">— · tail</span>
        </div>
        <div class="event-list" id="event-list"></div>
      </div>
    </main>
    <aside class="artifact-panel" id="artifact-panel" hidden>
      <div class="pane-header">
        <span class="label">Artifacts · /artifacts</span>
        <span class="spacer"></span>
        <span class="right" id="artifact-count">—</span>
      </div>
      <div class="artifact-list" id="artifact-list"></div>
      <div class="artifact-preview empty" id="artifact-preview"></div>
    </aside>
  </div>
</div>

</template>

<script setup lang="ts">
import { onMounted } from "vue";
import { mountInspectViewer } from "./viewer";

onMounted(() => {
  mountInspectViewer(document);
});
</script>
