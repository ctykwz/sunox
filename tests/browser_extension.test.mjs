import assert from "node:assert/strict";
import { createHmac, webcrypto } from "node:crypto";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

const serviceWorkerSource = await readFile(
  new URL("../assets/browser-extension/service-worker.js", import.meta.url),
  "utf8"
);
const offscreenSource = await readFile(
  new URL("../assets/browser-extension/offscreen.js", import.meta.url),
  "utf8"
);
const bridgeSource = await readFile(
  new URL("../assets/browser-extension/bridge.js", import.meta.url),
  "utf8"
);
const pageSource = await readFile(
  new URL("../assets/browser-extension/page.js", import.meta.url),
  "utf8"
);
const loopbackTransportSource = await readFile(
  new URL("../assets/browser-extension/transport-loopback.js", import.meta.url),
  "utf8"
);
const isolatedTurnstileSource = await readFile(
  new URL("../src/captcha/cdp/turnstile_solver.js", import.meta.url),
  "utf8"
);
const manifest = JSON.parse(await readFile(
  new URL("../assets/browser-extension/manifest.json", import.meta.url),
  "utf8"
));

function flushAsync() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

const managedNonce = "12345678-1234-4123-8123-123456789abc";
const managedHash = `#sunox-browser-bridge=${managedNonce}`;

async function deliverManagedReady(port) {
  port.deliver({
    type: "sunox-managed-window-ready-v1",
    nonce: managedNonce
  });
  await flushAsync();
  await flushAsync();
}

function numericConstant(source, name) {
  const match = source.match(new RegExp(`const ${name} = ([0-9_]+);`));
  assert.ok(match, `missing numeric constant ${name}`);
  return Number(match[1].replaceAll("_", ""));
}

const defaultDisplayInfo = [{
  activeState: "active",
  bounds: {
    height: 1117,
    left: 0,
    top: 0,
    width: 1728
  },
  isEnabled: true
}];

function bridgeProof(secret, label, fields) {
  const chunks = [Buffer.from(label), Buffer.from([0])];
  for (const field of fields) {
    const value = Buffer.from(String(field));
    const length = Buffer.alloc(4);
    length.writeUInt32BE(value.length);
    chunks.push(length, value);
  }
  return createHmac("sha256", secret)
    .update(Buffer.concat(chunks))
    .digest("hex");
}

function loadLoopbackTransport(fetchImpl, {
  portCount = 1,
  requestTimeoutMs = 350
} = {}) {
  const context = {
    AbortController,
    atob,
    btoa,
    clearTimeout,
    crypto: webcrypto,
    DataView,
    fetch: fetchImpl,
    Promise,
    setTimeout,
    SUNOX_BRIDGE_CONFIG: {
      loopback: {
        portCount,
        portStart: 29_764,
        protocolVersion: 3,
        runtimeBuild: "0.3.15",
        sharedSecret: "a".repeat(64)
      }
    },
    TextDecoder,
    TextEncoder,
    Uint8Array,
    URL
  };
  context.globalThis = context;
  vm.createContext(context);
  vm.runInContext(
    loopbackTransportSource.replace(
      "const requestTimeoutMs = 350;",
      `const requestTimeoutMs = ${requestTimeoutMs};`
    ),
    context
  );
  return context.SUNOX_BRIDGE_TRANSPORTS.loopback;
}

function popupServiceWorkerHarness({
  actualWindowBounds = {
    focused: false,
    height: 900,
    left: -2304,
    state: "minimized",
    top: -1924,
    width: 1280
  },
  connectDuringRecoveryReload = false,
  connectBeforeCreateResolves = false,
  createError = null,
  displayInfo = defaultDisplayInfo,
  displayInfoError = null,
  emitBlankUpdateDuringBoundsCheck = false,
  emitInitializationEvents = false,
  existingWindowUrl = null,
  initialStoredState = undefined,
  existingSessionRules = [],
  storageSetError = null,
  windowGetError = null,
  windowGetErrorSequence = [],
  windowGetSequence = [],
  windowUpdateApplies = true,
  windowUpdateError = null
} = {}) {
  const managedUrl =
    "https://suno.com/create#sunox-browser-bridge=12345678-1234-4123-8123-123456789abc";
  const listeners = {};
  const calls = {
    consoleErrors: [],
    createdOffscreen: [],
    createdWindows: [],
    dynamicRules: [],
    notifications: [],
    removedTabs: [],
    removedWindows: [],
    reloadedTabs: [],
    sessionRules: [],
    storageRemoves: [],
    storageSets: [],
    updatedTabs: [],
    updatedWindows: []
  };
  const hardTimers = [];
  const safetyIntervals = [];
  const storageValues = initialStoredState === undefined
    ? {}
    : { sunoxManagedWindowV1: structuredClone(initialStoredState) };
  let currentDisplayInfo = structuredClone(displayInfo);
  let currentWindowBounds = structuredClone(actualWindowBounds);
  let currentWindowGetError = windowGetError;
  const pendingWindowGetErrors = structuredClone(windowGetErrorSequence);
  const pendingWindowGetStates = structuredClone(windowGetSequence);
  let port;
  let recoveryReloadPort;
  let tabUrl = existingWindowUrl || "about:blank";
  const makePort = (sender, name = "sunox-managed-window-v1") => {
    let disconnectListener;
    let messageListener;
    const messages = [];
    return {
      disconnected: false,
      messages,
      name,
      onDisconnect: {
        addListener(listener) {
          disconnectListener = listener;
        }
      },
      onMessage: {
        addListener(listener) {
          messageListener = listener;
        }
      },
      postMessage(message) {
        messages.push(message);
      },
      disconnect() {
        this.disconnected = true;
        disconnectListener?.();
      },
      sender,
      deliver(message) {
        messageListener?.(message);
      }
    };
  };
  const chrome = {
    alarms: {
      async create() {},
      async get() {
        return null;
      },
      onAlarm: {
        addListener(listener) {
          listeners.alarm = listener;
        }
      }
    },
    declarativeNetRequest: {
      async getSessionRules() {
        return structuredClone(existingSessionRules);
      },
      async updateDynamicRules(options) {
        calls.dynamicRules.push(options);
      },
      async updateSessionRules(options) {
        calls.sessionRules.push(options);
      }
    },
    offscreen: {
      async closeDocument() {},
      async createDocument(options) {
        calls.createdOffscreen.push(options);
      }
    },
    runtime: {
      id: "abcdefghijklmnopabcdefghijklmnop",
      async getContexts() {
        return [];
      },
      getURL(path) {
        return `chrome-extension://abcdefghijklmnopabcdefghijklmnop/${path}`;
      },
      onConnect: {
        addListener(listener) {
          listeners.connect = listener;
        }
      },
      onInstalled: {
        addListener(listener) {
          listeners.installed = listener;
        }
      },
      onMessage: {
        addListener(listener) {
          listeners.runtimeMessage = listener;
        }
      },
      onStartup: {
        addListener(listener) {
          listeners.startup = listener;
        }
      },
      async sendMessage(message) {
        if (message.type === "sunox-offscreen-start-v1") {
          return { accepted: true };
        }
        if (message.type === "sunox-offscreen-ping-v1") {
          return {
            busy: false,
            busySince: null,
            pollWorkerAgeMs: 1,
            pollWorkerHealthy: true,
            type: "sunox-offscreen-pong-v1"
          };
        }
        calls.notifications.push(message);
        return { accepted: true };
      }
    },
    storage: {
      session: {
        async get(key) {
          return {
            [key]: storageValues[key] === undefined
              ? undefined
              : structuredClone(storageValues[key])
          };
        },
        async remove(key) {
          calls.storageRemoves.push(key);
          delete storageValues[key];
        },
        async set(values) {
          if (storageSetError) throw new Error(storageSetError);
          calls.storageSets.push(structuredClone(values));
          Object.assign(storageValues, structuredClone(values));
        }
      }
    },
    system: {
      display: {
        async getInfo() {
          if (displayInfoError) throw new Error(displayInfoError);
          return structuredClone(currentDisplayInfo);
        },
        onDisplayChanged: {
          addListener(listener) {
            listeners.displayChanged = listener;
          }
        }
      }
    },
    tabs: {
      TAB_ID_NONE: -1,
      onUpdated: {
        addListener(listener) {
          listeners.tabUpdated = listener;
        }
      },
      async query(options) {
        if (Number.isInteger(options.windowId)) {
          return [{ id: 41, url: tabUrl, windowId: options.windowId }];
        }
        if (existingWindowUrl) {
          return [{ id: 41, url: tabUrl, windowId: 7 }];
        }
        return [];
      },
      async get(tabId) {
        return { id: tabId, url: tabUrl, windowId: 7 };
      },
      async reload(tabId) {
        calls.reloadedTabs.push(tabId);
        if (connectDuringRecoveryReload) {
          recoveryReloadPort = makePort({
            documentId: "document-before-restart",
            frameId: 0,
            id: chrome.runtime.id,
            origin: "https://suno.com",
            tab: {
              id: tabId,
              windowId: 7
            },
            url: tabUrl
          });
          listeners.connect(recoveryReloadPort);
        }
        listeners.tabUpdated?.(tabId, { status: "loading" }, {
          id: tabId,
          url: tabUrl,
          windowId: 7
        });
        if (recoveryReloadPort && !recoveryReloadPort.disconnected) {
          recoveryReloadPort.disconnect();
        }
        listeners.tabUpdated?.(tabId, { status: "complete" }, {
          id: tabId,
          url: tabUrl,
          windowId: 7
        });
      },
      async remove(tabId) {
        calls.removedTabs.push(tabId);
      },
      async update(tabId, options) {
        calls.updatedTabs.push({ options, tabId });
        tabUrl = options.url;
        return { id: tabId, url: tabUrl, windowId: 7 };
      }
    },
    windows: {
      onBoundsChanged: {
        addListener(listener) {
          listeners.windowBoundsChanged = listener;
        }
      },
      onFocusChanged: {
        addListener(listener) {
          listeners.windowFocusChanged = listener;
        }
      },
      onRemoved: {
        addListener(listener) {
          listeners.windowRemoved = listener;
        }
      },
      async create(options) {
        calls.createdWindows.push(options);
        tabUrl = options.url;
        const sender = {
          documentId: "document-current",
          frameId: 0,
          id: chrome.runtime.id,
          origin: "https://suno.com",
          tab: {
            id: 41,
            windowId: 7
          },
          url: managedUrl
        };
        port = makePort(sender);
        if (connectBeforeCreateResolves) listeners.connect(port);
        if (createError) throw new Error(createError);
        return {
          ...currentWindowBounds,
          id: 7,
          type: "popup",
          tabs: [{
            id: 41,
            url: tabUrl,
            windowId: 7
          }]
        };
      },
      async get(windowId) {
        if (pendingWindowGetErrors.length > 0) {
          const sequencedError = pendingWindowGetErrors.shift();
          if (sequencedError) throw new Error(sequencedError);
        }
        if (currentWindowGetError) throw new Error(currentWindowGetError);
        if (pendingWindowGetStates.length > 0) {
          currentWindowBounds = pendingWindowGetStates.shift();
        }
        if (emitBlankUpdateDuringBoundsCheck) {
          listeners.tabUpdated(41, {}, {
            id: 41,
            url: "about:blank",
            windowId
          });
        }
        return {
          ...currentWindowBounds,
          id: windowId,
          type: "popup",
          tabs: [{
            id: 41,
            url: tabUrl,
            windowId
          }]
        };
      },
      async remove(windowId) {
        calls.removedWindows.push(windowId);
      },
      async update(windowId, options) {
        calls.updatedWindows.push({ options, windowId });
        if (windowUpdateError) throw new Error(windowUpdateError);
        if (windowUpdateApplies) {
          currentWindowBounds = {
            ...currentWindowBounds,
            ...options
          };
        }
        if (emitInitializationEvents) {
          listeners.windowBoundsChanged?.({
            ...currentWindowBounds,
            id: windowId,
            type: "popup"
          });
          listeners.windowFocusChanged?.(windowId);
        }
        return {
          ...currentWindowBounds,
          id: windowId,
          type: "popup"
        };
      }
    }
  };
  const detachedSetTimeout = (callback, delay) => {
    if (delay === 60_000) hardTimers.push(callback);
    const timer = setTimeout(() => {}, 60_000);
    timer.unref?.();
    return timer;
  };
  const context = {
    AbortController,
    chrome,
    clearInterval(timer) {
      if (timer) timer.cleared = true;
    },
    clearTimeout,
    console: {
      error(...values) {
        calls.consoleErrors.push(values.map((value) => (
          value instanceof Error ? value.message : String(value)
        )));
      }
    },
    crypto: {
      randomUUID() {
        return "12345678-1234-4123-8123-123456789abc";
      }
    },
    Date,
    fetch: async () => ({
      headers: new Headers({
        "content-security-policy":
          "default-src 'self'; frame-ancestors 'none'; font-src 'self'"
      }),
      ok: true,
      status: 200,
      url: "https://suno.com/create"
    }),
    Headers,
    Promise,
    setInterval(callback, delay) {
      const timer = { callback, cleared: false, delay };
      safetyIntervals.push(timer);
      return timer;
    },
    setTimeout: detachedSetTimeout
  };
  context.globalThis = context;
  context.URL = URL;
  vm.createContext(context);
  vm.runInContext(serviceWorkerSource, context);

  const dispatchFromOffscreen = (message) => new Promise((resolve) => {
    const sender = {
      id: chrome.runtime.id,
      url: chrome.runtime.getURL("offscreen.html")
    };
    const keepChannel = listeners.runtimeMessage(message, sender, resolve);
    if (keepChannel !== true) queueMicrotask(() => resolve(undefined));
  });

  return {
    calls,
    connect(sender, name) {
      const candidate = makePort(sender, name);
      listeners.connect(candidate);
      return candidate;
    },
    dispatchFromOffscreen,
    hardTimers,
    listeners,
    managedUrl,
    navigateTab(url) {
      tabUrl = url;
      listeners.tabUpdated?.(41, { url }, {
        id: 41,
        url,
        windowId: 7
      });
    },
    setTabUrl(url) {
      tabUrl = url;
    },
    port() {
      return port;
    },
    recoveryReloadPort() {
      return recoveryReloadPort;
    },
    runSafetyWatchdog() {
      const timer = safetyIntervals.find((candidate) => !candidate.cleared);
      assert.ok(timer, "managed popup safety watchdog must be armed");
      assert.equal(timer.delay, 250);
      return timer.callback();
    },
    storedState() {
      const value = storageValues.sunoxManagedWindowV1;
      return value === undefined ? undefined : structuredClone(value);
    },
    setWindowGetError(value) {
      currentWindowGetError = value;
    },
    updateDisplayInfo(value, { notify = true } = {}) {
      currentDisplayInfo = structuredClone(value);
      if (notify) listeners.displayChanged?.();
    },
    updateWindowBounds(value, { notify = true } = {}) {
      currentWindowBounds = structuredClone(value);
      if (notify) {
        listeners.windowBoundsChanged?.({
          ...currentWindowBounds,
          id: 7,
          type: "popup"
        });
      }
    }
  };
}

