import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

const serviceWorkerSource = await readFile(
  new URL("../assets/browser-extension/service-worker.js", import.meta.url),
  "utf8"
);
const sharedSource = await readFile(
  new URL("../assets/browser-extension/shared.js", import.meta.url),
  "utf8"
);

function serviceWorkerHarness({ deferOrphanQuery = false, orphanTabs = [] } = {}) {
  const listeners = {};
  const session = new Map();
  let nextStorageGetGate;
  let releaseOrphanQuery;
  const orphanQueryGate = deferOrphanQuery
    ? new Promise((resolve) => {
      releaseOrphanQuery = resolve;
    })
    : Promise.resolve();
  const calls = {
    alarms: [],
    createdWindows: [],
    messageResult: null,
    messages: [],
    orphanTabs,
    removedWindows: []
  };
  let nextWindowId = 41;
  let nextTabId = 73;

  const chrome = {
    alarms: {
      async create(name, options) {
        calls.alarms.push({ name, options });
      },
      async get() {
        return { name: "sunox-bridge-poll" };
      },
      onAlarm: {
        addListener(listener) {
          listeners.alarm = listener;
        }
      }
    },
    offscreen: {
      async createDocument() {
        throw new Error("offscreen document should already exist in this harness");
      }
    },
    runtime: {
      getURL(path) {
        return `chrome-extension://test/${path}`;
      },
      async getContexts() {
        return [{ contextType: "OFFSCREEN_DOCUMENT" }];
      },
      onInstalled: {
        addListener(listener) {
          listeners.installed = listener;
        }
      },
      onMessage: {
        addListener(listener) {
          listeners.message = listener;
        }
      },
      onStartup: {
        addListener(listener) {
          listeners.startup = listener;
        }
      }
    },
    storage: {
      session: {
        async get(key) {
          const gate = nextStorageGetGate;
          nextStorageGetGate = null;
          if (gate) await gate.promise;
          return { [key]: session.get(key) };
        },
        async remove(key) {
          session.delete(key);
        },
        async set(values) {
          for (const [key, value] of Object.entries(values)) session.set(key, value);
        }
      }
    },
    tabs: {
      async get(tabId) {
        const context = session.get("managedContext");
        if (context?.tabId !== tabId) throw new Error("missing tab");
        return { id: tabId, windowId: context.windowId };
      },
      async query({ windowId }) {
        if (windowId === undefined) {
          await orphanQueryGate;
          return calls.orphanTabs;
        }
        const created = calls.createdWindows.find((entry) => entry.id === windowId);
        return created ? [{ id: created.tabId, windowId }] : [];
      },
      async sendMessage(tabId, message) {
        calls.messages.push({ tabId, message });
        return calls.messageResult
          ?? { token: `token-${calls.messages.length}`, error: null };
      }
    },
    windows: {
      async create(options) {
        const created = { id: nextWindowId++, tabId: nextTabId++, options };
        calls.createdWindows.push(created);
        return { id: created.id, type: "popup" };
      },
      async get(windowId) {
        const context = session.get("managedContext");
        if (context?.windowId !== windowId) throw new Error("missing window");
        return { id: windowId, type: "popup" };
      },
      onRemoved: {
        addListener(listener) {
          listeners.windowRemoved = listener;
        }
      },
      async remove(windowId) {
        calls.removedWindows.push(windowId);
      }
    }
  };

  const context = {
    chrome,
    console,
    Promise,
    setTimeout
  };
  context.globalThis = context;
  context.importScripts = (path) => {
    if (path !== "shared.js") throw new Error(`unexpected import: ${path}`);
    vm.runInContext(sharedSource, context);
  };
  vm.createContext(context);
  vm.runInContext(serviceWorkerSource, context);

  return {
    blockNextStorageGet() {
      let release;
      const promise = new Promise((resolve) => {
        release = resolve;
      });
      nextStorageGetGate = { promise };
      return release;
    },
    calls,
    listeners,
    releaseOrphanQuery,
    session
  };
}

function sendChallenge(listener, requestId) {
  return new Promise((resolve, reject) => {
    const keepChannelOpen = listener(
      {
        type: "sunox-managed-challenge",
        challenge: { requestId, provider: "hcaptcha" }
      },
      {},
      resolve
    );
    if (keepChannelOpen !== true) reject(new Error("message channel was not kept open"));
  });
}

