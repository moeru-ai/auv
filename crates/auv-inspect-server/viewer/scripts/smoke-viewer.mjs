import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { Window } from "happy-dom";

const viewerRoot = resolve(import.meta.dirname, "..");
const html = await readFile(resolve(viewerRoot, "dist/index.html"), "utf8");

const window = new Window({ url: "http://127.0.0.1:8765/" });
const { document } = window;
document.write(html);
document.close();

const unexpectedFetches = [];
const fetchStub = async (input) => {
  const url = String(input);
  if (url === "/runs" || url.endsWith("/runs")) {
    return new Response("[]", {
      status: 200,
      headers: { "content-type": "application/json" }
    });
  }
  if (url.includes("/assets/")) {
    return new Response("<svg></svg>", {
      status: 200,
      headers: { "content-type": "image/svg+xml" }
    });
  }
  unexpectedFetches.push(url);
  return new Response("not found", { status: 404 });
};

class SmokeWebSocket {
  addEventListener() {}
  close() {}
}

function exposeGlobal(name, value) {
  Object.defineProperty(globalThis, name, {
    configurable: true,
    value
  });
}

exposeGlobal("window", window);
exposeGlobal("document", document);
exposeGlobal("location", window.location);
exposeGlobal("navigator", window.navigator);
exposeGlobal("Node", window.Node);
exposeGlobal("Text", window.Text);
exposeGlobal("Element", window.Element);
exposeGlobal("HTMLElement", window.HTMLElement);
exposeGlobal("SVGElement", window.SVGElement);
exposeGlobal("Event", window.Event);
exposeGlobal("CustomEvent", window.CustomEvent);
exposeGlobal("MutationObserver", window.MutationObserver);
exposeGlobal("fetch", fetchStub);
exposeGlobal("WebSocket", SmokeWebSocket);
window.fetch = fetchStub;
window.WebSocket = SmokeWebSocket;

await import(pathToFileURL(resolve(viewerRoot, "dist/assets/viewer.js")).href);

async function waitFor(predicate, label) {
  const deadline = Date.now() + 1000;
  while (Date.now() < deadline) {
    if (predicate()) return;
    await new Promise((resolveAfterDelay) => window.setTimeout(resolveAfterDelay, 10));
  }
  throw new Error(`timed out waiting for ${label}`);
}

await waitFor(() => document.querySelector(".shell") !== null, "viewer shell");

const shell = document.querySelector(".shell");
if (shell === null) {
  throw new Error("viewer shell did not mount");
}

await waitFor(() => {
  const currentRunList = document.getElementById("run-list");
  return currentRunList !== null && currentRunList.textContent.includes("no runs recorded yet.");
}, "empty /runs state");

const runList = document.getElementById("run-list");
if (runList === null || !runList.textContent.includes("no runs recorded yet.")) {
  throw new Error("viewer did not load the empty /runs state");
}

const proofPanel = document.getElementById("netease-select-proof-hint");
if (proofPanel === null) {
  throw new Error("viewer proof panel is missing");
}

if (unexpectedFetches.length > 0) {
  throw new Error(`unexpected fetches: ${unexpectedFetches.join(", ")}`);
}