test("bootstrap installs exact offscreen-frame rules and creates no browser window", async () => {
  const { calls } = popupServiceWorkerHarness();
  await flushAsync();
  await flushAsync();

  assert.deepEqual(JSON.parse(JSON.stringify(calls.dynamicRules)), [{
    removeRuleIds: [29_764, 29_765]
  }]);
  assert.equal(calls.sessionRules.length, 1);
  const update = JSON.parse(JSON.stringify(calls.sessionRules[0]));
  assert.deepEqual(update.removeRuleIds, [29_764, 29_765]);
  assert.equal(update.addRules.length, 2);
  assert.deepEqual(
    update.addRules.map((rule) => ({
      id: rule.id,
      regexFilter: rule.condition.regexFilter,
      initiatorDomains: rule.condition.initiatorDomains,
      resourceTypes: rule.condition.resourceTypes,
      tabIds: rule.condition.tabIds,
      headers: rule.action.responseHeaders
    })),
    [
      {
        id: 29_764,
        regexFilter: "^https://suno\\.com/create/?(?:[?#].*)?$",
        initiatorDomains: [
          "abcdefghijklmnopabcdefghijklmnop",
          "auth.suno.com",
          "suno.com"
        ],
        resourceTypes: ["sub_frame"],
        tabIds: [-1],
        headers: [
          {
            header: "content-security-policy",
            operation: "set",
            value: "default-src 'self'; font-src 'self';"
          },
          { header: "x-frame-options", operation: "remove" }
        ]
      },
      {
        id: 29_765,
        regexFilter:
          "^https://auth\\.suno\\.com/v1/client/handshake(?:\\?.*)?$",
        initiatorDomains: [
          "abcdefghijklmnopabcdefghijklmnop",
          "suno.com"
        ],
        resourceTypes: ["sub_frame"],
        tabIds: [-1],
        headers: [
          { header: "x-frame-options", operation: "remove" }
        ]
      }
    ]
  );
  assert.deepEqual(JSON.parse(JSON.stringify(calls.createdOffscreen)), [{
    url: "offscreen.html",
    reasons: ["IFRAME_SCRIPTING", "WORKERS"],
    justification:
      "Poll the local Sunox listener and run an invisible Suno challenge frame without creating a browser tab or window."
  }]);
  assert.deepEqual(calls.createdWindows, []);
});

test("bootstrap repairs a reusable Suno frame rule when the Clerk rule is missing", async () => {
  const harness = popupServiceWorkerHarness({
    existingSessionRules: [{
      id: 29_764,
      priority: 1,
      action: {
        type: "modifyHeaders",
        responseHeaders: [
          { header: "x-frame-options", operation: "remove" }
        ]
      },
      condition: {
        regexFilter: "^https://suno\\.com/create/?(?:[?#].*)?$",
        initiatorDomains: [
          "abcdefghijklmnopabcdefghijklmnop",
          "auth.suno.com",
          "suno.com"
        ],
        resourceTypes: ["sub_frame"],
        tabIds: [-1]
      }
    }]
  });
  await flushAsync();
  await flushAsync();

  assert.equal(harness.calls.sessionRules.length, 1);
  assert.deepEqual(
    JSON.parse(JSON.stringify(
      harness.calls.sessionRules[0].addRules.map((rule) => rule.id)
    )),
    [29_764, 29_765]
  );
});

