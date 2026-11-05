#!/usr/bin/env node
// Mount the unmodified current host renderer against the real process. All
// clipboard replies match launcher/clipboard.rs {ok:true}; no desktop effects.
const fs = require("node:fs"),
  path = require("node:path"),
  os = require("node:os");
const assert = require("node:assert/strict");
const { createRequire } = require("node:module");
const { spawn } = require("node:child_process");
const { createInterface } = require("node:readline");
const { createHash, randomUUID } = require("node:crypto");
const args = process.argv.slice(2);
function arg(name, fallback) {
  const i = args.indexOf(name);
  return i < 0 ? fallback : args[i + 1];
}
assert.ok(arg("--host"), "--host must name a read-only current Tezbar checkout");
const host = path.resolve(arg("--host"));
const binary = path.resolve(arg("--binary", "target-process/debug/tezbar-color-shades"));
const report = arg("--report");
const req = createRequire(path.join(host, "package.json"));
const { JSDOM } = req("jsdom");
const dom = new JSDOM('<!doctype html><html><body><div id="app"></div></body></html>', {
  url: "http://localhost/",
  pretendToBeVisual: true,
});
for (const key of [
  "window",
  "document",
  "HTMLElement",
  "HTMLInputElement",
  "Node",
  "Event",
  "KeyboardEvent",
  "MouseEvent",
  "CustomEvent",
  "MutationObserver",
])
  global[key] = dom.window[key];
Object.defineProperty(global, "navigator", { value: dom.window.navigator, configurable: true });
global.requestAnimationFrame = dom.window.requestAnimationFrame.bind(dom.window);
global.cancelAnimationFrame = dom.window.cancelAnimationFrame.bind(dom.window);
global.IS_REACT_ACT_ENVIRONMENT = true;
dom.window.HTMLElement.prototype.scrollIntoView = function () {};
const React = req("react"),
  { act } = React;
const { createRoot } = req("react-dom/client");
const scratch = fs.mkdtempSync(path.join(os.tmpdir(), "color-shades-renderer-"));
const child = spawn(binary, [], { cwd: scratch, env: { PATH: "/usr/bin:/bin" }, stdio: ["pipe", "pipe", "inherit"] });
const pending = new Map(),
  calls = [],
  results = [],
  failures = [];