test("managed challenge context uses a non-focused screen-outside popup and is reused", async () => {
  const { calls, listeners, session } = serviceWorkerHarness();

  const first = await sendChallenge(listeners.message, "request-1");
  assert.equal(first.token, "token-1");
  assert.equal(calls.createdWindows.length, 1);
  assert.deepEqual(
    {
      type: calls.createdWindows[0].options.type,
      focused: calls.createdWindows[0].options.focused,
      left: calls.createdWindows[0].options.left,
      top: calls.createdWindows[0].options.top
    },
    { type: "popup", focused: false, left: -32_000, top: -32_000 }
  );
  assert.ok(session.get("managedContext"));

  await new Promise((resolve) => setTimeout(resolve, 0));
  const second = await sendChallenge(listeners.message, "request-2");
  assert.equal(second.token, "token-2");
  assert.equal(calls.createdWindows.length, 1);
  assert.equal(calls.messages.length, 2);
  assert.equal(calls.removedWindows.length, 0);
  assert.ok(calls.alarms.some(({ name }) => name === "sunox-bridge-managed-context-idle"));
});

test("provider errors reuse the context twice, then rebuild after the third failure", async () => {
  const { calls, listeners, session } = serviceWorkerHarness();
  calls.messageResult = { token: null, error: "provider failed" };

  for (let attempt = 1; attempt <= 2; attempt += 1) {
    const result = await sendChallenge(listeners.message, `request-error-${attempt}`);
    assert.equal(result.error, "provider failed");
    assert.equal(calls.removedWindows.length, 0);
    assert.equal(session.get("managedContext").failureCount, attempt);
    await new Promise((resolve) => setTimeout(resolve, 0));
  }

  const third = await sendChallenge(listeners.message, "request-error-3");
  assert.equal(third.error, "provider failed");
  assert.equal(calls.removedWindows.length, 1);
  assert.equal(session.has("managedContext"), false);
});

test("idle cleanup cannot close a context while a challenge is running", async () => {
  const { calls, listeners, session } = serviceWorkerHarness();
  let finishChallenge;
  calls.messageResult = new Promise((resolve) => {
    finishChallenge = resolve;
  });

  const challenge = sendChallenge(listeners.message, "request-slow");
  await new Promise((resolve) => setTimeout(resolve, 0));
  const context = session.get("managedContext");
  session.set("managedContext", {
    ...context,
    lastUsedAt: Date.now() - 30 * 60 * 1000
  });

  listeners.alarm({ name: "sunox-bridge-managed-context-idle" });
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(calls.removedWindows.length, 0);
  assert.ok(calls.alarms.some(({ name }) => name === "sunox-bridge-managed-context-idle"));

  finishChallenge({ token: "token-slow", error: null });
  assert.equal((await challenge).token, "token-slow");
});

test("a challenge queued during idle inspection keeps and reuses the context", async () => {
  const {
    blockNextStorageGet,
    calls,
    listeners,
    session
  } = serviceWorkerHarness();

  assert.equal((await sendChallenge(listeners.message, "request-warmup")).token, "token-1");
  await new Promise((resolve) => setTimeout(resolve, 0));
  const context = session.get("managedContext");
  session.set("managedContext", {
    ...context,
    lastUsedAt: Date.now() - 30 * 60 * 1000
  });

  const releaseStorageGet = blockNextStorageGet();
  listeners.alarm({ name: "sunox-bridge-managed-context-idle" });
  await new Promise((resolve) => setTimeout(resolve, 0));
  const challenge = sendChallenge(listeners.message, "request-during-idle-read");
  await new Promise((resolve) => setTimeout(resolve, 0));

  releaseStorageGet();
  assert.equal((await challenge).token, "token-2");
  assert.equal(calls.createdWindows.length, 1);
  assert.equal(calls.removedWindows.length, 0);
});

test("a cold-start challenge waits for orphan cleanup before creating its context", async () => {
  const {
    calls,
    listeners,
    releaseOrphanQuery
  } = serviceWorkerHarness({ deferOrphanQuery: true });

  const challenge = sendChallenge(listeners.message, "request-cold-start");
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(calls.createdWindows.length, 0);

  releaseOrphanQuery();
  assert.equal((await challenge).token, "token-1");
  assert.equal(calls.createdWindows.length, 1);
  assert.equal(calls.removedWindows.length, 0);
});

test("startup removes a restored managed popup that lost its session record", async () => {
  const { calls } = serviceWorkerHarness({
    orphanTabs: [{
      id: 99,
      windowId: 88,
      url: "https://suno.com/create#sunox-browser-bridge"
    }]
  });
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.deepEqual(calls.removedWindows, [88]);
});