test("service worker relays one nonce-bound offscreen frame challenge without creating a window", async () => {
  const harness = popupServiceWorkerHarness();
  await flushAsync();
  await flushAsync();

  const port = harness.connect({
    documentId: "offscreen-frame-document",
    frameId: 1,
    id: "abcdefghijklmnopabcdefghijklmnop",
    origin: "https://suno.com",
    url: harness.managedUrl
  }, "sunox-managed-frame-v2");
  await flushAsync();

  assert.deepEqual(
    JSON.parse(JSON.stringify(harness.calls.notifications.at(-1))),
    {
      type: "sunox-managed-frame-ready-v2",
      nonce: managedNonce
    }
  );
  const response = await harness.dispatchFromOffscreen({
    type: "sunox-managed-frame-execute-v2",
    nonce: managedNonce,
    provider: "turnstile",
    requestId: "request-hidden-frame"
  });
  assert.deepEqual(JSON.parse(JSON.stringify(response)), { accepted: true });
  assert.deepEqual(JSON.parse(JSON.stringify(port.messages)), [{
    type: "sunox-managed-frame-execute-v2",
    provider: "turnstile",
    requestId: "request-hidden-frame"
  }]);

  port.deliver({
    type: "sunox-managed-frame-result-v2",
    requestId: "request-hidden-frame",
    token: "hidden-frame-token"
  });
  await flushAsync();

  assert.deepEqual(
    JSON.parse(JSON.stringify(harness.calls.notifications.at(-1))),
    {
      type: "sunox-managed-frame-result-v2",
      nonce: managedNonce,
      requestId: "request-hidden-frame",
      token: "hidden-frame-token",
      error: null
    }
  );
  assert.deepEqual(harness.calls.createdWindows, []);
});

test("service worker accepts an offscreen frame when Chrome omits documentId", async () => {
  const harness = popupServiceWorkerHarness();
  await flushAsync();
  await flushAsync();

  const port = harness.connect({
    frameId: 1,
    id: "abcdefghijklmnopabcdefghijklmnop",
    origin: "https://suno.com",
    url: harness.managedUrl
  }, "sunox-managed-frame-v2");
  await flushAsync();

  assert.equal(port.disconnected, false);
  assert.deepEqual(
    JSON.parse(JSON.stringify(harness.calls.notifications.at(-1))),
    {
      type: "sunox-managed-frame-ready-v2",
      nonce: managedNonce
    }
  );
});

test("service worker reports only a sanitized reason for a rejected managed frame", async () => {
  const harness = popupServiceWorkerHarness();
  await flushAsync();
  await flushAsync();

  const port = harness.connect({
    documentId: "top-level-document",
    frameId: 0,
    id: "abcdefghijklmnopabcdefghijklmnop",
    origin: "https://suno.com",
    url: harness.managedUrl
  }, "sunox-managed-frame-v2");
  await flushAsync();

  assert.equal(port.disconnected, true);
  assert.deepEqual(JSON.parse(JSON.stringify(port.messages)), [{
    type: "sunox-managed-frame-rejected-v2"
  }]);
  assert.deepEqual(
    JSON.parse(JSON.stringify(harness.calls.notifications.at(-1))),
    {
      type: "sunox-managed-frame-diagnostic-v1",
      nonce: managedNonce,
      reason: "top_level_frame"
    }
  );
  assert.equal(
    JSON.stringify(harness.calls.notifications).includes("__clerk_handshake"),
    false
  );
});

test("a port arriving before bootstrap recovery is retriable, not rejected", async () => {
  const harness = popupServiceWorkerHarness();
  const port = harness.connect({
    documentId: "document-before-bootstrap",
    frameId: 1,
    id: "abcdefghijklmnopabcdefghijklmnop",
    origin: "https://suno.com",
    url: harness.managedUrl
  }, "sunox-managed-frame-v2");

  assert.equal(port.disconnected, true);
  assert.deepEqual(
    port.messages,
    [],
    "bootstrap recovery must not permanently reject a retriable content port"
  );
  await flushAsync();
  await flushAsync();
  await flushAsync();
  assert.deepEqual(harness.calls.notifications, []);
});

test("content bridge connects only after a clean canonical frame is stable", () => {
  const nonce = "00000000-0000-4000-8000-000000000001";
  const canonicalUrl =
    `https://suno.com/create#sunox-browser-bridge=${nonce}`;
  const location = {
    href: canonicalUrl.replace(
      "/create#",
      "/create?__clerk_handshake=opaque-return#"
    ),
    origin: "https://suno.com"
  };
  const posted = [];
  const timers = [];
  let connections = 0;
  let now = 0;
  const port = {
    onDisconnect: {
      addListener() {}
    },
    onMessage: {
      addListener() {}
    },
    disconnect() {},
    postMessage(message) {
      posted.push(message);
    }
  };
  const context = {
    chrome: {
      runtime: {
        connect() {
          connections += 1;
          return port;
        }
      }
    },
    clearInterval() {},
    clearTimeout() {},
    Date: {
      now() {
        return now;
      }
    },
    document: {
      readyState: "complete"
    },
    location,
    Promise,
    setInterval() {
      throw new Error("execution keepalive must not start before readiness");
    },
    setTimeout(callback, delay) {
      const timer = { callback, delay };
      timers.push(timer);
      return timer;
    },
    URL
  };
  context.window = context;
  context.top = {};
  context.parent = context.top;
  context.addEventListener = () => {};
  context.removeEventListener = () => {};
  context.postMessage = () => {};
  context.globalThis = context;
  vm.createContext(context);
  vm.runInContext(bridgeSource, context);

  timers.shift().callback();
  assert.equal(connections, 0);
  assert.deepEqual(posted, []);
  location.href = canonicalUrl;
  now = 100;
  timers.shift().callback();
  assert.equal(connections, 0);
  assert.deepEqual(posted, []);
  now = 599;
  timers.shift().callback();
  assert.equal(connections, 0);
  assert.deepEqual(posted, []);
  now = 600;
  timers.shift().callback();

  assert.equal(connections, 1);
  assert.deepEqual(posted, []);
});

test("managed frame messages keep the MV3 service worker alive during a challenge", async () => {
  let disconnectedListener;
  let interval;
  let portMessageListener;
  let realmWindow;
  let windowMessageListener;
  const posted = [];
  const timers = [];
  let now = 0;
  const port = {
    onDisconnect: {
      addListener(listener) {
        disconnectedListener = listener;
      }
    },
    onMessage: {
      addListener(listener) {
        portMessageListener = listener;
      }
    },
    disconnect() {
      disconnectedListener?.();
    },
    postMessage(message) {
      posted.push(message);
    }
  };
  const context = {
    chrome: {
      runtime: {
        connect() {
          return port;
        }
      }
    },
    clearInterval(timer) {
      timer.cleared = true;
    },
    clearTimeout() {},
    document: {
      readyState: "complete"
    },
    location: {
      href: "https://suno.com/create#sunox-browser-bridge=00000000-0000-4000-8000-000000000001",
      origin: "https://suno.com"
    },
    Promise,
    setInterval(callback, delay) {
      interval = { callback, cleared: false, delay };
      return interval;
    },
    Date: {
      now() {
        return now;
      }
    },
    setTimeout(callback) {
      timers.push(callback);
      return timers.length;
    }
  };
  context.window = context;
  context.top = {};
  context.parent = context.top;
  context.addEventListener = (type, listener) => {
    if (type === "message") windowMessageListener = listener;
  };
  context.removeEventListener = () => {};
  context.postMessage = () => {};
  context.globalThis = context;
  context.URL = URL;
  vm.createContext(context);
  vm.runInContext(bridgeSource, context);
  timers.shift()();
  now = 500;
  timers.shift()();
  realmWindow = vm.runInContext("window", context);

  const execution = portMessageListener({
    type: "sunox-managed-frame-execute-v2",
    requestId: "request-keepalive",
    provider: "turnstile"
  });
  assert.equal(interval.delay, 20_000);
  interval.callback();
  assert.deepEqual(JSON.parse(JSON.stringify(posted)), [{
    type: "sunox-managed-frame-keepalive-v2",
    requestId: "request-keepalive"
  }]);

  windowMessageListener({
    data: {
      source: "sunox-page-v1",
      requestId: "request-keepalive",
      token: "keepalive-token"
    },
    origin: "https://suno.com",
    source: realmWindow
  });
  await execution;

  assert.equal(interval.cleared, true);
  assert.deepEqual(JSON.parse(JSON.stringify(posted.at(-1))), {
    type: "sunox-managed-frame-result-v2",
    requestId: "request-keepalive",
    token: "keepalive-token",
    errorCode: null
  });
});