let failClipboard = false;
const hello = new Promise((resolve) => {
  createInterface({ input: child.stdout }).on("line", (line) => {
    const frame = JSON.parse(line);
    if (frame.type === "hello") resolve(frame);
    else if (frame.type === "request") {
      calls.push({ method: frame.method, params: frame.params });
      assert.equal(frame.method, "host.clipboard.write");
      send(
        failClipboard
          ? {
              type: "response",
              id: frame.id,
              error: { code: "host_error", message: "Clipboard failure from recording host" },
            }
          : { type: "response", id: frame.id, result: { ok: true } },
      );
    } else if (frame.type === "response") {
      const p = pending.get(frame.id);
      if (!p) return;
      pending.delete(frame.id);
      clearTimeout(p.timer);
      frame.error ? p.reject(new Error(frame.error.message)) : p.resolve(frame.result);
    }
  });
});
function send(frame) {
  child.stdin.write(JSON.stringify(frame) + "\n");
}
function request(method, params) {
  return new Promise((resolve, reject) => {
    const id = "h:" + randomUUID();
    const timer = setTimeout(() => reject(new Error("process timeout: " + method)), 10000);
    pending.set(id, { resolve, reject, timer });
    send({ type: "request", id, method, params });
  });
}
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function settled(fn, ms = 60) {
  await act(async () => {
    if (fn) await fn();
    await pause(ms);
  });
}
function key(node, key, opts = {}) {
  node.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, ...opts }));
}
function button(text) {
  const b = [...document.querySelectorAll("button")].find((b) => b.textContent.includes(text));
  assert.ok(b, "button " + text);
  return b;
}
function input() {
  const node = document.querySelector("input");
  assert.ok(node);
  return node;
}
async function search(text) {
  await settled(() => {
    const i = input();
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(i, text);
    i.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await settled(null, 260);
}
let root;
(async () => {
  const build = await req("esbuild").build({
    stdin: {
      contents: `export { default } from ${JSON.stringify(path.join(host, "src/renderer/ExtensionRuntimeView.tsx"))};`,
      resolveDir: host,
      loader: "tsx",
    },
    bundle: true,
    write: false,
    platform: "node",
    format: "cjs",
    jsx: "automatic",
    external: ["react", "react-dom", "react/*", "react-dom/*"],
    loader: { ".css": "empty" },
    logLevel: "silent",
  });
  const module = { exports: {} };
  new Function("require", "module", "exports", build.outputFiles[0].text)(req, module, module.exports);
  const RuntimeView = module.exports.default;
  class Boundary extends React.Component {
    constructor(p) {
      super(p);
      this.state = { error: null };
    }
    static getDerivedStateFromError(error) {
      return { error };
    }
    componentDidCatch(error) {
      failures.push(error.message);
    }
    render() {
      return this.state.error
        ? React.createElement("div", { role: "alert" }, "RENDER CRASH: " + this.state.error.message)
        : this.props.children;
    }
  }
  await hello;
  const levels = [50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 950];
  const snapshot = {
    name: "Black",
    creationDate: "2024-02-03T04:05:06.007Z",
    colors: Object.fromEntries(
      levels.map((level, i) => [level, i === 5 ? "#000000" : "#aBcD" + String(i).padStart(2, "0")]),
    ),
  };
  const legacy = path.join(scratch, ".tezbar-local-storage.json");
  const legacyBytes = JSON.stringify({ Black: JSON.stringify(snapshot) });
  fs.writeFileSync(legacy, legacyBytes);
  await request("initialize", {
    protocol: 1,
    extensionId: "com.tezbar.native.color-shades",
    platform: "macOS",
    packageDir: scratch,
    dataDir: path.join(scratch, "data"),
    tempDir: path.join(scratch, "tmp"),
    grantedCapabilities: ["clipboard.write"],
    migrationPaths: { jsExtensionsSlugCurrent: legacy },
    reason: "command",
  });
  function annotate(value, sessionId, commandName) {
    return value.mode === "view"
      ? {
          ...value,
          sessionId,
          commandName,
          extensionId: "com.tezbar.native.color-shades",
          refreshMs: null,
          generation: "headless",
        }
      : value;
  }
  window.tezbar = {
    extensionInvokeAction: async (p) => {
      // NativeSessions annotates views; tauri-bridge propagates failures as
      // rejected Errors. Do not manufacture an ok:false success response here.
      const r = await request("session.call", {
        sessionId: p.sessionId,
        operation: "extension:invoke-action",
        payload: p,
      });
      results.push(r);
      return annotate(r, p.sessionId, p.sessionId === "list" ? "view-color-shades" : "generate-color-shades");
    },
    extensionSearchTextChanged: async (p) => {
      const r = await request("session.call", {
        sessionId: p.sessionId,
        operation: "extension:search-text-changed",
        payload: p,
      });
      results.push(r);
      return annotate(r, p.sessionId, p.sessionId === "list" ? "view-color-shades" : "generate-color-shades");
    },
    extensionRefreshSession: async (p) =>
      request("session.call", { sessionId: p.sessionId, operation: "extension:refresh-session", payload: {} }),
    extensionDisposeSession: async () => true,
    onExtensionSessionInvalidated: () => () => {},
    onWindowVisibilityChanged: () => () => {},
    getAssetIconDataUrl: async () => null,
  };
  root = createRoot(document.getElementById("app"));
  const initial = annotate(
    await request("session.open", {
      sessionId: "grid",
      commandName: "generate-color-shades",
      argumentValues: { color: "#82123d" },
      preferences: {},
    }),
    "grid",
    "generate-color-shades",
  );
  await settled(() =>
    root.render(React.createElement(Boundary, null, React.createElement(RuntimeView, { initial, onBack: () => {} }))),
  );
  function healthy() {
    assert.deepEqual(failures, []);
    assert.ok(document.querySelector(".grid"), "palette remains mounted");
  }
  healthy();
  for (const row of initial.root.children)
    assert.ok(document.body.textContent.includes(row.props.subtitle), row.props.subtitle);
  const copyId = initial.root.children[0].props.actionIds[0];
  await settled(() => key(input(), "Enter"));
  healthy();
  assert.equal(calls.at(-1).params.text, initial.root.children[0].props.subtitle);
  assert.ok(
    document.querySelector('[role="status"]')?.textContent.includes("Copied to clipboard"),
    "visible row-copy success",
  );
  for (const title of ["Copy All Colors as Variables", "Copy All Colors as JSON"]) {
    await settled(() => key(input(), "k", { metaKey: true }));
    await settled(() => button(title).click());
    healthy();
    assert.ok(document.body.textContent.includes("Copied to clipboard"), "visible export success");
    assert.equal(results.at(-1).mode, "no-view");
  }
  failClipboard = true;
  await settled(() => key(input(), "Enter"));
  healthy();
  assert.ok(document.body.textContent.includes("Clipboard failure from recording host"), "visible failure");
  failClipboard = false;
  await search("500");
  healthy();
  assert.equal(document.querySelectorAll(".grid > button").length, 1, "local grid filtering");
  assert.equal(results.at(-1).mode, "no-view");
  await search("");
  healthy();
  assert.equal(document.querySelectorAll(".grid > button").length, 11);
  await settled(() => key(input(), "Enter"));
  healthy();
  const stable = await request("session.call", {
    sessionId: "grid",
    operation: "extension:invoke-action",
    payload: { actionId: copyId },
  });
  assert.equal(stable.mode, "no-view", "search and copies preserve issued IDs");
  const refresh = await request("session.call", {
    sessionId: "grid",
    operation: "extension:refresh-session",
    payload: {},
  });
  assert.equal(refresh.mode, "unchanged");
  await settled(() => root.unmount());
  root = null;
  const history = annotate(
    await request("session.open", {
      sessionId: "list",
      commandName: "view-color-shades",
      argumentValues: {},
      preferences: {},
    }),
    "list",
    "view-color-shades",
  );
  root = createRoot(document.getElementById("app"));
  await settled(() =>
    root.render(
      React.createElement(Boundary, null, React.createElement(RuntimeView, { initial: history, onBack: () => {} })),
    ),
  );
  assert.ok(document.body.textContent.includes(initial.title));
  await search("not-a-palette");
  assert.equal(results.at(-1).mode, "no-view");
  assert.deepEqual(failures, []);
  assert.ok(!document.body.textContent.includes(initial.title), "local list filtering");
  await search(initial.title);
  assert.ok(document.body.textContent.includes(initial.title));
  await search("Black");
  const originalStore = fs.readFileSync(path.join(scratch, "data/color-shades/storage.json"), "utf8");
  await settled(() => key(input(), "Enter"));
  healthy();
  for (const level of levels)
    assert.ok(document.body.textContent.includes(snapshot.colors[level]), "snapshot shade " + level);
  assert.equal(
    fs.readFileSync(path.join(scratch, "data/color-shades/storage.json"), "utf8"),
    originalStore,
    "view does not rewrite snapshot",
  );
  await settled(() => key(input(), "k", { metaKey: true }));
  await settled(() => button("Copy All Colors as JSON").click());
  healthy();
  assert.deepEqual(JSON.parse(calls.at(-1).params.text), { black: snapshot.colors });
  assert.equal(fs.readFileSync(legacy, "utf8"), legacyBytes, "source bytes unchanged");
  const sources = [
    "src/renderer/tauri-bridge.ts",
    "src-tauri/src/native_core/extensions/native.rs",
    "src-tauri/src/native_core/extensions/process_host.rs",
    "src-tauri/src/native_core/launcher/clipboard.rs",
    "src/renderer/ExtensionRuntimeView.tsx",
    "src/renderer/src/raycast-api/index.tsx",
    "src/renderer/src/raycast-api/grid-runtime.tsx",
    "src/renderer/src/raycast-api/list-runtime.tsx",
  ];
  const summary = {
    status: "passed",
    binary,
    executableSha256: createHash("sha256").update(fs.readFileSync(binary)).digest("hex"),
    hostSources: Object.fromEntries(
      sources.map((f) => [
        f,
        createHash("sha256")
          .update(fs.readFileSync(path.join(host, f)))
          .digest("hex"),
      ]),
    ),
    checks: [
      "all 11 swatches",
      "normal Enter copy",
      "both export menu actions",
      "visible copy success",
      "visible clipboard error",
      "Grid/List local search and callback results",
      "stable action IDs",
      "unchanged refresh",
      "JS snapshot View Palette + exact colors and JSON export",
    ],
    clipboardCalls: calls.length,
    desktopEffects: false,
  };
  if (report) fs.writeFileSync(report, JSON.stringify(summary, null, 2) + "\n");
  console.log(JSON.stringify(summary, null, 2));
})()
  .catch((e) => {
    console.error(e);
    process.exitCode = 1;
  })
  .finally(async () => {
    if (root) await settled(() => root.unmount());
    try {
      await request("shutdown", {});
    } catch {}
    child.stdin.end();
    child.kill();
    dom.window.close();
    fs.rmSync(scratch, { recursive: true, force: true });
  });
