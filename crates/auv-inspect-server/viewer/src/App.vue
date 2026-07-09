<template>
  <main id="auv-inspect-viewer">
    <div id="legacy-viewer-root"></div>
  </main>
</template>

<script setup lang="ts">
import { onMounted } from "vue";
import { mountLegacyViewer } from "./legacy/viewer";

onMounted(() => {
  const root = document.getElementById("legacy-viewer-root");
  if (root === null) {
    throw new Error("legacy viewer root is missing");
  }
  mountLegacyViewer(root);
});
</script>