test("content bridge reduces forged main-world errors to an allowlisted code", async () => {
  const sentinel = "FORGED___clerk_handshake_MAIN_WORLD_SECRET";
  let portMessageListener;
  let realmWindow;
  let windowMessageListener;
  const posted = [];
  const timers = [];
  let now = 0;
  const port = {
    onDisconnect: {
      addListener() {}
    },
    onMessage: {
      addListener(listener) {
        portMessageListener = listener;
      }
    },
    disconnect() {},
    postMessage(message) {
      posted.push(message);
    }
  };
  const context = {
    chrome: {
      runtime: {
        connect() {
          return port;
        }
      }
    },
    clearInterval() {},
    clearTimeout() {},
    document: {
      readyState: "complete"
    },
    location: {
      href: "https://suno.com/create#sunox-browser-bridge=00000000-0000-4000-8000-000000000001",
      origin: "https://suno.com"
    },
    Promise,
    Date: {
      now() {
        return now;
      }
    },
    setInterval() {
      return 1;
    },
    setTimeout(callback) {
      timers.push(callback);
      return timers.length;
    },
    URL
  };
  context.window = context;
  context.top = {};
  context.parent = context.top;
  context.addEventListener = (type, listener) => {
    if (type === "message") windowMessageListener = listener;
  };
  context.removeEventListener = () => {};
  context.postMessage = () => {};
  context.globalThis = context;
  vm.createContext(context);
  vm.runInContext(bridgeSource, context);
  timers.shift()();
  now = 500;
  timers.shift()();
  realmWindow = vm.runInContext("window", context);

  const execution = portMessageListener({
    type: "sunox-managed-frame-execute-v2",
    requestId: "request-forged-main-world-error",
    provider: "turnstile"
  });
  windowMessageListener({
    data: {
      source: "sunox-page-v1",
      requestId: "request-forged-main-world-error",
      token: null,
      error: sentinel,
      errorCode: sentinel
    },
    origin: "https://suno.com",
    source: realmWindow
  });
  await execution;

  const finalMessage = posted.at(-1);
  assert.equal(JSON.stringify(finalMessage).includes(sentinel), false);
  assert.deepEqual(JSON.parse(JSON.stringify(finalMessage)), {
    type: "sunox-managed-frame-result-v2",
    requestId: "request-forged-main-world-error",
    token: null,
    errorCode: "challenge_failed"
  });
});

test("page bridge executes both invisible challenge providers", async () => {
  const listeners = new Map();
  const pageResults = [];
  let hcaptchaExecute;
  let hcaptchaOptions;
  let turnstileExecute;
  let turnstileCallback;
  let turnstileOptions;
  let realmWindow;
  const context = {
    clearInterval,
    clearTimeout,
    document: {
      body: {
        appendChild() {}
      },
      createElement() {
        return {
          remove() {},
          style: {}
        };
      },
      head: {
        appendChild() {}
      },
      querySelector() {
        return null;
      },
      readyState: "complete"
    },
    hcaptcha: {
      async execute(widgetId, options) {
        hcaptchaExecute = { options, widgetId };
        return { response: "hcaptcha-token" };
      },
      remove() {},
      render(_container, options) {
        hcaptchaOptions = options;
        return "hcaptcha-widget";
      }
    },
    location: {
      href: "https://suno.com/create#sunox-browser-bridge=00000000-0000-4000-8000-000000000001",
      origin: "https://suno.com"
    },
    Promise,
    setInterval,
    setTimeout,
    turnstile: {
      execute(widgetId) {
        turnstileExecute = { widgetId };
        queueMicrotask(() => turnstileCallback("turnstile-token"));
      },
      remove() {},
      render(_container, options) {
        turnstileOptions = options;
        turnstileCallback = options.callback;
        return "turnstile-widget";
      }
    }
  };
  context.window = context;
  context.top = {};
  context.parent = context.top;
  context.addEventListener = (type, listener) => {
    listeners.set(type, listener);
  };
  context.postMessage = (message) => {
    if (message?.source === "sunox-page-v1") pageResults.push(message);
  };
  context.globalThis = context;
  context.URL = URL;
  vm.createContext(context);
  vm.runInContext(pageSource, context);
  realmWindow = vm.runInContext("window", context);

  const send = async (requestId, provider) => {
    await listeners.get("message")({
      data: {
        source: "sunox-extension-v1",
        requestId,
        provider
      },
      origin: "https://suno.com",
      source: realmWindow
    });
    await flushAsync();
  };
  await send("request-hcaptcha", "hcaptcha");
  await send("request-turnstile", "turnstile");

  assert.equal(hcaptchaExecute.widgetId, "hcaptcha-widget");
  assert.equal(hcaptchaExecute.options.async, true);
  assert.equal(hcaptchaOptions.sitekey, "d65453de-3f1a-4aac-9366-a0f06e52b2ce");
  assert.equal(hcaptchaOptions.size, "invisible");
  assert.equal(hcaptchaOptions.sentry, false);
  assert.equal(hcaptchaOptions.endpoint, "https://hcaptcha-endpoint-prod.suno.com");
  assert.equal(hcaptchaOptions.assethost, "https://hcaptcha-assets-prod.suno.com");
  assert.equal(hcaptchaOptions.imghost, "https://hcaptcha-imgs-prod.suno.com");
  assert.equal(hcaptchaOptions.reportapi, "https://hcaptcha-reportapi-prod.suno.com");

  assert.equal(turnstileExecute.widgetId, "turnstile-widget");
  assert.equal(turnstileOptions.sitekey, "0x4AAAAAADI7xDNyj-3LcIbi");
  assert.equal(turnstileOptions.execution, "execute");
  assert.equal(turnstileOptions.appearance, "interaction-only");
  assert.equal(typeof turnstileOptions.callback, "function");
  assert.equal(typeof turnstileOptions["error-callback"], "function");
  assert.equal(typeof turnstileOptions["expired-callback"], "function");
  assert.equal(typeof turnstileOptions["timeout-callback"], "function");
  assert.equal(typeof turnstileOptions["unsupported-callback"], "function");
  assert.equal(typeof turnstileOptions["before-interactive-callback"], "function");
  assert.equal(typeof turnstileOptions["after-interactive-callback"], "function");

  assert.deepEqual(JSON.parse(JSON.stringify(pageResults)), [
    {
      source: "sunox-page-v1",
      requestId: "request-hcaptcha",
      token: "hcaptcha-token"
    },
    {
      source: "sunox-page-v1",
      requestId: "request-turnstile",
      token: "turnstile-token"
    }
  ]);
});

test("page bridge waits for document.body at document_start before mounting hCaptcha", async () => {
  const listeners = new Map();
  const pageResults = [];
  const appended = [];
  let realmWindow;
  const context = {
    clearInterval,
    clearTimeout,
    document: {
      body: null,
      createElement() {
        return {
          remove() {},
          style: {}
        };
      },
      head: {
        appendChild() {}
      },
      querySelector() {
        return null;
      }
    },
    hcaptcha: {
      async execute() {
        return { response: "body-ready-token" };
      },
      remove() {},
      render() {
        return "body-ready-widget";
      }
    },
    location: {
      href: "https://suno.com/create#sunox-browser-bridge=00000000-0000-4000-8000-000000000001",
      origin: "https://suno.com"
    },
    Promise,
    setInterval,
    setTimeout
  };
  context.window = context;
  context.top = {};
  context.parent = context.top;
  context.addEventListener = (type, listener) => {
    listeners.set(type, listener);
  };
  context.postMessage = (message) => {
    if (message?.source === "sunox-page-v1") pageResults.push(message);
  };
  context.globalThis = context;
  context.URL = URL;
  vm.createContext(context);
  vm.runInContext(pageSource, context);
  realmWindow = vm.runInContext("window", context);

  const solve = listeners.get("message")({
    data: {
      source: "sunox-extension-v1",
      requestId: "request-body-ready",
      provider: "hcaptcha"
    },
    origin: "https://suno.com",
    source: realmWindow
  });
  setTimeout(() => {
    context.document.body = {
      appendChild(node) {
        appended.push(node);
      }
    };
  }, 10);
  await solve;
  await flushAsync();

  assert.equal(appended.length, 1);
  assert.deepEqual(JSON.parse(JSON.stringify(pageResults)), [{
    source: "sunox-page-v1",
    requestId: "request-body-ready",
    token: "body-ready-token"
  }]);
});

test("page bridge loads the current Turnstile SDK when the page has none", async () => {
  const listeners = new Map();
  const pageResults = [];
  const loadedScripts = [];
  let turnstileCallback;
  let realmWindow;
  const context = {
    clearInterval: clearTimeout,
    clearTimeout,
    document: {
      body: {
        appendChild() {}
      },
      createElement() {
        return {
          dataset: {},
          remove() {},
          style: {}
        };
      },
      head: {
        appendChild(script) {
          loadedScripts.push(script.src);
          context.turnstile = {
            execute() {
              queueMicrotask(() => turnstileCallback("loaded-sdk-token"));
            },
            remove() {},
            render(_container, options) {
              turnstileCallback = options.callback;
              return "turnstile-widget";
            }
          };
        }
      },
      querySelector() {
        return null;
      }
    },
    location: {
      href: "https://suno.com/create#sunox-browser-bridge=00000000-0000-4000-8000-000000000001",
      origin: "https://suno.com"
    },
    Promise,
    setInterval(callback) {
      return setTimeout(callback, 0);
    },
    setTimeout
  };
  context.window = context;
  context.top = {};
  context.parent = context.top;
  context.addEventListener = (type, listener) => {
    listeners.set(type, listener);
  };
  context.postMessage = (message) => {
    if (message?.source === "sunox-page-v1") pageResults.push(message);
  };
  context.globalThis = context;
  context.URL = URL;
  vm.createContext(context);
  vm.runInContext(pageSource, context);
  realmWindow = vm.runInContext("window", context);

  await listeners.get("message")({
    data: {
      source: "sunox-extension-v1",
      requestId: "request-turnstile-loader",
      provider: "turnstile"
    },
    origin: "https://suno.com",
    source: realmWindow
  });
  await flushAsync();

  assert.deepEqual(loadedScripts, [
    "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit"
  ]);
  assert.deepEqual(JSON.parse(JSON.stringify(pageResults)), [{
    source: "sunox-page-v1",
    requestId: "request-turnstile-loader",
    token: "loaded-sdk-token"
  }]);
});

test("page bridge loads the current hCaptcha SDK when the page has none", async () => {
  const listeners = new Map();
  const pageResults = [];
  const loadedScripts = [];
  let realmWindow;
  const context = {
    clearInterval: clearTimeout,
    clearTimeout,
    document: {
      body: {
        appendChild() {}
      },
      createElement() {
        return {
          dataset: {},
          remove() {},
          style: {}
        };
      },
      head: {
        appendChild(script) {
          loadedScripts.push({
            marker: script.dataset.sunoxHcaptcha,
            src: script.src
          });
          context.hcaptcha = {
            async execute() {
              return { response: "loaded-hcaptcha-token" };
            },
            remove() {},
            render() {
              return "hcaptcha-widget";
            }
          };
        }
      },
      querySelector() {
        return null;
      }
    },
    location: {
      href: "https://suno.com/create#sunox-browser-bridge=00000000-0000-4000-8000-000000000001",
      origin: "https://suno.com"
    },
    Promise,
    setInterval(callback) {
      return setTimeout(callback, 0);
    },
    setTimeout
  };
  context.window = context;
  context.top = {};
  context.parent = context.top;
  context.addEventListener = (type, listener) => {
    listeners.set(type, listener);
  };
  context.postMessage = (message) => {
    if (message?.source === "sunox-page-v1") pageResults.push(message);
  };
  context.globalThis = context;
  context.URL = URL;
  vm.createContext(context);
  vm.runInContext(pageSource, context);
  realmWindow = vm.runInContext("window", context);

  await listeners.get("message")({
    data: {
      source: "sunox-extension-v1",
      requestId: "request-hcaptcha-loader",
      provider: "hcaptcha"
    },
    origin: "https://suno.com",
    source: realmWindow
  });
  await flushAsync();

  assert.deepEqual(loadedScripts, [{
    marker: "true",
    src: "https://hcaptcha-endpoint-prod.suno.com/1/api.js?render=explicit"
  }]);
  assert.deepEqual(JSON.parse(JSON.stringify(pageResults)), [{
    source: "sunox-page-v1",
    requestId: "request-hcaptcha-loader",
    token: "loaded-hcaptcha-token"
  }]);
});

test("page bridge reports fatal Turnstile callbacks", async () => {
  const cases = [
    ["expired-callback", undefined, "challenge_expired"],
    ["unsupported-callback", undefined, "unsupported_browser"]
  ];

  for (const [callbackName, callbackArgument, expectedErrorCode] of cases) {
    const listeners = new Map();
    const pageResults = [];
    let realmWindow;
    const context = {
      clearInterval,
      clearTimeout,
      document: {
        body: {
          appendChild() {}
        },
        createElement() {
          return {
            remove() {},
            style: {}
          };
        },
        head: {
          appendChild() {}
        },
        querySelector() {
          return null;
        }
      },
      location: {
        href: "https://suno.com/create#sunox-browser-bridge=00000000-0000-4000-8000-000000000001",
        origin: "https://suno.com"
      },
      Promise,
      setInterval,
      setTimeout,
      turnstile: {
        execute() {},
        remove() {},
        render(_container, options) {
          queueMicrotask(() => options[callbackName](callbackArgument));
          return "turnstile-widget";
        }
      }
    };
    context.window = context;
    context.top = {};
    context.parent = context.top;
    context.addEventListener = (type, listener) => {
      listeners.set(type, listener);
    };
    context.postMessage = (message) => {
      if (message?.source === "sunox-page-v1") pageResults.push(message);
    };
    context.globalThis = context;
    context.URL = URL;
    vm.createContext(context);
    vm.runInContext(pageSource, context);
    realmWindow = vm.runInContext("window", context);

    await listeners.get("message")({
      data: {
        source: "sunox-extension-v1",
        requestId: `request-${callbackName}`,
        provider: "turnstile"
      },
      origin: "https://suno.com",
      source: realmWindow
    });
    await flushAsync();

    assert.equal(pageResults.length, 1);
    assert.equal(pageResults[0].requestId, `request-${callbackName}`);
    assert.equal("token" in pageResults[0], false);
    assert.equal(pageResults[0].errorCode, expectedErrorCode);
  }
});

test("page bridge fails immediately when Turnstile requires interaction", async () => {
  const listeners = new Map();
  const pageResults = [];
  const timeoutDelays = [];
  let nextTimeoutId = 0;
  let realmWindow;
  const context = {
    clearInterval,
    clearTimeout() {},
    document: {
      body: {
        appendChild() {}
      },
      createElement() {
        return {
          remove() {},
          style: {}
        };
      },
      head: {
        appendChild() {}
      },
      querySelector() {
        return null;
      }
    },
    location: {
      href: "https://suno.com/create#sunox-browser-bridge=00000000-0000-4000-8000-000000000001",
      origin: "https://suno.com"
    },
    Promise,
    setInterval,
    setTimeout(_callback, delay) {
      timeoutDelays.push(delay);
      nextTimeoutId += 1;
      return nextTimeoutId;
    },
    turnstile: {
      execute() {},
      remove() {},
      render(_container, options) {
        queueMicrotask(() => {
          options["before-interactive-callback"]();
        });
        return "turnstile-widget";
      }
    }
  };
  context.window = context;
  context.top = {};
  context.parent = context.top;
  context.addEventListener = (type, listener) => {
    listeners.set(type, listener);
  };
  context.postMessage = (message) => {
    if (message?.source === "sunox-page-v1") pageResults.push(message);
  };
  context.globalThis = context;
  context.URL = URL;
  vm.createContext(context);
  vm.runInContext(pageSource, context);
  realmWindow = vm.runInContext("window", context);

  await listeners.get("message")({
    data: {
      source: "sunox-extension-v1",
      requestId: "request-turnstile-recovery",
      provider: "turnstile"
    },
    origin: "https://suno.com",
    source: realmWindow
  });
  await flushAsync();

  assert.deepEqual(timeoutDelays, [15_000]);
  assert.equal(pageResults.length, 1);
  assert.equal(pageResults[0].requestId, "request-turnstile-recovery");
  assert.equal(
    pageResults[0].errorCode,
    "interactive_browser_required"
  );
  assert.equal("token" in pageResults[0], false);
});

test("page bridge fails immediately when invisible hCaptcha opens a challenge", async () => {
  const listeners = new Map();
  const pageResults = [];
  let options;
  let realmWindow;
  const context = {
    clearInterval,
    clearTimeout,
    document: {
      body: {
        appendChild() {}
      },
      createElement() {
        return {
          remove() {},
          style: {}
        };
      },
      head: {
        appendChild() {}
      },
      querySelector() {
        return null;
      }
    },
    hcaptcha: {
      execute() {
        queueMicrotask(() => options["open-callback"]());
        return new Promise(() => {});
      },
      remove() {},
      render(_container, renderedOptions) {
        options = renderedOptions;
        return "hcaptcha-widget";
      }
    },
    location: {
      href: "https://suno.com/create#sunox-browser-bridge=00000000-0000-4000-8000-000000000001",
      origin: "https://suno.com"
    },
    Promise,
    setInterval,
    setTimeout
  };
  context.window = context;
  context.top = {};
  context.parent = context.top;
  context.addEventListener = (type, listener) => {
    listeners.set(type, listener);
  };
  context.postMessage = (message) => {
    if (message?.source === "sunox-page-v1") pageResults.push(message);
  };
  context.globalThis = context;
  context.URL = URL;
  vm.createContext(context);
  vm.runInContext(pageSource, context);
  realmWindow = vm.runInContext("window", context);

  await listeners.get("message")({
    data: {
      source: "sunox-extension-v1",
      requestId: "request-hcaptcha-interactive",
      provider: "hcaptcha"
    },
    origin: "https://suno.com",
    source: realmWindow
  });
  await flushAsync();

  assert.equal(pageResults.length, 1);
  assert.equal(pageResults[0].requestId, "request-hcaptcha-interactive");
  assert.equal(
    pageResults[0].errorCode,
    "interactive_browser_required"
  );
  assert.equal("token" in pageResults[0], false);
});

test("isolated Turnstile solver executes the recoverable callback lifecycle", async () => {
  const timeoutDelays = [];
  let nextTimeoutId = 0;
  let options;
  let removedContainer = false;
  let removedWidget;
  const context = {
    clearTimeout() {},
    document: {
      body: {
        appendChild() {}
      },
      createElement() {
        return {
          remove() {
            removedContainer = true;
          }
        };
      }
    },
    Promise,
    setTimeout(_callback, delay) {
      timeoutDelays.push(delay);
      nextTimeoutId += 1;
      return nextTimeoutId;
    },
    turnstile: {
      execute(widgetId) {
        assert.equal(widgetId, "isolated-turnstile-widget");
        queueMicrotask(() => {
          options["error-callback"]("network");
          options["before-interactive-callback"]();
          options["error-callback"]("interactive-network");
          options["timeout-callback"]();
          options["after-interactive-callback"]();
          options.callback("isolated-recovered-token");
        });
      },
      remove(widgetId) {
        removedWidget = widgetId;
      },
      render(_container, renderedOptions) {
        options = renderedOptions;
        return "isolated-turnstile-widget";
      }
    }
  };
  vm.createContext(context);

  const result = await vm.runInContext(isolatedTurnstileSource, context);

  assert.equal(result, "isolated-recovered-token");
  assert.equal(options.sitekey, "0x4AAAAAADI7xDNyj-3LcIbi");
  assert.equal(options.execution, "execute");
  assert.equal(options.appearance, "interaction-only");
  assert.deepEqual(
    timeoutDelays,
    [15_000, 15_000, 120_000, 120_000, 15_000]
  );
  assert.equal(removedWidget, "isolated-turnstile-widget");
  assert.equal(removedContainer, true);
});

test("isolated Turnstile solver reports fatal callbacks", async () => {
  for (const [callbackName, expected] of [
    ["expired-callback", "ERR:Turnstile token expired"],
    ["unsupported-callback", "ERR:Turnstile unsupported in this browser"]
  ]) {
    let options;
    const context = {
      clearTimeout() {},
      document: {
        body: {
          appendChild() {}
        },
        createElement() {
          return {
            remove() {}
          };
        }
      },
      Promise,
      setTimeout() {
        return 1;
      },
      turnstile: {
        execute() {
          queueMicrotask(() => options[callbackName]());
        },
        remove() {},
        render(_container, renderedOptions) {
          options = renderedOptions;
          return "isolated-turnstile-widget";
        }
      }
    };
    vm.createContext(context);

    assert.equal(await vm.runInContext(isolatedTurnstileSource, context), expected);
  }
});

test("page bridge recovers after a hanging hCaptcha execution deadline", async () => {
  const listeners = new Map();
  const pageResults = [];
  let attempts = 0;
  let realmWindow;
  const context = {
    clearInterval,
    clearTimeout,
    document: {
      body: {
        appendChild() {}
      },
      createElement() {
        return {
          remove() {},
          style: {}
        };
      },
      head: {
        appendChild() {}
      },
      querySelector() {
        return null;
      }
    },
    hcaptcha: {
      execute() {
        attempts += 1;
        return attempts === 1
          ? new Promise(() => {})
          : Promise.resolve({ response: "recovered-token" });
      },
      remove() {},
      render() {
        return `hcaptcha-widget-${attempts + 1}`;
      }
    },
    location: {
      href: "https://suno.com/create#sunox-browser-bridge=00000000-0000-4000-8000-000000000001",
      origin: "https://suno.com"
    },
    Promise,
    setInterval,
    setTimeout(callback, delay) {
      return setTimeout(callback, delay === 15_000 ? 0 : delay);
    }
  };
  context.window = context;
  context.top = {};
  context.parent = context.top;
  context.addEventListener = (type, listener) => {
    listeners.set(type, listener);
  };
  context.postMessage = (message) => {
    if (message?.source === "sunox-page-v1") pageResults.push(message);
  };
  context.globalThis = context;
  context.URL = URL;
  vm.createContext(context);
  vm.runInContext(pageSource, context);
  realmWindow = vm.runInContext("window", context);

  const send = async (requestId) => {
    await listeners.get("message")({
      data: {
        source: "sunox-extension-v1",
        requestId,
        provider: "hcaptcha"
      },
      origin: "https://suno.com",
      source: realmWindow
    });
    await flushAsync();
  };
  await send("request-hanging");
  await send("request-recovered");

  assert.equal(pageResults[0].requestId, "request-hanging");
  assert.equal(
    pageResults[0].errorCode,
    "silent_challenge_unavailable"
  );
  assert.deepEqual(JSON.parse(JSON.stringify(pageResults[1])), {
    source: "sunox-page-v1",
    requestId: "request-recovered",
    token: "recovered-token"
  });
});

test("loopback transport acknowledges a signed probe without returning a challenge", async () => {
  const secret = "a".repeat(64);
  const requests = [];
  const serverNonce = "server-nonce-00000001";
  let acknowledged = false;
  const context = {
    AbortController,
    atob,
    btoa,
    clearTimeout,
    crypto: webcrypto,
    DataView,
    fetch: async (url, options) => {
      assert.equal(options.redirect, "error");
      const path = new URL(url).pathname;
      const body = JSON.parse(options.body);
      requests.push({ body, path });
      if (path === "/v3/challenge/hello") {
        return new Response(JSON.stringify({
          version: 3,
          server_nonce: serverNonce,
          proof: bridgeProof(secret, "sunox-bridge-server-v3", [
            29_764,
            body.client_nonce,
            serverNonce
          ])
        }), {
          headers: {
            "Content-Type": "application/json"
          }
        });
      }
      if (path === "/v3/challenge/claim") {
        assert.equal(body.runtime_build, "0.3.15");
        assert.equal(
          body.proof,
          bridgeProof(secret, "sunox-bridge-client-v3", [
            29_764,
            body.client_nonce,
            body.server_nonce,
            "0.3.15",
            body.client_id,
            body.page_url
          ])
        );
        return new Response(JSON.stringify({
          version: 3,
          probe: true,
          request_id: "probe-request"
        }), {
          headers: {
            "Content-Type": "application/json"
          }
        });
      }
      if (path === "/v3/challenge/probe-ack") {
        assert.equal(body.runtime_build, "0.3.15");
        assert.equal(
          body.proof,
          bridgeProof(secret, "sunox-bridge-probe-ack-v3", [
            29_764,
            body.client_nonce,
            body.server_nonce,
            body.request_id,
            body.runtime_build
          ])
        );
        acknowledged = true;
        return new Response(null, { status: 204 });
      }
      throw new Error(`unexpected loopback path ${path}`);
    },
    Promise,
    setTimeout,
    SUNOX_BRIDGE_CONFIG: {
      loopback: {
        portCount: 1,
        portStart: 29_764,
        protocolVersion: 3,
        runtimeBuild: "0.3.15",
        sharedSecret: secret
      }
    },
    TextDecoder,
    TextEncoder,
    Uint8Array,
    URL
  };
  context.globalThis = context;
  vm.createContext(context);
  vm.runInContext(loopbackTransportSource, context);

  const challenge = await context.SUNOX_BRIDGE_TRANSPORTS.loopback.claimChallenge({
    clientId: "offscreen-client",
    pageUrl: "https://suno.com/create#sunox-browser-bridge"
  });

  assert.equal(challenge, null);
  assert.equal(acknowledged, true);
  assert.deepEqual(requests.map(({ path }) => path), [
    "/v3/challenge/hello",
    "/v3/challenge/claim",
    "/v3/challenge/probe-ack"
  ]);
});

test("loopback transport deadline covers a response body that never finishes", async () => {
  let aborted = false;
  let redirectMode;
  const transport = loadLoopbackTransport(async (_url, options) => {
    redirectMode = options.redirect;
    options.signal.addEventListener("abort", () => {
      aborted = true;
    });
    return {
      ok: true,
      headers: {
        get(name) {
          return name.toLowerCase() === "content-type"
            ? "application/json"
            : null;
        }
      },
      body: {
        getReader() {
          return {
            async read() {
              return await new Promise(() => {});
            }
          };
        }
      }
    };
  }, {
    requestTimeoutMs: 20
  });

  const started = Date.now();
  const challenge = await transport.claimChallenge({
    clientId: "offscreen-client",
    pageUrl: "https://suno.com/create#sunox-browser-bridge"
  });

  assert.equal(challenge, null);
  assert.equal(redirectMode, "error");
  assert.equal(aborted, true);
  assert.ok(Date.now() - started < 250, "body timeout must remain tightly bounded");
});

test("loopback transport rejects non-JSON and chunked JSON bodies over 4 KiB", async () => {
  let oversizedReaderCancelled = false;
  const transport = loadLoopbackTransport(async (url, options) => {
    assert.equal(options.redirect, "error");
    const port = Number(new URL(url).port);
    const json = port === 29_764;
    return {
      ok: true,
      headers: {
        get(name) {
          if (name.toLowerCase() === "content-type") {
            return json ? "application/json; charset=utf-8" : "text/plain";
          }
          return null;
        }
      },
      body: {
        getReader() {
          return {
            async read() {
              return {
                done: false,
                value: new Uint8Array(4 * 1024 + 1)
              };
            },
            async cancel() {
              oversizedReaderCancelled = true;
            }
          };
        }
      }
    };
  }, {
    portCount: 2
  });

  const challenge = await transport.claimChallenge({
    clientId: "offscreen-client",
    pageUrl: "https://suno.com/create#sunox-browser-bridge"
  });

  assert.equal(challenge, null);
  assert.equal(oversizedReaderCancelled, true);
});

test("offscreen heartbeat reports only recent poll-worker ticks as healthy", () => {
  const runtimeListeners = new Set();
  let now = 1_000;
  let pollWorkerErrorListener;
  let pollWorkerMessageListener;
  const scheduledRestarts = [];
  let terminatedWorkers = 0;
  let workerInstances = 0;
  const sender = {
    id: "abcdefghijklmnopabcdefghijklmnop"
  };
  const context = {
    chrome: {
      runtime: {
        id: sender.id,
        onMessage: {
          addListener(listener) {
            runtimeListeners.add(listener);
          },
          removeListener(listener) {
            runtimeListeners.delete(listener);
          }
        },
        async sendMessage() {
          return undefined;
        }
      }
    },
    clearTimeout() {},
    crypto,
    Date: {
      now() {
        return now;
      }
    },
    document: {
      body: {
        appendChild() {}
      },
      createElement() {
        return {
          remove() {},
          sandbox: {
            add() {}
          },
          style: {}
        };
      }
    },
    Promise,
    setTimeout(callback, delay) {
      scheduledRestarts.push({ callback, delay });
      return scheduledRestarts.length;
    },
    SUNOX_BRIDGE_CONFIG: {
      schemaVersion: 1,
      transport: "test"
    },
    SUNOX_BRIDGE_SHARED: {
      errorMessage(error) {
        return error instanceof Error ? error.message : String(error);
      }
    },
    SUNOX_BRIDGE_TRANSPORTS: {
      test: {
        contractVersion: 1,
        async claimChallenge() {
          return null;
        },
        async submitResult() {
          return { accepted: true };
        }
      }
    },
    Worker: class {
      constructor() {
        workerInstances += 1;
      }

      addEventListener(type, listener) {
        if (type === "message") pollWorkerMessageListener = listener;
        if (type === "error") pollWorkerErrorListener = listener;
      }

      terminate() {
        terminatedWorkers += 1;
      }
    }
  };
  context.globalThis = context;
  vm.createContext(context);
  vm.runInContext(offscreenSource, context);

  const ping = () => {
    let response;
    for (const listener of runtimeListeners) {
      listener(
        { type: "sunox-offscreen-ping-v1" },
        sender,
        (value) => {
          response = value;
        }
      );
    }
    return response;
  };

  assert.deepEqual(JSON.parse(JSON.stringify(ping())), {
    busy: false,
    busySince: null,
    type: "sunox-offscreen-pong-v1",
    pollWorkerAgeMs: null,
    pollWorkerHealthy: false
  });

  pollWorkerMessageListener({ data: { type: "sunox-poll" } });
  now += 250;
  assert.deepEqual(JSON.parse(JSON.stringify(ping())), {
    busy: false,
    busySince: null,
    type: "sunox-offscreen-pong-v1",
    pollWorkerAgeMs: 250,
    pollWorkerHealthy: true
  });

  now += 5_001;
  assert.equal(ping().pollWorkerHealthy, false);
  pollWorkerErrorListener();
  assert.equal(ping().pollWorkerAgeMs, null);
  assert.equal(terminatedWorkers, 1);
  assert.equal(scheduledRestarts.length, 1);
  assert.equal(scheduledRestarts[0].delay, 250);

  scheduledRestarts[0].callback();
  assert.equal(workerInstances, 2);
  pollWorkerMessageListener({ data: { type: "sunox-poll" } });
  assert.equal(ping().pollWorkerHealthy, true);
});

test("offscreen heartbeat protects a claim request while its response is in flight", async () => {
  const runtimeListeners = new Set();
  let now = 1_000;
  let pollWorkerMessageListener;
  let resolveClaim;
  const sender = {
    id: "abcdefghijklmnopabcdefghijklmnop"
  };
  const context = {
    chrome: {
      runtime: {
        id: sender.id,
        onMessage: {
          addListener(listener) {
            runtimeListeners.add(listener);
          },
          removeListener(listener) {
            runtimeListeners.delete(listener);
          }
        },
        async sendMessage() {
          return undefined;
        }
      }
    },
    clearTimeout() {},
    crypto,
    Date: {
      now() {
        return now;
      }
    },
    document: {
      body: {
        appendChild() {}
      },
      createElement() {
        return {
          remove() {},
          sandbox: {
            add() {}
          },
          style: {}
        };
      }
    },
    Promise,
    setTimeout() {
      return 1;
    },
    SUNOX_BRIDGE_CONFIG: {
      schemaVersion: 1,
      transport: "test"
    },
    SUNOX_BRIDGE_SHARED: {
      errorMessage(error) {
        return error instanceof Error ? error.message : String(error);
      }
    },
    SUNOX_BRIDGE_TRANSPORTS: {
      test: {
        contractVersion: 1,
        async claimChallenge() {
          return await new Promise((resolve) => {
            resolveClaim = resolve;
          });
        },
        async submitResult() {
          return { accepted: true };
        }
      }
    },
    Worker: class {
      addEventListener(type, listener) {
        if (type === "message") pollWorkerMessageListener = listener;
      }
    }
  };
  context.globalThis = context;
  vm.createContext(context);
  vm.runInContext(offscreenSource, context);

  const dispatch = (message) => {
    let response;
    for (const listener of runtimeListeners) {
      listener(message, sender, (value) => {
        response = value;
      });
    }
    return response;
  };

  pollWorkerMessageListener({ data: { type: "sunox-poll" } });
  dispatch({ type: "sunox-offscreen-start-v1" });
  assert.equal(typeof resolveClaim, "function");

  now += 30_000;
  assert.deepEqual(
    JSON.parse(JSON.stringify(dispatch({ type: "sunox-offscreen-ping-v1" }))),
    {
      busy: true,
      busySince: 1_000,
      type: "sunox-offscreen-pong-v1",
      pollWorkerAgeMs: 30_000,
      pollWorkerHealthy: false
    }
  );

  resolveClaim(null);
  await flushAsync();
  assert.equal(
    dispatch({ type: "sunox-offscreen-ping-v1" }).busy,
    false,
    "the in-flight marker must clear after the claim response settles"
  );
});

test("offscreen solves a claimed challenge in an invisible iframe without requesting a window", async () => {
  const runtimeListeners = new Set();
  const runtimeMessages = [];
  const submitted = [];
  const frames = [];
  let claims = 0;
  const sender = {
    id: "abcdefghijklmnopabcdefghijklmnop"
  };
  const dispatchRuntimeMessage = (message) => {
    for (const listener of [...runtimeListeners]) {
      listener(message, sender, () => {});
    }
  };
  const detachedSetTimeout = (callback, delay) => {
    const timer = setTimeout(callback, delay);
    timer.unref?.();
    return timer;
  };
  const context = {
    chrome: {
      runtime: {
        id: sender.id,
        onMessage: {
          addListener(listener) {
            runtimeListeners.add(listener);
          },
          removeListener(listener) {
            runtimeListeners.delete(listener);
          }
        },
        async sendMessage(message) {
          runtimeMessages.push(message);
          if (message.type !== "sunox-managed-frame-execute-v2") return undefined;
          queueMicrotask(() => {
            dispatchRuntimeMessage({
              type: "sunox-managed-frame-result-v2",
              nonce: message.nonce,
              requestId: message.requestId,
              token: "offscreen-token"
            });
          });
          return { accepted: true };
        }
      }
    },
    clearTimeout,
    crypto,
    Date,
    document: {
      body: {
        appendChild(frame) {
          frames.push(frame);
          const nonce = new URL(frame.src).hash.slice(
            "#sunox-browser-bridge=".length
          );
          queueMicrotask(() => {
            dispatchRuntimeMessage({
              type: "sunox-managed-frame-ready-v2",
              nonce
            });
          });
        }
      },
      createElement(name) {
        assert.equal(name, "iframe");
        return {
          addEventListener() {},
          remove() {},
          sandbox: {
            add() {}
          },
          style: {}
        };
      }
    },
    Promise,
    setTimeout: detachedSetTimeout,
    SUNOX_BRIDGE_CONFIG: {
      schemaVersion: 1,
      transport: "test"
    },
    SUNOX_BRIDGE_SHARED: {
      errorMessage(error) {
        return error instanceof Error ? error.message : String(error);
      }
    },
    SUNOX_BRIDGE_TRANSPORTS: {
      test: {
        contractVersion: 1,
        async claimChallenge() {
          claims += 1;
          return claims === 1
            ? {
                provider: "hcaptcha",
                requestId: "request-offscreen",
                transportReceipt: "receipt"
              }
            : null;
        },
        async submitResult(message) {
          submitted.push(message);
          return { accepted: true };
        }
      }
    },
    Worker: class {
      addEventListener() {}
    }
  };
  context.globalThis = context;
  vm.createContext(context);
  vm.runInContext(offscreenSource, context);
  dispatchRuntimeMessage({ type: "sunox-offscreen-start-v1" });
  await flushAsync();
  await flushAsync();

  assert.equal(claims, 1);
  assert.equal(frames.length, 1);
  assert.match(
    frames[0].src,
    /^https:\/\/suno\.com\/create#sunox-browser-bridge=[0-9a-f-]+$/
  );
  assert.equal(
    runtimeMessages.some(
      (message) => message.type === "sunox-managed-window-start-v1"
    ),
    false
  );
  assert.deepEqual(JSON.parse(JSON.stringify(submitted)), [{
    transportReceipt: "receipt",
    requestId: "request-offscreen",
    token: "offscreen-token",
    error: null
  }]);
});

test("offscreen bundle exposes no managed-window fallback", async () => {
  const source = offscreenSource;
  assert.equal(source.includes("sunox-managed-window-start-v1"), false);
  assert.equal(source.includes("sunox-managed-window-close-v1"), false);
  assert.equal(source.includes("sunox-managed-window-result-v1"), false);
  assert.ok(source.includes('document.createElement("iframe")'));
});

test("all Browser Bridge scripts parse as standalone extension scripts", () => {
  for (const [name, source] of [
    ["service-worker.js", serviceWorkerSource],
    ["offscreen.js", offscreenSource],
    ["bridge.js", bridgeSource],
    ["page.js", pageSource],
    ["transport-loopback.js", loopbackTransportSource]
  ]) {
    assert.doesNotThrow(() => new vm.Script(source), `${name} should parse`);
  }
});

function managedPageGateActivated(source, href, { framed = true } = {}) {
  let connected = false;
  let messageListenerRegistered = false;
  const port = {
    disconnect() {},
    onDisconnect: {
      addListener() {}
    },
    onMessage: {
      addListener() {}
    },
    postMessage() {}
  };
  const context = {
    chrome: {
      runtime: {
        connect() {
          connected = true;
          return port;
        }
      }
    },
    clearInterval() {},
    clearTimeout() {},
    location: {
      href,
      origin: new URL(href).origin
    },
    Promise,
    setInterval() {
      return 1;
    },
    setTimeout() {
      return 1;
    },
    URL
  };
  context.window = context;
  context.top = framed ? {} : context;
  context.parent = framed ? context.top : context;
  context.addEventListener = (type) => {
    if (type === "message") messageListenerRegistered = true;
  };
  context.postMessage = () => {};
  context.globalThis = context;
  vm.createContext(context);
  vm.runInContext(source, context);
  return connected
    || messageListenerRegistered
    || context.__sunoxBridgeContentLoaded === true
    || context.__sunoxBridgePageLoaded === true;
}

test("managed page URL parser rejects ambiguous or oversized URLs", () => {
  const nonce = "00000000-0000-4000-8000-000000000001";
  const hash = `#sunox-browser-bridge=${nonce}`;
  const invalidUrls = [
    ["credentials", `https://user:password@suno.com/create${hash}`],
    ["port", `https://suno.com:8443/create${hash}`],
    [
      "duplicate query",
      `https://suno.com/create?__clerk_handshake=one&__clerk_handshake=two${hash}`
    ],
    [
      "extra query",
      `https://suno.com/create?__clerk_handshake=one&unexpected=two${hash}`
    ],
    [
      "oversized handshake",
      `https://suno.com/create?__clerk_handshake=${"x".repeat(65_537)}${hash}`
    ],
    [
      "oversized URL",
      `https://suno.com/create?__clerk_handshake=${"x".repeat(131_073)}${hash}`
    ],
    [
      "wrong hash",
      `https://suno.com/create#sunox-browser-bridge=${nonce}-unexpected`
    ]
  ];

  for (const [scriptName, source] of [
    ["bridge.js", bridgeSource],
    ["page.js", pageSource]
  ]) {
    for (const [reason, href] of invalidUrls) {
      assert.equal(
        managedPageGateActivated(source, href),
        false,
        `${scriptName} must reject a managed page URL with ${reason}`
      );
    }
  }
});

test("managed page scripts activate only in a first-level extension frame", () => {
  const href = "https://suno.com/create#sunox-browser-bridge=00000000-0000-4000-8000-000000000001";
  for (const source of [bridgeSource, pageSource]) {
    assert.equal(managedPageGateActivated(source, href), true);
    assert.equal(
      managedPageGateActivated(source, href, { framed: false }),
      false
    );
  }
});

test("managed page scripts survive the exact Clerk return until the URL is cleaned", () => {
  const href = "https://suno.com/create?__clerk_handshake=opaque-return#sunox-browser-bridge=00000000-0000-4000-8000-000000000001";
  for (const source of [bridgeSource, pageSource]) {
    assert.equal(managedPageGateActivated(source, href), true);
  }
});

test("manifest and scripts expose only the invisible offscreen-frame transport", () => {
  assert.equal(manifest.version, "0.3.15");
  assert.equal(manifest.version_name, "__SUNOX_VERSION__");
  assert.equal(manifest.permissions.includes("declarativeNetRequestFeedback"), false);
  assert.ok(manifest.permissions.includes("declarativeNetRequestWithHostAccess"));
  assert.equal(manifest.permissions.includes("activeTab"), false);
  assert.ok(manifest.permissions.includes("offscreen"));
  assert.equal(manifest.permissions.includes("tabs"), false);
  assert.equal(manifest.permissions.includes("storage"), false);
  assert.equal(manifest.permissions.includes("system.display"), false);
  assert.ok(manifest.host_permissions.includes("https://auth.suno.com/*"));
  assert.ok(manifest.content_scripts.every((script) => script.all_frames === true));
  assert.equal(
    manifest.content_scripts.some((script) =>
      script.matches.some((match) => match.includes("auth.suno.com"))
    ),
    false
  );
  for (const path of [
    "/v3/challenge/hello",
    "/v3/challenge/claim",
    "/v3/challenge/probe-ack",
    "/v3/challenge/result"
  ]) {
    assert.ok(loopbackTransportSource.includes(path));
  }
  for (const context of [
    "sunox-bridge-server-v3",
    "sunox-bridge-client-v3",
    "sunox-bridge-probe-ack-v3",
    "sunox-bridge-result-v3",
    "sunox-bridge-receipt-v3"
  ]) {
    assert.ok(loopbackTransportSource.includes(context));
  }
  assert.equal(loopbackTransportSource.includes("/v2/challenge/"), false);
  assert.equal(loopbackTransportSource.includes("sunox-bridge-receipt-v2"), false);
  assert.ok(loopbackTransportSource.includes("runtime_build"));

  assert.ok(offscreenSource.includes('document.createElement("iframe")'));
  assert.ok(serviceWorkerSource.includes("declarativeNetRequest"));
  assert.ok(serviceWorkerSource.includes("addRules"));
  assert.ok(serviceWorkerSource.includes("content-security-policy"));
  assert.ok(serviceWorkerSource.includes("x-frame-options"));
  assert.ok(serviceWorkerSource.includes("cspWithoutFrameAncestors"));
  assert.equal(serviceWorkerSource.includes("chrome.windows"), false);
  assert.equal(serviceWorkerSource.includes("chrome.tabs.create"), false);
  assert.equal(serviceWorkerSource.includes("chrome.tabs.update"), false);
  assert.equal(serviceWorkerSource.includes("chrome.storage"), false);
  assert.ok(serviceWorkerSource.includes("isOffscreenSender(sender)"));
  assert.equal(
    [serviceWorkerSource, offscreenSource, bridgeSource].some((source) =>
      source.includes("sunox-managed-window")
    ),
    false
  );
  assert.ok(bridgeSource.includes('"sunox-managed-frame-v2"'));
  assert.ok(bridgeSource.includes("sunox-managed-frame-execute-v2"));
  assert.ok(bridgeSource.includes("sunox-managed-frame-keepalive-v2"));
  assert.ok(bridgeSource.includes("sunox-managed-frame-result-v2"));
  const sdkReadyTimeout = numericConstant(
    pageSource,
    "CHALLENGE_SDK_READY_TIMEOUT_MS"
  );
  const hcaptchaSilentTimeout = numericConstant(
    pageSource,
    "HCAPTCHA_SILENT_TIMEOUT_MS"
  );
  const idleTimeout = numericConstant(pageSource, "TURNSTILE_IDLE_TIMEOUT_MS");
  const pageTimeout = numericConstant(bridgeSource, "challengePageTimeoutMs");
  const frameReadyTimeout = numericConstant(
    offscreenSource,
    "managedFrameReadyTimeoutMs"
  );
  const frameResultTimeout = numericConstant(
    offscreenSource,
    "managedFrameResultTimeoutMs"
  );
  const offscreenBusyMaxAge = numericConstant(
    serviceWorkerSource,
    "OFFSCREEN_BUSY_MAX_AGE_MS"
  );
  assert.ok(
    pageTimeout > sdkReadyTimeout + idleTimeout + hcaptchaSilentTimeout
  );
  assert.ok(frameResultTimeout > pageTimeout);
  assert.ok(
    offscreenBusyMaxAge > frameReadyTimeout + frameResultTimeout
  );
  assert.equal(offscreenBusyMaxAge, 125_000);
  assert.equal(pageSource.includes("25_000"), false);
  assert.ok(pageSource.includes("TURNSTILE_HIDDEN_STYLE"));
  assert.equal(pageSource.includes("TURNSTILE_INTERACTIVE_STYLE"), false);
  assert.ok(pageSource.includes("interactive_browser_required"));
  assert.ok(pageSource.includes("silent_challenge_unavailable"));
  assert.equal(pageSource.includes(
    'container.style.cssText = "position:fixed;top:-9999px;left:-9999px;pointer-events:none"'
  ), true);
  assert.ok(pageSource.includes("MANAGED_NONCE_PATTERN"));
  assert.ok(bridgeSource.includes("MANAGED_NONCE_PATTERN"));
  assert.ok(pageSource.includes("window === window.top"));
  assert.ok(bridgeSource.includes("window === window.top"));
  assert.equal(offscreenSource.includes("contentWindow.postMessage"), false);
  assert.equal(offscreenSource.includes("chrome.windows"), false);
  assert.equal(offscreenSource.includes("chrome.tabs"), false);
});
