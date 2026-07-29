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
const bridgeContractSource = await readFile(
  new URL("../src/captcha/bridge_contract.rs", import.meta.url),
  "utf8"
);
const runtimeBuildMatch = bridgeContractSource.match(
  /BROWSER_BRIDGE_RUNTIME_BUILD:\s*&str\s*=\s*"([^"]+)"/
);
assert.ok(runtimeBuildMatch, "missing Browser Bridge runtime build contract");
const runtimeBuild = runtimeBuildMatch[1];

function sourceNumber(source, name) {
  const match = source.match(new RegExp(
    `const ${name} = ([0-9_]+);`
  ));
  assert.ok(match, `missing numeric source constant ${name}`);
  return Number(match[1].replaceAll("_", ""));
}

const managedFramePrepareTimeoutMs = sourceNumber(
  offscreenSource,
  "managedFramePrepareTimeoutMs"
);
const managedFrameReadyTimeoutMs = sourceNumber(
  offscreenSource,
  "managedFrameReadyTimeoutMs"
);
const managedFrameReleaseTimeoutMs = sourceNumber(
  offscreenSource,
  "managedFrameReleaseTimeoutMs"
);
const managedFrameWarmupGraceMs = sourceNumber(
  offscreenSource,
  "managedFrameWarmupGraceMs"
);

function flushAsync() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

const managedNonce = "12345678-1234-4123-8123-123456789abc";
const offscreenClientIdA =
  "offscreen-12345678-1234-4123-8123-123456789abc";
const offscreenClientIdB =
  "offscreen-87654321-4321-4123-8123-123456789abc";
const controlledDocumentAttribute = "data-sunox-managed-nonce";
const pageReadyAttribute = "data-sunox-page-ready";

function managedFrameHref(
  pageUrl,
  nonce = managedNonce,
  { clerkHandshake = null } = {}
) {
  const url = new URL(pageUrl);
  url.searchParams.set("__sunox_bridge", nonce);
  if (clerkHandshake !== null) {
    url.searchParams.set("__clerk_handshake", clerkHandshake);
  }
  url.hash = `sunox-browser-bridge=${nonce}`;
  return url.href;
}

function managedNetworkUrl(pageUrl, nonce = managedNonce, options = {}) {
  const url = new URL(managedFrameHref(pageUrl, nonce, options));
  url.hash = "";
  return url.href;
}

function nonceFromManagedHref(href) {
  const hash = new URL(href).hash;
  return hash.startsWith("#sunox-browser-bridge=")
    ? hash.slice("#sunox-browser-bridge=".length)
    : null;
}

function elementHarness(name) {
  const attributeValues = new Map();
  const children = [];
  const eventListeners = new Map();
  return {
    get attributes() {
      return [...attributeValues].map(([attributeName, value]) => ({
        name: attributeName,
        value
      }));
    },
    children,
    dataset: {},
    eventListeners,
    name,
    style: {},
    addEventListener(type, listener) {
      eventListeners.set(type, listener);
    },
    append(...nodes) {
      children.push(...nodes);
    },
    appendChild(child) {
      children.push(child);
      return child;
    },
    dispatchEvent(type) {
      eventListeners.get(type)?.();
    },
    getAttribute(attribute) {
      return attributeValues.get(attribute) ?? null;
    },
    getAttributeNames() {
      return [...attributeValues.keys()];
    },
    remove() {},
    removeAttribute(attribute) {
      attributeValues.delete(attribute);
    },
    removeEventListener(type, listener) {
      if (eventListeners.get(type) === listener) {
        eventListeners.delete(type);
      }
    },
    replaceChildren(...nodes) {
      children.splice(0, children.length, ...nodes);
    },
    setAttribute(attribute, value) {
      attributeValues.set(attribute, String(value));
    }
  };
}

function preparePageBridgeContext(context) {
  const nonce = nonceFromManagedHref(context.location.href);
  context.credentialless = true;
  context.document ??= {};
  const documentElement =
    context.document.documentElement ?? elementHarness("html");
  if (nonce) {
    documentElement.setAttribute(controlledDocumentAttribute, nonce);
  }
  context.document.documentElement = documentElement;
  return {
    documentElement,
    nonce
  };
}

function prepareContentBridgeContext(
  context,
  { credentialless = true, pageReady = true } = {}
) {
  const trace = [];
  const originalCreateElement = context.document?.createElement;
  const originalGetUrl = context.chrome?.runtime?.getURL;
  const originalSendMessage = context.chrome?.runtime?.sendMessage;
  context.credentialless = credentialless;
  context.stop = () => {
    trace.push("stop");
  };
  context.chrome.runtime.sendMessage = async (message) => {
    trace.push(`stage:${message?.stage ?? "unknown"}`);
    return await originalSendMessage?.call(context.chrome.runtime, message);
  };
  context.chrome.runtime.getURL = (path) => {
    trace.push(`getURL:${path}`);
    return originalGetUrl?.call(context.chrome.runtime, path)
      ?? `chrome-extension://abcdefghijklmnopabcdefghijklmnop/${path}`;
  };
  context.document ??= {};
  const documentElement =
    context.document.documentElement ?? elementHarness("html");
  context.document.documentElement = documentElement;
  const originalReplaceChildren = documentElement.replaceChildren.bind(
    documentElement
  );
  documentElement.replaceChildren = (...nodes) => {
    trace.push("root-replace");
    originalReplaceChildren(...nodes);
    const head = nodes.find((child) => child.name === "head");
    const body = nodes.find((child) => child.name === "body");
    if (head) context.document.head = head;
    if (body) context.document.body = body;
    if (head) {
      const appendChild = head.appendChild.bind(head);
      head.appendChild = (child) => {
        trace.push(`append:${child.name}`);
        const result = appendChild(child);
        if (pageReady && child.name === "script") {
          documentElement.setAttribute(
            pageReadyAttribute,
            nonceFromManagedHref(context.location.href)
          );
        }
        return result;
      };
    }
  };
  context.document.createElement = (name) => {
    trace.push(`create:${name}`);
    const element = originalCreateElement?.call(context.document, name)
      ?? elementHarness(name);
    if (name === "script") {
      let src = "";
      Object.defineProperty(element, "src", {
        configurable: true,
        get() {
          return src;
        },
        set(value) {
          src = value;
          trace.push(`src:${value}`);
        }
      });
    }
    return element;
  };
  return trace;
}

function expectedControlledDocumentCsp(provider, extensionId) {
  const extensionSource = `chrome-extension://${extensionId}`;
  const common = [
    "default-src 'none'",
    "object-src 'none'",
    "base-uri 'none'",
    "form-action 'none'",
    "manifest-src 'none'",
    "media-src 'none'",
    `frame-ancestors ${extensionSource}`
  ];
  if (provider === "turnstile") {
    return [
      ...common,
      `script-src ${extensionSource} https://challenges.cloudflare.com`,
      "connect-src https://challenges.cloudflare.com",
      "frame-src https://challenges.cloudflare.com",
      "img-src data: blob: https://challenges.cloudflare.com",
      "style-src 'unsafe-inline'",
      "worker-src blob: https://challenges.cloudflare.com"
    ].join("; ") + ";";
  }
  const hcaptcha = "https://hcaptcha.com https://*.hcaptcha.com";
  return [
    ...common,
    [
      "script-src",
      extensionSource,
      hcaptcha,
      "https://hcaptcha-endpoint-prod.suno.com",
      "https://hcaptcha-assets-prod.suno.com",
      "'wasm-unsafe-eval'"
    ].join(" "),
    [
      "connect-src",
      hcaptcha,
      "https://hcaptcha-endpoint-prod.suno.com",
      "https://hcaptcha-assets-prod.suno.com",
      "https://hcaptcha-imgs-prod.suno.com",
      "https://hcaptcha-reportapi-prod.suno.com"
    ].join(" "),
    [
      "frame-src",
      hcaptcha,
      "https://hcaptcha-endpoint-prod.suno.com",
      "https://hcaptcha-assets-prod.suno.com"
    ].join(" "),
    [
      "img-src",
      "data:",
      "blob:",
      hcaptcha,
      "https://hcaptcha-assets-prod.suno.com",
      "https://hcaptcha-imgs-prod.suno.com"
    ].join(" "),
    [
      "font-src",
      "data:",
      hcaptcha,
      "https://hcaptcha-assets-prod.suno.com"
    ].join(" "),
    [
      "style-src",
      "'unsafe-inline'",
      hcaptcha,
      "https://hcaptcha-assets-prod.suno.com"
    ].join(" "),
    `worker-src blob: ${hcaptcha}`
  ].join("; ") + ";";
}

const expectedPermissionsPolicy =
  "accelerometer=(), autoplay=(), camera=(), display-capture=(), "
  + "encrypted-media=(), fullscreen=(), geolocation=(), gyroscope=(), "
  + "magnetometer=(), microphone=(), midi=(), payment=(), "
  + "picture-in-picture=(), publickey-credentials-get=(), "
  + "screen-wake-lock=(), serial=(), usb=(), web-share=(), "
  + "xr-spatial-tracking=()";

function expectedManagedResponseHeaderActions(provider, extensionId) {
  return [{
    header: "content-security-policy",
    operation: "set",
    value: expectedControlledDocumentCsp(provider, extensionId)
  }, {
    header: "content-security-policy-report-only",
    operation: "remove"
  }, {
    header: "x-frame-options",
    operation: "remove"
  }, {
    header: "set-cookie",
    operation: "remove"
  }, {
    header: "clear-site-data",
    operation: "remove"
  }, {
    header: "cross-origin-embedder-policy",
    operation: "remove"
  }, {
    header: "cross-origin-opener-policy",
    operation: "remove"
  }, {
    header: "cross-origin-resource-policy",
    operation: "remove"
  }, {
    header: "refresh",
    operation: "remove"
  }, {
    header: "location",
    operation: "remove"
  }, {
    header: "link",
    operation: "remove"
  }, {
    header: "content-disposition",
    operation: "remove"
  }, {
    header: "www-authenticate",
    operation: "remove"
  }, {
    header: "proxy-authenticate",
    operation: "remove"
  }, {
    header: "report-to",
    operation: "remove"
  }, {
    header: "reporting-endpoints",
    operation: "remove"
  }, {
    header: "nel",
    operation: "remove"
  }, {
    header: "content-type",
    operation: "set",
    value: "text/html; charset=utf-8"
  }, {
    header: "x-content-type-options",
    operation: "set",
    value: "nosniff"
  }, {
    header: "referrer-policy",
    operation: "set",
    value: "strict-origin-when-cross-origin"
  }, {
    header: "permissions-policy",
    operation: "set",
    value: expectedPermissionsPolicy
  }, {
    header: "cache-control",
    operation: "set",
    value: "no-store"
  }, {
    header: "pragma",
    operation: "set",
    value: "no-cache"
  }, {
    header: "expires",
    operation: "set",
    value: "0"
  }];
}

const expectedManagedRequestHeaderActions = [
  { header: "cookie", operation: "remove" },
  { header: "authorization", operation: "remove" },
  { header: "if-modified-since", operation: "remove" },
  { header: "if-none-match", operation: "remove" },
  {
    header: "cache-control",
    operation: "set",
    value: "no-cache"
  },
  {
    header: "pragma",
    operation: "set",
    value: "no-cache"
  }
];

function observedManagedResponseHeaders(provider, extensionId) {
  return expectedManagedResponseHeaderActions(provider, extensionId)
    .filter((header) => header.operation === "set")
    .map((header) => ({
      name: header.header,
      value: header.value
    }));
}

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
        runtimeBuild,
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
  autoObserveManagedResponse = true,
  connectDuringRecoveryReload = false,
  connectBeforeCreateResolves = false,
  createError = null,
  deferInitialRuleCleanup = false,
  deferSessionRuleCall = null,
  displayInfo = defaultDisplayInfo,
  displayInfoError = null,
  emitBlankUpdateDuringBoundsCheck = false,
  emitInitializationEvents = false,
  existingWindowUrl = null,
  initialOffscreenContext = false,
  initialStoredState = undefined,
  existingSessionRules = [],
  fetchImpl = null,
  managedPageUrl = "https://suno.com/home/advanced",
  sessionRuleFailureCalls = [],
  storageSetError = null,
  windowGetError = null,
  windowGetErrorSequence = [],
  windowGetSequence = [],
  windowUpdateApplies = true,
  windowUpdateError = null
} = {}) {
  const managedUrl = managedFrameHref(managedPageUrl);
  const listeners = {};
  const calls = {
    consoleErrors: [],
    createdOffscreen: [],
    createdWindows: [],
    dynamicRules: [],
    fetches: [],
    notifications: [],
    offscreenCloses: 0,
    offscreenStarts: 0,
    removedTabs: [],
    removedWindows: [],
    reloadedTabs: [],
    sessionRules: [],
    storageRemoves: [],
    storageSets: [],
    updatedTabs: [],
    updatedWindows: [],
    webRequestFilters: []
  };
  const hardTimers = [];
  const safetyIntervals = [];
  const storageValues = initialStoredState === undefined
    ? {}
    : { sunoxManagedWindowV1: structuredClone(initialStoredState) };
  let currentDisplayInfo = structuredClone(displayInfo);
  let currentWindowBounds = structuredClone(actualWindowBounds);
  let currentWindowGetError = windowGetError;
  let activeProvider = "turnstile";
  const pendingWindowGetErrors = structuredClone(windowGetErrorSequence);
  const pendingWindowGetStates = structuredClone(windowGetSequence);
  let port;
  let recoveryReloadPort;
  let offscreenContexts = initialOffscreenContext
    ? [{
        contextType: "OFFSCREEN_DOCUMENT",
        documentId: "offscreen-owner-document",
        documentOrigin: "chrome-extension://abcdefghijklmnopabcdefghijklmnop",
        documentUrl:
          "chrome-extension://abcdefghijklmnopabcdefghijklmnop/offscreen.html",
        frameId: 0,
        tabId: -1,
        windowId: -1
      }]
    : [];
  let offscreenClientId = offscreenClientIdA;
  let releaseInitialRuleCleanup = () => {};
  let releaseDeferredSessionRuleCall = () => {};
  let nextContextLookupGate = null;
  const failedSessionRuleCalls = new Set(sessionRuleFailureCalls);
  const initialRuleCleanupGate = deferInitialRuleCleanup
    ? new Promise((resolve) => {
        releaseInitialRuleCleanup = resolve;
      })
    : null;
  const deferredSessionRuleGate = Number.isInteger(deferSessionRuleCall)
    ? new Promise((resolve) => {
        releaseDeferredSessionRuleCall = resolve;
      })
    : null;
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
        if (this.deferDisconnectNotification) {
          this.disconnectNotificationPending = true;
        } else {
          disconnectListener?.();
        }
      },
      deliverDisconnect() {
        if (!this.disconnectNotificationPending) return;
        this.disconnectNotificationPending = false;
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
        if (failedSessionRuleCalls.has(calls.sessionRules.length)) {
          throw new Error("simulated session-rule update failure");
        }
        if (
          initialRuleCleanupGate
          && calls.sessionRules.length === 1
          && !options.addRules
        ) {
          await initialRuleCleanupGate;
        }
        if (
          deferredSessionRuleGate
          && calls.sessionRules.length === deferSessionRuleCall
        ) {
          await deferredSessionRuleGate;
        }
      }
    },
    offscreen: {
      async closeDocument() {
        calls.offscreenCloses += 1;
        offscreenContexts = [];
      },
      async createDocument(options) {
        calls.createdOffscreen.push(options);
        offscreenContexts = [{
          contextType: "OFFSCREEN_DOCUMENT",
          documentId: "offscreen-owner-document",
          documentOrigin: "chrome-extension://abcdefghijklmnopabcdefghijklmnop",
          documentUrl:
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop/offscreen.html",
          frameId: 0,
          tabId: -1,
          windowId: -1
        }];
      }
    },
    runtime: {
      id: "abcdefghijklmnopabcdefghijklmnop",
      async getContexts() {
        if (nextContextLookupGate) {
          const gate = nextContextLookupGate;
          nextContextLookupGate = null;
          await gate.promise;
        }
        return structuredClone(offscreenContexts);
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
          calls.offscreenStarts += 1;
          return {
            accepted: true,
            clientId: offscreenClientId
          };
        }
        if (message.type === "sunox-offscreen-ping-v1") {
          return {
            busy: false,
            busySince: null,
            clientId: offscreenClientId,
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
    },
    webRequest: {
      onBeforeRedirect: {
        addListener(listener, filter, extraInfoSpec) {
          listeners.beforeRedirect = listener;
          calls.webRequestFilters.push({
            event: "onBeforeRedirect",
            extraInfoSpec: structuredClone(extraInfoSpec),
            filter: structuredClone(filter)
          });
        }
      },
      onBeforeRequest: {
        addListener(listener, filter, extraInfoSpec) {
          listeners.beforeRequest = listener;
          calls.webRequestFilters.push({
            event: "onBeforeRequest",
            extraInfoSpec: structuredClone(extraInfoSpec),
            filter: structuredClone(filter)
          });
        }
      },
      onErrorOccurred: {
        addListener(listener, filter, extraInfoSpec) {
          listeners.requestError = listener;
          calls.webRequestFilters.push({
            event: "onErrorOccurred",
            extraInfoSpec: structuredClone(extraInfoSpec),
            filter: structuredClone(filter)
          });
        }
      },
      onResponseStarted: {
        addListener(listener, filter, extraInfoSpec) {
          listeners.responseStarted = listener;
          calls.webRequestFilters.push({
            event: "onResponseStarted",
            extraInfoSpec: structuredClone(extraInfoSpec),
            filter: structuredClone(filter)
          });
        }
      },
      onSendHeaders: {
        addListener(listener, filter, extraInfoSpec) {
          listeners.sendHeaders = listener;
          calls.webRequestFilters.push({
            event: "onSendHeaders",
            extraInfoSpec: structuredClone(extraInfoSpec),
            filter: structuredClone(filter)
          });
        }
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
      },
      warn(...values) {
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
    async fetch(input, options) {
      calls.fetches.push({
        input,
        options: {
          cache: options?.cache,
          credentials: options?.credentials,
          method: options?.method,
          redirect: options?.redirect
        }
      });
      const implementation = fetchImpl || (async () => ({
        headers: new Headers({
          "content-type": "text/html; charset=utf-8"
        }),
        ok: true,
        status: 200,
        url: managedPageUrl
      }));
      return await implementation(input, options);
    },
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

  const dispatchFromSender = (message, sender) => new Promise((resolve) => {
    const keepChannel = listeners.runtimeMessage(message, sender, resolve);
    if (keepChannel !== true) queueMicrotask(() => resolve(undefined));
  });
  const dispatchFromOffscreen = (message, clientId = offscreenClientId) => {
    const deliveredMessage = {
      clientId,
      ...(message?.type === "sunox-frame-environment-prepare-v1"
        ? { provider: "turnstile" }
        : {}),
      ...message
    };
    if (deliveredMessage?.type === "sunox-frame-environment-prepare-v1") {
      activeProvider = deliveredMessage.provider;
    }
    const sender = {
      id: chrome.runtime.id,
      origin: `chrome-extension://${chrome.runtime.id}`,
      url: chrome.runtime.getURL("offscreen.html")
    };
    return dispatchFromSender(deliveredMessage, sender);
  };

  const managedNetworkDetails = ({
    documentId,
    frameId = 1,
    initiator = `chrome-extension://${chrome.runtime.id}`,
    parentDocumentId,
    parentFrameId = 0,
    requestId = "managed-request",
    url = managedUrl
  } = {}) => ({
    documentId,
    frameId,
    initiator,
    parentDocumentId:
      parentDocumentId ?? offscreenContexts[0]?.documentId,
    parentFrameId,
    requestId,
    tabId: chrome.tabs.TAB_ID_NONE,
    type: "sub_frame",
    url
  });

  const observeManagedResponse = ({
    fromCache = false,
    requestHeaders = [
      { name: "cache-control", value: "no-cache" },
      { name: "pragma", value: "no-cache" }
    ],
    responseHeaders = observedManagedResponseHeaders(
      activeProvider,
      chrome.runtime.id
    ),
    statusCode = 200,
    ...details
  } = {}) => {
    const common = managedNetworkDetails(details);
    listeners.beforeRequest?.(common);
    listeners.sendHeaders?.({
      ...common,
      requestHeaders
    });
    listeners.responseStarted?.({
      ...common,
      fromCache,
      responseHeaders,
      statusCode
    });
  };

  const observeManagedRedirect = (details = {}) => {
    const common = managedNetworkDetails(details);
    listeners.beforeRequest?.(common);
    listeners.beforeRedirect?.({
      ...common,
      redirectUrl: "https://suno.com/"
    });
  };

  const observeManagedError = (details = {}) => {
    const common = managedNetworkDetails(details);
    listeners.beforeRequest?.(common);
    listeners.requestError?.({
      ...common,
      error: "net::ERR_FAILED"
    });
  };

  return {
    calls,
    connect(sender, name) {
      const candidate = makePort(sender, name);
      if (
        autoObserveManagedResponse
        && name === "sunox-managed-frame-v2"
        && sender?.frameId !== 0
      ) {
        observeManagedResponse({
          url: sender.url
        });
      }
      listeners.connect(candidate);
      return candidate;
    },
    dispatchFromOffscreen,
    dispatchFromSender,
    dropOffscreenContext() {
      offscreenContexts = [];
      offscreenClientId = offscreenClientIdB;
    },
    deferNextContextLookup() {
      let release;
      const promise = new Promise((resolve) => {
        release = resolve;
      });
      nextContextLookupGate = { promise };
      return release;
    },
    replaceOffscreenContext(documentId, clientId = offscreenClientIdB) {
      offscreenClientId = clientId;
      offscreenContexts = [{
        contextType: "OFFSCREEN_DOCUMENT",
        documentId,
        documentOrigin: "chrome-extension://abcdefghijklmnopabcdefghijklmnop",
        documentUrl:
          "chrome-extension://abcdefghijklmnopabcdefghijklmnop/offscreen.html",
        frameId: 0,
        tabId: -1,
        windowId: -1
      }];
    },
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
    observeManagedError,
    observeManagedRedirect,
    observeManagedResponse,
    setTabUrl(url) {
      tabUrl = url;
    },
    port() {
      return port;
    },
    releaseInitialRuleCleanup,
    releaseDeferredSessionRuleCall,
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

test("bootstrap clears stale frame rules without resolving a managed route", async () => {
  const { calls } = popupServiceWorkerHarness();
  await flushAsync();
  await flushAsync();

  assert.deepEqual(JSON.parse(JSON.stringify(calls.dynamicRules)), [{
    removeRuleIds: [29_764, 29_765]
  }]);
  assert.deepEqual(JSON.parse(JSON.stringify(calls.sessionRules)), [{
    removeRuleIds: [29_764, 29_765]
  }]);
  assert.deepEqual(JSON.parse(JSON.stringify(calls.createdOffscreen)), [{
    url: "offscreen.html",
    reasons: ["IFRAME_SCRIPTING", "WORKERS"],
    justification:
      "Poll the local Sunox listener and run an invisible Suno challenge frame without creating a browser tab or window."
  }]);
  assert.equal(calls.offscreenStarts, 1);
  assert.deepEqual(calls.createdWindows, []);
});

test("a cold worker wake rebinds its live offscreen client before prepare", async () => {
  const harness = popupServiceWorkerHarness({
    deferInitialRuleCleanup: true,
    initialOffscreenContext: true
  });
  let preparationSettled = false;
  const preparation = harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-prepare-v1",
    nonce: managedNonce
  }).finally(() => {
    preparationSettled = true;
  });
  await flushAsync();
  await flushAsync();

  assert.equal(
    preparationSettled,
    false,
    "the wake-up message must bind its live client and wait for bootstrap cleanup"
  );
  assert.equal(harness.calls.fetches.length, 0);

  harness.releaseInitialRuleCleanup();
  assert.deepEqual(JSON.parse(JSON.stringify(await preparation)), {
    accepted: true,
    pageUrl: "https://suno.com/home/advanced"
  });
  assert.equal(harness.calls.fetches.length, 1);
});

test("owner binding rejects replacement between its ping and context lookup", async () => {
  const harness = popupServiceWorkerHarness({
    deferInitialRuleCleanup: true,
    initialOffscreenContext: true
  });
  const releaseOwnerLookup = harness.deferNextContextLookup();
  const preparation = harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-prepare-v1",
    nonce: managedNonce
  }, offscreenClientIdA);
  await flushAsync();

  harness.replaceOffscreenContext(
    "replacement-offscreen-owner-document",
    offscreenClientIdB
  );
  releaseOwnerLookup();
  assert.deepEqual(JSON.parse(JSON.stringify(await preparation)), {
    accepted: false,
    error: "challenge_environment_unavailable"
  });
  assert.equal(harness.calls.fetches.length, 0);

  harness.releaseInitialRuleCleanup();
  await flushAsync();
  await flushAsync();
});

test("challenge preparation waits for an in-flight stale-rule cleanup", async () => {
  const harness = popupServiceWorkerHarness({
    deferSessionRuleCall: 3
  });
  const nextNonce = "87654321-4321-4123-8123-123456789abc";
  await flushAsync();
  await flushAsync();
  await harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-prepare-v1",
    nonce: managedNonce
  });
  const release = harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-release-v1",
    nonce: managedNonce
  });
  await flushAsync();
  const preparation = harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-prepare-v1",
    nonce: nextNonce
  });
  await flushAsync();

  assert.equal(harness.calls.fetches.length, 1);
  assert.equal(harness.calls.sessionRules.length, 3);

  harness.releaseDeferredSessionRuleCall();
  assert.deepEqual(JSON.parse(JSON.stringify(await release)), {
    accepted: true
  });
  assert.deepEqual(JSON.parse(JSON.stringify(await preparation)), {
    accepted: true,
    pageUrl: "https://suno.com/home/advanced"
  });
  assert.equal(harness.calls.fetches.length, 2);
  assert.equal(harness.calls.sessionRules.length, 4);
  assert.equal(harness.calls.sessionRules[3].addRules.length, 2);
});

test("a timed-out prepare release waits through stale cleanup and late install", async () => {
  const harness = popupServiceWorkerHarness({
    deferSessionRuleCall: 3
  });
  const firstNonce = "87654321-4321-4123-8123-123456789abc";
  await flushAsync();
  await flushAsync();
  await harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-prepare-v1",
    nonce: managedNonce
  });
  const oldRelease = harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-release-v1",
    nonce: managedNonce
  });
  await flushAsync();
  let preparationSettled = false;
  let releaseSettled = false;
  const preparation = harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-prepare-v1",
    nonce: firstNonce
  }).finally(() => {
    preparationSettled = true;
  });
  await flushAsync();
  const release = harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-release-v1",
    nonce: firstNonce
  }).finally(() => {
    releaseSettled = true;
  });
  await flushAsync();

  assert.equal(preparationSettled, false);
  assert.equal(releaseSettled, false);
  assert.equal(harness.calls.fetches.length, 1);
  assert.equal(harness.calls.sessionRules.length, 3);

  harness.releaseDeferredSessionRuleCall();
  assert.deepEqual(JSON.parse(JSON.stringify(await oldRelease)), {
    accepted: true
  });
  assert.deepEqual(JSON.parse(JSON.stringify(await preparation)), {
    accepted: true,
    pageUrl: "https://suno.com/home/advanced"
  });
  assert.deepEqual(JSON.parse(JSON.stringify(await release)), {
    accepted: true
  });
  assert.equal(harness.calls.fetches.length, 2);
  assert.equal(harness.calls.sessionRules.length, 5);
  assert.equal(harness.calls.sessionRules[3].addRules.length, 2);
  assert.deepEqual(
    JSON.parse(JSON.stringify(harness.calls.sessionRules[4])),
    { removeRuleIds: [29_764, 29_765] }
  );

  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: managedNonce
    })
  )), {
    accepted: true,
    pageUrl: "https://suno.com/home/advanced"
  });
  assert.equal(
    harness.calls.fetches.length,
    3,
    "the timed-out nonce must not remain active or block a fresh prepare"
  );
});

test("challenge preparation follows a clean same-origin redirect and binds the exact final route", async () => {
  const managedPageUrl = "https://suno.com/create/v3";
  const harness = popupServiceWorkerHarness({
    fetchImpl: async () => ({
      headers: new Headers({
        "content-type": "text/html; charset=utf-8"
      }),
      ok: true,
      status: 200,
      url: managedPageUrl
    }),
    managedPageUrl
  });
  await flushAsync();
  await flushAsync();

  const preparation = await harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-prepare-v1",
    nonce: managedNonce
  });
  assert.deepEqual(JSON.parse(JSON.stringify(preparation)), {
    accepted: true,
    pageUrl: managedPageUrl
  });
  assert.deepEqual(harness.calls.fetches, [{
    input: "https://suno.com/",
    options: {
      cache: "no-store",
      credentials: "omit",
      method: "GET",
      redirect: "follow"
    }
  }]);
  const update = JSON.parse(JSON.stringify(harness.calls.sessionRules.at(-1)));
  const sunoRule = update.addRules.find((rule) => rule.id === 29_764);
  const cookieRule = update.addRules.find((rule) => rule.id === 29_765);
  assert.ok(sunoRule.condition.regexFilter.includes("__sunox_bridge"));
  assert.equal(sunoRule.priority, 1_000);
  assert.equal(sunoRule.condition.isUrlFilterCaseSensitive, true);
  assert.equal("responseHeaders" in sunoRule.condition, false);
  const webRequestFilter = {
    types: ["sub_frame"],
    urls: ["https://suno.com/*"]
  };
  assert.deepEqual(harness.calls.webRequestFilters, [{
    event: "onBeforeRequest",
    extraInfoSpec: undefined,
    filter: webRequestFilter
  }, {
    event: "onSendHeaders",
    extraInfoSpec: ["requestHeaders", "extraHeaders"],
    filter: webRequestFilter
  }, {
    event: "onBeforeRedirect",
    extraInfoSpec: undefined,
    filter: webRequestFilter
  }, {
    event: "onResponseStarted",
    extraInfoSpec: ["responseHeaders", "extraHeaders"],
    filter: webRequestFilter
  }, {
    event: "onErrorOccurred",
    extraInfoSpec: undefined,
    filter: webRequestFilter
  }]);
  assert.deepEqual(
    sunoRule.action.responseHeaders,
    expectedManagedResponseHeaderActions(
      "turnstile",
      "abcdefghijklmnopabcdefghijklmnop"
    )
  );
  assert.deepEqual(
    cookieRule.action.requestHeaders,
    expectedManagedRequestHeaderActions
  );
  assert.equal(cookieRule.priority, 1_000);
  assert.deepEqual(cookieRule.condition, {
    regexFilter: sunoRule.condition.regexFilter,
    isUrlFilterCaseSensitive: true,
    initiatorDomains: ["abcdefghijklmnopabcdefghijklmnop"],
    requestMethods: ["get"],
    resourceTypes: ["sub_frame"],
    tabIds: [-1]
  });
  const exactRoute = new RegExp(sunoRule.condition.regexFilter);
  assert.equal(exactRoute.test(managedPageUrl), false);
  assert.equal(
    exactRoute.test(
      `${managedPageUrl}?__sunox_bridge=${managedNonce}`
    ),
    true
  );
  assert.equal(
    exactRoute.test(
      `${managedPageUrl}?__sunox_bridge=${managedNonce}`
      + "&__clerk_handshake=opaque-return"
    ),
    false
  );
  assert.equal(
    exactRoute.test(
      `${managedPageUrl}/child?__sunox_bridge=${managedNonce}`
    ),
    false
  );
  assert.equal(
    exactRoute.test(
      `${managedPageUrl}?__sunox_bridge=${managedNonce}&unexpected=1`
    ),
    false
  );
  assert.equal(
    exactRoute.test(
      `${managedPageUrl}?__sunox_bridge=${managedNonce}`
      + "&__clerk_handshake=one&unexpected=two"
    ),
    false
  );
  assert.equal(
    exactRoute.test(
      `${managedPageUrl}?__sunox_bridge=87654321-4321-4123-8123-123456789abc`
    ),
    false
  );

  const wrongRoute = harness.connect({
    documentId: "stale-route-document",
    frameId: 1,
    id: "abcdefghijklmnopabcdefghijklmnop",
    origin: "https://suno.com",
    url: managedFrameHref("https://suno.com/home/advanced")
  }, "sunox-managed-frame-v2");
  await flushAsync();
  assert.equal(wrongRoute.disconnected, true);
  assert.deepEqual(
    JSON.parse(JSON.stringify(harness.calls.notifications.at(-1))),
    {
      type: "sunox-managed-frame-diagnostic-v1",
      nonce: managedNonce,
      reason: "managed_route_mismatch"
    }
  );

  const accepted = harness.connect({
    documentId: "dynamic-route-document",
    frameId: 1,
    id: "abcdefghijklmnopabcdefghijklmnop",
    origin: "https://suno.com",
    url: harness.managedUrl
  }, "sunox-managed-frame-v2");
  assert.equal(accepted.disconnected, false);
  accepted.disconnect();

  const release = await harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-release-v1",
    nonce: managedNonce
  });
  assert.deepEqual(JSON.parse(JSON.stringify(release)), { accepted: true });
  assert.deepEqual(
    JSON.parse(JSON.stringify(harness.calls.sessionRules.at(-1))),
    { removeRuleIds: [29_764, 29_765] }
  );
});

test("a managed network redirect is a sticky fail-closed condition", async () => {
  const harness = popupServiceWorkerHarness({
    autoObserveManagedResponse: false
  });
  await flushAsync();
  await flushAsync();
  await harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-prepare-v1",
    nonce: managedNonce
  });
  harness.observeManagedRedirect();
  harness.observeManagedResponse();

  const port = harness.connect({
    documentId: "redirected-frame-document",
    frameId: 1,
    id: "abcdefghijklmnopabcdefghijklmnop",
    origin: "https://suno.com",
    url: harness.managedUrl
  }, "sunox-managed-frame-v2");
  await flushAsync();

  assert.equal(port.disconnected, true);
  assert.deepEqual(
    JSON.parse(JSON.stringify(harness.calls.notifications.at(-1))),
    {
      type: "sunox-managed-frame-diagnostic-v1",
      nonce: managedNonce,
      reason: "managed_redirect_rejected"
    }
  );
});

test("extra managed-route query parameters cannot authorize a frame", async () => {
  for (const query of [
    `__sunox_bridge=${managedNonce}`
      + "&__clerk_handshake=one&__clerk_handshake=two",
    `__sunox_bridge=${managedNonce}`
      + "&__clerk_handshake=one&unexpected=two"
  ]) {
    const harness = popupServiceWorkerHarness({
      autoObserveManagedResponse: false
    });
    await flushAsync();
    await flushAsync();
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: managedNonce
    });
    harness.observeManagedResponse({
      url:
        `https://suno.com/home/advanced?${query}`
        + `#sunox-browser-bridge=${managedNonce}`
    });

    const port = harness.connect({
      documentId: "ambiguous-clerk-return-frame-document",
      frameId: 1,
      id: "abcdefghijklmnopabcdefghijklmnop",
      origin: "https://suno.com",
      url: harness.managedUrl
    }, "sunox-managed-frame-v2");
    await flushAsync();

    assert.equal(port.disconnected, true);
    assert.equal(
      harness.calls.notifications.at(-1)?.reason,
      "managed_request_unverified"
    );
  }
});

test("cold retry rotates the one-time reservation without broadening its route", async () => {
  const harness = popupServiceWorkerHarness();
  const firstNonce = managedNonce;
  const secondNonce = "87654321-4321-4123-8123-123456789abc";
  await flushAsync();
  await flushAsync();

  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: firstNonce,
      previousNonce: null
    })
  )), {
    accepted: true,
    pageUrl: "https://suno.com/home/advanced"
  });
  const firstRuleUpdate = JSON.parse(JSON.stringify(
    harness.calls.sessionRules.at(-1)
  ));
  const firstRegex = new RegExp(
    firstRuleUpdate.addRules.find((rule) => rule.id === 29_764)
      .condition.regexFilter
  );
  assert.equal(
    firstRegex.test(
      managedNetworkUrl("https://suno.com/home/advanced", firstNonce)
    ),
    true
  );
  assert.equal(
    firstRegex.test(
      managedNetworkUrl("https://suno.com/home/advanced", secondNonce)
    ),
    false
  );
  const dynamicRuleCallCountBeforeRetry = harness.calls.dynamicRules.length;
  const sessionRuleCallCountBeforeRetry = harness.calls.sessionRules.length;

  const firstPort = harness.connect({
    documentId: "first-retry-document",
    frameId: 1,
    id: "abcdefghijklmnopabcdefghijklmnop",
    origin: "https://suno.com",
    url: managedFrameHref("https://suno.com/home/advanced", firstNonce)
  }, "sunox-managed-frame-v2");
  firstPort.deferDisconnectNotification = true;
  assert.equal(firstPort.disconnected, false);
  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-retire-v1",
      nonce: firstNonce
    })
  )), { accepted: true });

  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: secondNonce,
      previousNonce: firstNonce
    })
  )), {
    accepted: true,
    pageUrl: "https://suno.com/home/advanced"
  });
  assert.equal(
    harness.calls.sessionRules.length,
    sessionRuleCallCountBeforeRetry + 1
  );
  assert.equal(
    harness.calls.dynamicRules.length,
    dynamicRuleCallCountBeforeRetry
  );
  const rotatedRuleUpdate = JSON.parse(JSON.stringify(
    harness.calls.sessionRules.at(-1)
  ));
  assert.deepEqual(rotatedRuleUpdate.removeRuleIds, [29_764, 29_765]);
  assert.equal(rotatedRuleUpdate.addRules.length, 2);
  const rotatedRegex = new RegExp(
    rotatedRuleUpdate.addRules.find((rule) => rule.id === 29_764)
      .condition.regexFilter
  );
  assert.equal(
    rotatedRegex.test(
      managedNetworkUrl("https://suno.com/home/advanced", firstNonce)
    ),
    false
  );
  assert.equal(
    rotatedRegex.test(
      managedNetworkUrl("https://suno.com/home/advanced", secondNonce)
    ),
    true
  );
  assert.equal(firstPort.disconnected, true);
  assert.equal(firstPort.disconnectNotificationPending, true);
  firstPort.deliverDisconnect();

  const stalePort = harness.connect({
    documentId: "stale-retry-document",
    frameId: 1,
    id: "abcdefghijklmnopabcdefghijklmnop",
    origin: "https://suno.com",
    url: managedFrameHref("https://suno.com/home/advanced", firstNonce)
  }, "sunox-managed-frame-v2");
  assert.equal(stalePort.disconnected, true);
  assert.deepEqual(
    JSON.parse(JSON.stringify(harness.calls.notifications.at(-1))),
    {
      type: "sunox-managed-frame-diagnostic-v1",
      nonce: firstNonce,
      reason: "managed_nonce_mismatch"
    }
  );

  const currentPort = harness.connect({
    documentId: "current-retry-document",
    frameId: 1,
    id: "abcdefghijklmnopabcdefghijklmnop",
    origin: "https://suno.com",
    url: managedFrameHref("https://suno.com/home/advanced", secondNonce)
  }, "sunox-managed-frame-v2");
  assert.equal(currentPort.disconnected, false);
  currentPort.disconnect();
});

test("a foreign release cannot disturb a pending nonce rotation", async () => {
  const firstNonce = managedNonce;
  const rotatingNonce = "87654321-4321-4123-8123-123456789abc";
  const foreignNonce = "11111111-1111-4111-8111-111111111111";
  const freshNonce = "22222222-2222-4222-8222-222222222222";
  const harness = popupServiceWorkerHarness({
    // 1 = bootstrap cleanup, 2 = first install, 3 = deferred rotation.
    deferSessionRuleCall: 3
  });
  await flushAsync();
  await flushAsync();

  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: firstNonce
    })
  )), {
    accepted: true,
    pageUrl: "https://suno.com/home/advanced"
  });
  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-retire-v1",
      nonce: firstNonce
    })
  )), { accepted: true });

  let rotationSettled = false;
  const rotation = harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-prepare-v1",
    nonce: rotatingNonce,
    previousNonce: firstNonce
  }).finally(() => {
    rotationSettled = true;
  });
  await flushAsync();
  assert.equal(rotationSettled, false);
  assert.equal(harness.calls.sessionRules.length, 3);

  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: foreignNonce
    })
  )), {
    accepted: false,
    error: "challenge_environment_unavailable"
  });
  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-release-v1",
      nonce: foreignNonce
    })
  )), { accepted: false });
  assert.equal(rotationSettled, false);
  assert.equal(
    harness.calls.sessionRules.length,
    3,
    "a foreign finally-release must not start cleanup during A2 rotation"
  );

  harness.releaseDeferredSessionRuleCall();
  assert.deepEqual(JSON.parse(JSON.stringify(await rotation)), {
    accepted: true,
    pageUrl: "https://suno.com/home/advanced"
  });
  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-release-v1",
      nonce: rotatingNonce
    })
  )), { accepted: true });
  assert.deepEqual(
    JSON.parse(JSON.stringify(harness.calls.sessionRules.at(-1))),
    { removeRuleIds: [29_764, 29_765] }
  );

  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: freshNonce
    })
  )), {
    accepted: true,
    pageUrl: "https://suno.com/home/advanced"
  });
  assert.equal(
    harness.calls.fetches.length,
    2,
    "A2 release must leave no reservation that blocks fresh C1"
  );
});

test("cold retry preserves the provider-specific controlled-document policy", async () => {
  const harness = popupServiceWorkerHarness();
  const firstNonce = managedNonce;
  const secondNonce = "87654321-4321-4123-8123-123456789abc";
  await flushAsync();
  await flushAsync();

  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: firstNonce,
      previousNonce: null,
      provider: "hcaptcha"
    })
  )), {
    accepted: true,
    pageUrl: "https://suno.com/home/advanced"
  });

  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-retire-v1",
      nonce: firstNonce
    })
  )), { accepted: true });
  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: secondNonce,
      previousNonce: firstNonce,
      provider: "hcaptcha"
    })
  )), {
    accepted: true,
    pageUrl: "https://suno.com/home/advanced"
  });
  const rotatedRule = JSON.parse(JSON.stringify(
    harness.calls.sessionRules.at(-1).addRules.find(
      (rule) => rule.id === 29_764
    )
  ));
  assert.equal("responseHeaders" in rotatedRule.condition, false);
  assert.deepEqual(
    rotatedRule.action.responseHeaders,
    expectedManagedResponseHeaderActions(
      "hcaptcha",
      "abcdefghijklmnopabcdefghijklmnop"
    )
  );
  const csp = rotatedRule.action.responseHeaders[0].value;
  assert.ok(csp.includes("https://hcaptcha.com https://*.hcaptcha.com"));
  assert.equal(csp.includes("https://*.suno.com"), false);
});

test("cold retry fails closed after ambiguous controlled response headers", async () => {
  const harness = popupServiceWorkerHarness();
  const secondNonce = "87654321-4321-4123-8123-123456789abc";
  await flushAsync();
  await flushAsync();

  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: managedNonce,
      previousNonce: null
    })
  )), {
    accepted: true,
    pageUrl: "https://suno.com/home/advanced"
  });
  const callsBeforeRetry = harness.calls.sessionRules.length;
  const responseHeaders = observedManagedResponseHeaders(
    "turnstile",
    "abcdefghijklmnopabcdefghijklmnop"
  );
  harness.observeManagedResponse({
    responseHeaders: [
      ...responseHeaders,
      {
        name: "content-security-policy",
        value: "script-src 'none'"
      }
    ]
  });

  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: secondNonce,
      previousNonce: managedNonce
    })
  )), {
    accepted: false,
    error: "challenge_environment_unavailable"
  });
  assert.equal(harness.calls.sessionRules.length, callsBeforeRetry);
});

test("managed frame cannot connect before Chrome verifies its network lifecycle", async () => {
  const harness = popupServiceWorkerHarness({
    autoObserveManagedResponse: false
  });
  await flushAsync();
  await flushAsync();
  await harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-prepare-v1",
    nonce: managedNonce
  });

  const port = harness.connect({
    documentId: "unobserved-frame-document",
    frameId: 1,
    id: "abcdefghijklmnopabcdefghijklmnop",
    origin: "https://suno.com",
    url: harness.managedUrl
  }, "sunox-managed-frame-v2");
  await flushAsync();

  assert.equal(port.disconnected, true);
  assert.deepEqual(
    JSON.parse(JSON.stringify(harness.calls.notifications.at(-1))),
    {
      type: "sunox-managed-frame-diagnostic-v1",
      nonce: managedNonce,
      reason: "managed_request_unverified"
    }
  );
  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-managed-frame-execute-v2",
      nonce: managedNonce,
      provider: "turnstile",
      requestId: "request-without-policy"
    })
  )), { accepted: false });
});

test("ambiguous response headers permanently reject the managed frame", async () => {
  const harness = popupServiceWorkerHarness({
    autoObserveManagedResponse: false
  });
  await flushAsync();
  await flushAsync();
  await harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-prepare-v1",
    nonce: managedNonce
  });
  const responseHeaders = observedManagedResponseHeaders(
    "turnstile",
    "abcdefghijklmnopabcdefghijklmnop"
  );
  harness.observeManagedResponse({
    responseHeaders: [
      ...responseHeaders,
      {
        name: "content-security-policy",
        value: "script-src 'none'"
      }
    ]
  });
  harness.observeManagedResponse();

  const port = harness.connect({
    documentId: "multiple-policy-frame-document",
    frameId: 1,
    id: "abcdefghijklmnopabcdefghijklmnop",
    origin: "https://suno.com",
    url: harness.managedUrl
  }, "sunox-managed-frame-v2");
  await flushAsync();

  assert.equal(port.disconnected, true);
  assert.deepEqual(
    JSON.parse(JSON.stringify(harness.calls.notifications.at(-1))),
    {
      type: "sunox-managed-frame-diagnostic-v1",
      nonce: managedNonce,
      reason: "managed_response_csp_invalid"
    }
  );
});

test("managed network verification rejects leaked credentials, cache hits, and side-effect headers", async () => {
  const extensionId = "abcdefghijklmnopabcdefghijklmnop";
  const cases = [{
    name: "request credentials",
    observe(harness) {
      harness.observeManagedResponse({
        requestHeaders: [
          { name: "cache-control", value: "no-cache" },
          { name: "pragma", value: "no-cache" },
          { name: "cookie", value: "session=must-not-reach-suno" }
        ]
      });
    },
    reason: "managed_request_credentials_present"
  }, {
    name: "cached response",
    observe(harness) {
      harness.observeManagedResponse({ fromCache: true });
    },
    reason: "managed_response_cache_unsafe"
  }, {
    name: "response side effect",
    observe(harness) {
      harness.observeManagedResponse({
        responseHeaders: [
          ...observedManagedResponseHeaders("turnstile", extensionId),
          { name: "set-cookie", value: "session=must-not-be-stored" }
        ]
      });
    },
    reason: "managed_response_side_effect_header"
  }, {
    name: "network error",
    observe(harness) {
      harness.observeManagedError();
    },
    reason: "managed_network_error"
  }];

  for (const candidate of cases) {
    const harness = popupServiceWorkerHarness({
      autoObserveManagedResponse: false
    });
    await flushAsync();
    await flushAsync();
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: managedNonce
    });
    candidate.observe(harness);

    const port = harness.connect({
      documentId: `rejected-${candidate.name}`,
      frameId: 1,
      id: extensionId,
      origin: "https://suno.com",
      url: harness.managedUrl
    }, "sunox-managed-frame-v2");
    await flushAsync();

    assert.equal(port.disconnected, true, candidate.name);
    assert.equal(
      harness.calls.notifications.at(-1)?.reason,
      candidate.reason,
      candidate.name
    );
  }
});

test("multiple request identities permanently reject the managed frame", async () => {
  const harness = popupServiceWorkerHarness({
    autoObserveManagedResponse: false
  });
  await flushAsync();
  await flushAsync();
  await harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-prepare-v1",
    nonce: managedNonce
  });
  harness.observeManagedResponse();
  harness.observeManagedResponse({
    requestId: "second-managed-request"
  });
  harness.observeManagedResponse();

  const port = harness.connect({
    documentId: "changing-policy-frame-document",
    frameId: 1,
    id: "abcdefghijklmnopabcdefghijklmnop",
    origin: "https://suno.com",
    url: harness.managedUrl
  }, "sunox-managed-frame-v2");
  await flushAsync();

  assert.equal(port.disconnected, true);
  assert.equal(
    harness.calls.notifications.at(-1)?.reason,
    "multiple_managed_requests"
  );
});

test("nested frame observations cannot authorize the managed first-level frame", async () => {
  const harness = popupServiceWorkerHarness({
    autoObserveManagedResponse: false
  });
  await flushAsync();
  await flushAsync();
  await harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-prepare-v1",
    nonce: managedNonce
  });
  harness.observeManagedResponse({ parentFrameId: 1 });

  const port = harness.connect({
    documentId: "nested-policy-frame-document",
    frameId: 1,
    id: "abcdefghijklmnopabcdefghijklmnop",
    origin: "https://suno.com",
    url: harness.managedUrl
  }, "sunox-managed-frame-v2");
  await flushAsync();

  assert.equal(port.disconnected, true);
  assert.equal(
    harness.calls.notifications.at(-1)?.reason,
    "managed_parent_frame_mismatch"
  );
});

test("a retired old-frame error cannot poison the rotated reservation", async () => {
  const firstNonce = managedNonce;
  const secondNonce = "87654321-4321-4123-8123-123456789abc";
  const firstUrl = managedFrameHref(
    "https://suno.com/home/advanced",
    firstNonce
  );
  const secondUrl = managedFrameHref(
    "https://suno.com/home/advanced",
    secondNonce
  );
  const harness = popupServiceWorkerHarness({
    autoObserveManagedResponse: false
  });
  await flushAsync();
  await flushAsync();
  await harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-prepare-v1",
    nonce: firstNonce
  });
  harness.observeManagedResponse({ url: firstUrl });
  const firstPort = harness.connect({
    documentId: "old-retiring-frame-document",
    frameId: 1,
    id: "abcdefghijklmnopabcdefghijklmnop",
    origin: "https://suno.com",
    url: firstUrl
  }, "sunox-managed-frame-v2");
  await flushAsync();
  assert.equal(firstPort.disconnected, false);
  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-retire-v1",
      nonce: firstNonce
    })
  )), { accepted: true });
  assert.equal(firstPort.disconnected, true);
  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-managed-frame-execute-v2",
      nonce: firstNonce,
      provider: "turnstile",
      requestId: "late-execute-after-retire"
    })
  )), { accepted: false });
  assert.deepEqual(firstPort.messages, []);
  const notificationCountAfterRetire = harness.calls.notifications.length;
  harness.observeManagedError({ url: firstUrl });
  assert.equal(
    harness.calls.notifications.length,
    notificationCountAfterRetire,
    "a late old onErrorOccurred must be ignored after retire ACK"
  );
  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: secondNonce,
      previousNonce: firstNonce
    })
  )), {
    accepted: true,
    pageUrl: "https://suno.com/home/advanced"
  });
  harness.observeManagedResponse({ url: secondUrl });

  const port = harness.connect({
    documentId: "rotated-policy-frame-document",
    frameId: 1,
    id: "abcdefghijklmnopabcdefghijklmnop",
    origin: "https://suno.com",
    url: secondUrl
  }, "sunox-managed-frame-v2");
  await flushAsync();

  assert.equal(port.disconnected, false);
  assert.deepEqual(
    JSON.parse(JSON.stringify(harness.calls.notifications.at(-1))),
    {
      type: "sunox-managed-frame-ready-v2",
      nonce: secondNonce
    }
  );
});

test("a failed cold-retry rule rotation preserves old rules but retires the old reservation", async () => {
  const firstNonce = managedNonce;
  const secondNonce = "87654321-4321-4123-8123-123456789abc";
  const harness = popupServiceWorkerHarness({
    // 1 = bootstrap cleanup, 2 = fresh install, 3 = atomic retry rotation.
    sessionRuleFailureCalls: [3]
  });
  await flushAsync();
  await flushAsync();

  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: firstNonce,
      previousNonce: null
    })
  )), {
    accepted: true,
    pageUrl: "https://suno.com/home/advanced"
  });
  const firstRuleUpdate = JSON.parse(JSON.stringify(
    harness.calls.sessionRules.at(-1)
  ));

  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-retire-v1",
      nonce: firstNonce
    })
  )), { accepted: true });
  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: secondNonce,
      previousNonce: firstNonce
    })
  )), {
    accepted: false,
    error: "challenge_environment_unavailable"
  });
  const failedRotation = JSON.parse(JSON.stringify(
    harness.calls.sessionRules.at(-1)
  ));
  assert.deepEqual(failedRotation.removeRuleIds, [29_764, 29_765]);
  assert.equal(failedRotation.addRules.length, 2);
  assert.notDeepEqual(failedRotation.addRules, firstRuleUpdate.addRules);

  const previousPort = harness.connect({
    documentId: "preserved-reservation-document",
    frameId: 1,
    id: "abcdefghijklmnopabcdefghijklmnop",
    origin: "https://suno.com",
    url: managedFrameHref("https://suno.com/home/advanced", firstNonce)
  }, "sunox-managed-frame-v2");
  assert.equal(previousPort.disconnected, true);
  assert.equal(
    harness.calls.notifications.at(-1)?.reason,
    "managed_environment_retired"
  );

  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-release-v1",
      nonce: firstNonce
    })
  )), { accepted: true });
  assert.deepEqual(
    JSON.parse(JSON.stringify(harness.calls.sessionRules.at(-1))),
    { removeRuleIds: [29_764, 29_765] }
  );
});

test("a failed terminal rule cleanup is retried by the next poll alarm", async () => {
  const harness = popupServiceWorkerHarness({
    // 1 = bootstrap cleanup, 2 = install, 3 = terminal cleanup.
    sessionRuleFailureCalls: [3]
  });
  await flushAsync();
  await flushAsync();

  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: managedNonce
    })
  )), {
    accepted: true,
    pageUrl: "https://suno.com/home/advanced"
  });
  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-release-v1",
      nonce: managedNonce
    })
  )), { accepted: false });

  harness.listeners.alarm({ name: "sunox-bridge-poll" });
  await flushAsync();
  await flushAsync();
  await flushAsync();

  assert.deepEqual(
    JSON.parse(JSON.stringify(harness.calls.sessionRules.at(-1))),
    { removeRuleIds: [29_764, 29_765] }
  );
  assert.equal(harness.calls.sessionRules.length, 4);
  assert.equal(harness.calls.offscreenStarts, 2);
});

test("terminal release retires a delayed old port before the next reservation", async () => {
  const harness = popupServiceWorkerHarness();
  const nextNonce = "87654321-4321-4123-8123-123456789abc";
  await flushAsync();
  await flushAsync();

  await harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-prepare-v1",
    nonce: managedNonce
  });
  const oldPort = harness.connect({
    documentId: "terminal-release-old-document",
    frameId: 1,
    id: "abcdefghijklmnopabcdefghijklmnop",
    origin: "https://suno.com",
    url: harness.managedUrl
  }, "sunox-managed-frame-v2");
  oldPort.deferDisconnectNotification = true;

  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-release-v1",
      nonce: managedNonce
    })
  )), { accepted: true });
  assert.equal(oldPort.disconnected, true);
  assert.equal(oldPort.disconnectNotificationPending, true);

  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: nextNonce
    })
  )), {
    accepted: true,
    pageUrl: "https://suno.com/home/advanced"
  });
  const currentPort = harness.connect({
    documentId: "terminal-release-current-document",
    frameId: 1,
    id: "abcdefghijklmnopabcdefghijklmnop",
    origin: "https://suno.com",
    url: managedFrameHref("https://suno.com/home/advanced", nextNonce)
  }, "sunox-managed-frame-v2");
  assert.equal(currentPort.disconnected, false);

  oldPort.deliverDisconnect();
  assert.equal(currentPort.disconnected, false);
  currentPort.disconnect();
});

test("release of a timed-out new nonce cleans its retired previous owner", async () => {
  const harness = popupServiceWorkerHarness();
  const firstNonce = managedNonce;
  const secondNonce = "87654321-4321-4123-8123-123456789abc";
  await flushAsync();
  await flushAsync();

  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: firstNonce
    })
  )), {
    accepted: true,
    pageUrl: "https://suno.com/home/advanced"
  });
  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-retire-v1",
      nonce: firstNonce
    })
  )), { accepted: true });
  const cleanupCallsBeforeRelease = harness.calls.sessionRules.length;

  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-release-v1",
      nonce: secondNonce
    })
  )), { accepted: true });
  assert.equal(
    harness.calls.sessionRules.length,
    cleanupCallsBeforeRelease + 1
  );
  assert.deepEqual(
    JSON.parse(JSON.stringify(harness.calls.sessionRules.at(-1))),
    { removeRuleIds: [29_764, 29_765] }
  );

  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-release-v1",
      nonce: firstNonce
    })
  )), { accepted: true });
  assert.equal(
    harness.calls.sessionRules.length,
    cleanupCallsBeforeRelease + 1,
    "release(secondNonce) must already clean the retired previous owner"
  );
});

test("a missing offscreen owner revokes its reservation before replacement", async () => {
  const harness = popupServiceWorkerHarness();
  const replacementNonce = "87654321-4321-4123-8123-123456789abc";
  await flushAsync();
  await flushAsync();

  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: managedNonce
    })
  )), {
    accepted: true,
    pageUrl: "https://suno.com/home/advanced"
  });

  // Simulate Chrome destroying the offscreen document after preparation but
  // before its iframe can bind a port or send the terminal release message.
  harness.dropOffscreenContext();
  harness.listeners.alarm({ name: "sunox-bridge-poll" });
  await flushAsync();
  await flushAsync();
  await flushAsync();

  assert.deepEqual(
    JSON.parse(JSON.stringify(harness.calls.sessionRules.at(-1))),
    { removeRuleIds: [29_764, 29_765] }
  );
  assert.equal(harness.calls.createdOffscreen.length, 2);

  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: replacementNonce
    })
  )), {
    accepted: true,
    pageUrl: "https://suno.com/home/advanced"
  });
  assert.equal(harness.calls.fetches.length, 2);
});

test("an offscreen owner replacement cleans a late rule commit before the new owner prepares", async () => {
  const ownerA = "offscreen-owner-document";
  const ownerB = "replacement-offscreen-owner-document";
  const staleNonce = "87654321-4321-4123-8123-123456789abc";
  const harness = popupServiceWorkerHarness({
    deferSessionRuleCall: 2
  });
  await flushAsync();
  await flushAsync();

  const stalePreparation = harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-prepare-v1",
    nonce: staleNonce
  }, offscreenClientIdA);
  await flushAsync();
  assert.equal(
    harness.calls.sessionRules.length,
    2,
    "the stale owner must be paused inside its rule commit"
  );

  harness.replaceOffscreenContext(ownerB);
  harness.releaseDeferredSessionRuleCall();
  assert.deepEqual(JSON.parse(JSON.stringify(await stalePreparation)), {
    accepted: false,
    error: "challenge_environment_unavailable"
  });
  assert.deepEqual(
    JSON.parse(JSON.stringify(harness.calls.sessionRules.at(-1))),
    { removeRuleIds: [29_764, 29_765] },
    "a late stale-owner commit must be removed before another prepare"
  );

  harness.listeners.alarm({ name: "sunox-bridge-poll" });
  await flushAsync();
  await flushAsync();
  await flushAsync();
  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: managedNonce
    }, offscreenClientIdB)
  )), {
    accepted: true,
    pageUrl: "https://suno.com/home/advanced"
  });

  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-retire-v1",
    nonce: staleNonce
    }, offscreenClientIdA)
  )), { accepted: false });
  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-release-v1",
    nonce: staleNonce
    }, offscreenClientIdA)
  )), { accepted: false });

  const port = harness.connect({
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

test("a replacement offscreen client cannot act until the worker rebinds it", async () => {
  const replacementOwner = "replacement-offscreen-owner-document";
  const replacementNonce = "87654321-4321-4123-8123-123456789abc";
  const harness = popupServiceWorkerHarness();
  await flushAsync();
  await flushAsync();

  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: managedNonce
    }, offscreenClientIdA)
  )), {
    accepted: true,
    pageUrl: "https://suno.com/home/advanced"
  });

  harness.replaceOffscreenContext(replacementOwner, offscreenClientIdB);
  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: replacementNonce,
      ownerDocumentId: "attacker-selected-owner"
    }, offscreenClientIdB)
  )), {
    accepted: true,
    pageUrl: "https://suno.com/home/advanced"
  });
  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-release-v1",
    nonce: replacementNonce,
    ownerDocumentId: replacementOwner
    }, offscreenClientIdA)
  )), { accepted: false });
  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-release-v1",
      nonce: replacementNonce
    }, offscreenClientIdB)
  )), { accepted: true });
});

test("challenge preparation rejects unsafe final URLs and clears frame rules", async () => {
  const unsafeUrls = [
    "https://attacker.example/create",
    "https://suno.com/create?redirected=1",
    "https://suno.com/create#unexpected",
    "https://user:password@suno.com/create",
    "https://suno.com:8443/create"
  ];
  for (const url of unsafeUrls) {
    const harness = popupServiceWorkerHarness({
      fetchImpl: async () => ({
        headers: new Headers({
          "content-type": "text/html"
        }),
        ok: true,
        status: 200,
        url
      })
    });
    await flushAsync();
    await flushAsync();

    const preparation = await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: managedNonce
    });
    assert.deepEqual(JSON.parse(JSON.stringify(preparation)), {
      accepted: false,
      error: "challenge_environment_unavailable"
    });
    assert.deepEqual(
      JSON.parse(JSON.stringify(harness.calls.sessionRules.at(-1))),
      { removeRuleIds: [29_764, 29_765] }
    );
  }
});

test("challenge preparation replaces a restrictive upstream CSP with the controlled document CSP", async () => {
  const harness = popupServiceWorkerHarness({
    fetchImpl: async () => ({
      headers: new Headers({
        "content-type": "text/html; charset=utf-8",
        "content-security-policy": "frame-ancestors 'none'"
      }),
      ok: true,
      status: 200,
      url: "https://suno.com/create"
    })
  });
  await flushAsync();
  await flushAsync();

  const preparation = await harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-prepare-v1",
    nonce: managedNonce
  });
  assert.deepEqual(JSON.parse(JSON.stringify(preparation)), {
    accepted: true,
    pageUrl: "https://suno.com/create"
  });
  const frameRule = JSON.parse(JSON.stringify(
    harness.calls.sessionRules.at(-1).addRules.find(
      (rule) => rule.id === 29_764
    )
  ));
  assert.equal("responseHeaders" in frameRule.condition, false);
  assert.equal(
    frameRule.action.responseHeaders[0].value,
    expectedControlledDocumentCsp(
      "turnstile",
      "abcdefghijklmnopabcdefghijklmnop"
    )
  );
});

test("challenge preparation rejects a non-HTML discovery response", async () => {
  const harness = popupServiceWorkerHarness({
    fetchImpl: async () => ({
      headers: new Headers({
        "content-type": "application/json"
      }),
      ok: true,
      status: 200,
      url: "https://suno.com/create"
    })
  });
  await flushAsync();
  await flushAsync();

  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: managedNonce
    })
  )), {
    accepted: false,
    error: "challenge_environment_unavailable"
  });
  assert.deepEqual(
    JSON.parse(JSON.stringify(harness.calls.sessionRules.at(-1))),
    { removeRuleIds: [29_764, 29_765] }
  );
});

test("service worker relays one nonce-bound offscreen frame challenge without creating a window", async () => {
  const harness = popupServiceWorkerHarness();
  await flushAsync();
  await flushAsync();
  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: managedNonce
    })
  )), {
    accepted: true,
    pageUrl: "https://suno.com/home/advanced"
  });

  const port = harness.connect({
    documentId: "offscreen-frame-document",
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

test("service worker exposes only static Turnstile diagnostic families", async () => {
  const cases = [
    [
      "turnstile_error_200",
      "silent_challenge_unavailable: Turnstile error callback (family 200)"
    ],
    [
      "turnstile_error_unknown",
      "silent_challenge_unavailable: Turnstile reported an unrecognized provider error"
    ],
    [
      "turnstile_no_callback",
      "silent_challenge_unavailable: Turnstile produced no callback before the silent deadline"
    ]
  ];

  for (const [errorCode, expectedError] of cases) {
    const harness = popupServiceWorkerHarness();
    await flushAsync();
    await flushAsync();
    await harness.dispatchFromOffscreen({
      type: "sunox-frame-environment-prepare-v1",
      nonce: managedNonce
    });
    const port = harness.connect({
      documentId: `diagnostic-${errorCode}`,
      id: "abcdefghijklmnopabcdefghijklmnop",
      origin: "https://suno.com",
      url: harness.managedUrl
    }, "sunox-managed-frame-v2");
    await flushAsync();
    await harness.dispatchFromOffscreen({
      type: "sunox-managed-frame-execute-v2",
      nonce: managedNonce,
      provider: "turnstile",
      requestId: `request-${errorCode}`
    });
    port.deliver({
      type: "sunox-managed-frame-result-v2",
      requestId: `request-${errorCode}`,
      token: null,
      errorCode
    });
    await flushAsync();

    assert.deepEqual(
      JSON.parse(JSON.stringify(harness.calls.notifications.at(-1))),
      {
        type: "sunox-managed-frame-result-v2",
        nonce: managedNonce,
        requestId: `request-${errorCode}`,
        token: null,
        error: expectedError
      }
    );
    assert.deepEqual(harness.calls.createdWindows, []);
  }
});

test("service worker accepts a real no-tab sender without frame or document IDs", async () => {
  const harness = popupServiceWorkerHarness();
  await flushAsync();
  await flushAsync();
  await harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-prepare-v1",
    nonce: managedNonce
  });

  const port = harness.connect({
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

test("service worker accepts matching documentId without a sender frameId", async () => {
  const harness = popupServiceWorkerHarness({
    autoObserveManagedResponse: false
  });
  await flushAsync();
  await flushAsync();
  await harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-prepare-v1",
    nonce: managedNonce
  });
  harness.observeManagedResponse();
  const sender = {
    documentId: "matching-managed-document",
    id: "abcdefghijklmnopabcdefghijklmnop",
    origin: "https://suno.com",
    url: harness.managedUrl
  };
  const port = harness.connect(sender, "sunox-managed-frame-v2");
  await flushAsync();

  assert.equal(port.disconnected, false);
  assert.deepEqual(
    JSON.parse(JSON.stringify(harness.calls.notifications.at(-1))),
    {
      type: "sunox-managed-frame-ready-v2",
      nonce: managedNonce
    }
  );

  await harness.dispatchFromSender({
    type: "sunox-managed-frame-stage-report-v1",
    nonce: managedNonce,
    stage: "controlled_document"
  }, sender);
  assert.deepEqual(
    JSON.parse(JSON.stringify(harness.calls.notifications.at(-1))),
    {
      type: "sunox-managed-frame-stage-v1",
      nonce: managedNonce,
      stage: "controlled_document"
    }
  );
  assert.deepEqual(port.messages, []);

  const verifiedNotificationCount = harness.calls.notifications.length;
  await harness.dispatchFromSender({
    type: "sunox-managed-frame-stage-report-v1",
    nonce: managedNonce,
    stage: "runner_loaded"
  }, {
    ...sender,
    documentId: "mismatched-stage-document"
  });
  assert.equal(
    harness.calls.notifications.length,
    verifiedNotificationCount + 1,
    "an unverified sender must produce one sanitized diagnostic"
  );
  assert.deepEqual(
    JSON.parse(JSON.stringify(harness.calls.notifications.at(-1))),
    {
      type: "sunox-managed-frame-diagnostic-v1",
      nonce: managedNonce,
      reason: "stage_report_rejected_managed_document_mismatch"
    }
  );
  assert.deepEqual(port.messages, []);

  assert.deepEqual(JSON.parse(JSON.stringify(
    await harness.dispatchFromOffscreen({
      type: "sunox-managed-frame-execute-v2",
      nonce: managedNonce,
      provider: "turnstile",
      requestId: "request-after-stage"
    })
  )), { accepted: true });
  assert.deepEqual(JSON.parse(JSON.stringify(port.messages)), [{
    type: "sunox-managed-frame-execute-v2",
    provider: "turnstile",
    requestId: "request-after-stage"
  }]);
});

test("service worker rejects a mismatched managed document identity", async () => {
  const harness = popupServiceWorkerHarness({
    autoObserveManagedResponse: false
  });
  await flushAsync();
  await flushAsync();
  await harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-prepare-v1",
    nonce: managedNonce
  });
  harness.observeManagedResponse();
  await harness.dispatchFromSender({
    type: "sunox-managed-frame-stage-report-v1",
    nonce: managedNonce,
    stage: "controlled_document"
  }, {
    documentId: "bound-managed-document",
    id: "abcdefghijklmnopabcdefghijklmnop",
    origin: "https://suno.com",
    url: harness.managedUrl
  });

  const port = harness.connect({
    documentId: "different-managed-document",
    id: "abcdefghijklmnopabcdefghijklmnop",
    origin: "https://suno.com",
    url: harness.managedUrl
  }, "sunox-managed-frame-v2");
  await flushAsync();

  assert.equal(port.disconnected, true);
  assert.equal(
    harness.calls.notifications.at(-1)?.reason,
    "managed_document_mismatch"
  );
});

test("service worker rejects document identity downgrade after an optional ID was bound", async () => {
  const harness = popupServiceWorkerHarness({
    autoObserveManagedResponse: false
  });
  await flushAsync();
  await flushAsync();
  await harness.dispatchFromOffscreen({
    type: "sunox-frame-environment-prepare-v1",
    nonce: managedNonce
  });
  harness.observeManagedResponse();
  await harness.dispatchFromSender({
    type: "sunox-managed-frame-stage-report-v1",
    nonce: managedNonce,
    stage: "controlled_document"
  }, {
    documentId: "bound-managed-document",
    id: "abcdefghijklmnopabcdefghijklmnop",
    origin: "https://suno.com",
    url: harness.managedUrl
  });

  const port = harness.connect({
    id: "abcdefghijklmnopabcdefghijklmnop",
    origin: "https://suno.com",
    url: harness.managedUrl
  }, "sunox-managed-frame-v2");
  await flushAsync();

  assert.equal(port.disconnected, true);
  assert.equal(
    harness.calls.notifications.at(-1)?.reason,
    "managed_sender_document_missing"
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

test("a managed frame without an active nonce reservation is rejected", async () => {
  const harness = popupServiceWorkerHarness();
  const port = harness.connect({
    documentId: "document-before-bootstrap",
    frameId: 1,
    id: "abcdefghijklmnopabcdefghijklmnop",
    origin: "https://suno.com",
    url: harness.managedUrl
  }, "sunox-managed-frame-v2");

  assert.equal(port.disconnected, true);
  assert.deepEqual(JSON.parse(JSON.stringify(port.messages)), [{
    type: "sunox-managed-frame-rejected-v2"
  }]);
  await flushAsync();
  await flushAsync();
  await flushAsync();
  assert.deepEqual(
    JSON.parse(JSON.stringify(harness.calls.notifications.at(-1))),
    {
      type: "sunox-managed-frame-diagnostic-v1",
      nonce: managedNonce,
      reason: "managed_environment_missing"
    }
  );
});

test("isolated bridge stops the host response and reuses its real document root", () => {
  const nonce = "00000000-0000-4000-8000-000000000001";
  const href = managedFrameHref("https://suno.com/create/v3", nonce);
  let connections = 0;
  const hostileDocumentElement = elementHarness("host-html");
  hostileDocumentElement.setAttribute("class", "host-controlled");
  hostileDocumentElement.setAttribute("data-host-secret", "must-be-cleared");
  const hostileHead = elementHarness("host-head");
  const hostileBody = elementHarness("host-body");
  hostileDocumentElement.append(hostileHead, hostileBody);
  const context = {
    chrome: {
      runtime: {
        connect() {
          connections += 1;
          throw new Error("the bridge must wait for the MAIN-world ready marker");
        }
      }
    },
    clearInterval() {},
    clearTimeout() {},
    document: {
      documentElement: hostileDocumentElement,
      readyState: "loading"
    },
    location: {
      href,
      origin: "https://suno.com"
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
  context.top = {};
  context.parent = context.top;
  context.addEventListener = () => {};
  context.removeEventListener = () => {};
  context.postMessage = () => {};
  context.globalThis = context;
  const trace = prepareContentBridgeContext(context, { pageReady: false });
  vm.createContext(context);
  vm.runInContext(bridgeSource, context);

  assert.equal(trace[0], "stop");
  const replaceIndex = trace.indexOf("root-replace");
  const runnerUrlIndex = trace.indexOf("getURL:page.js");
  const runnerAppendIndex = trace.indexOf("append:script");
  assert.ok(replaceIndex > trace.indexOf("stop"));
  assert.ok(runnerUrlIndex > replaceIndex);
  assert.ok(runnerAppendIndex > runnerUrlIndex);
  assert.equal(
    context.document.documentElement.getAttribute(
      controlledDocumentAttribute
    ),
    nonce
  );
  assert.equal(
    context.document.documentElement,
    hostileDocumentElement
  );
  assert.equal(hostileDocumentElement.getAttribute("class"), null);
  assert.equal(hostileDocumentElement.getAttribute("data-host-secret"), null);
  assert.equal(hostileDocumentElement.children.includes(hostileHead), false);
  assert.equal(hostileDocumentElement.children.includes(hostileBody), false);
  assert.deepEqual(
    hostileDocumentElement.children.map((child) => child.name),
    ["head", "body"]
  );
  assert.equal(
    context.document.documentElement.getAttribute(pageReadyAttribute),
    null
  );
  const runner = context.document.head.children.at(-1);
  assert.equal(runner.name, "script");
  assert.equal(
    runner.src,
    "chrome-extension://abcdefghijklmnopabcdefghijklmnop/page.js"
  );
  assert.equal(runner.dataset.sunoxManagedRunner, nonce);
  assert.equal(context.__sunoxBridgeContentLoaded, true);
  assert.equal(connections, 0);

  const nonCredentialless = {
    ...context,
    __sunoxBridgeContentLoaded: false,
    document: {
      readyState: "loading"
    }
  };
  nonCredentialless.window = nonCredentialless;
  nonCredentialless.globalThis = nonCredentialless;
  nonCredentialless.top = {};
  nonCredentialless.parent = nonCredentialless.top;
  const rejectedTrace = prepareContentBridgeContext(nonCredentialless, {
    credentialless: false,
    pageReady: false
  });
  vm.createContext(nonCredentialless);
  vm.runInContext(bridgeSource, nonCredentialless);
  assert.equal(rejectedTrace[0], "stop");
  assert.ok(
    rejectedTrace.indexOf("root-replace") > rejectedTrace.indexOf("stop")
  );
  assert.equal(
    nonCredentialless.document.documentElement.getAttribute(
      controlledDocumentAttribute
    ),
    null
  );
  assert.equal(
    rejectedTrace.some((entry) => (
      entry === "getURL:page.js" || entry === "append:script"
    )),
    false
  );
  assert.notEqual(nonCredentialless.__sunoxBridgeContentLoaded, true);

  const invalidUrl = new URL(href);
  invalidUrl.hash =
    "sunox-browser-bridge=00000000-0000-4000-8000-000000000002";
  const invalidNonce = {
    ...context,
    __sunoxBridgeContentLoaded: false,
    document: {
      readyState: "loading"
    },
    location: {
      href: invalidUrl.href,
      origin: "https://suno.com"
    }
  };
  invalidNonce.window = invalidNonce;
  invalidNonce.globalThis = invalidNonce;
  invalidNonce.top = {};
  invalidNonce.parent = invalidNonce.top;
  const invalidTrace = prepareContentBridgeContext(invalidNonce, {
    pageReady: false
  });
  vm.createContext(invalidNonce);
  vm.runInContext(bridgeSource, invalidNonce);
  assert.deepEqual(invalidTrace, []);
  assert.notEqual(invalidNonce.__sunoxBridgeContentLoaded, true);
});

test("MAIN-world page bridge marks only an existing controlled document ready", () => {
  const nonce = "00000000-0000-4000-8000-000000000001";
  let messageListeners = 0;
  const context = {
    clearInterval() {},
    clearTimeout() {},
    document: {
      body: elementHarness("body"),
      head: elementHarness("head"),
      querySelector() {
        return null;
      },
      readyState: "complete"
    },
    location: {
      href: managedFrameHref("https://suno.com/create/v3", nonce),
      origin: "https://suno.com"
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
  context.top = {};
  context.parent = context.top;
  context.addEventListener = (type) => {
    if (type === "message") messageListeners += 1;
  };
  context.postMessage = () => {};
  context.globalThis = context;
  const { documentElement } = preparePageBridgeContext(context);
  vm.createContext(context);
  vm.runInContext(pageSource, context);

  assert.equal(
    documentElement.getAttribute(controlledDocumentAttribute),
    nonce
  );
  assert.equal(documentElement.getAttribute(pageReadyAttribute), nonce);
  assert.equal(context.__sunoxBridgePageLoaded, true);
  assert.equal(messageListeners, 1);

  const uncontrolled = {
    ...context,
    __sunoxBridgePageLoaded: false,
    document: {
      body: elementHarness("body"),
      head: elementHarness("head"),
      querySelector() {
        return null;
      },
      readyState: "complete"
    }
  };
  uncontrolled.window = uncontrolled;
  uncontrolled.globalThis = uncontrolled;
  uncontrolled.top = {};
  uncontrolled.parent = uncontrolled.top;
  uncontrolled.credentialless = true;
  uncontrolled.document.documentElement = elementHarness("html");
  vm.createContext(uncontrolled);
  vm.runInContext(pageSource, uncontrolled);
  assert.equal(uncontrolled.__sunoxBridgePageLoaded, true);
  assert.equal(
    uncontrolled.document.documentElement.getAttribute(pageReadyAttribute),
    null
  );
});

test("content bridge connects only after the exact nonce-bound frame is stable", () => {
  const nonce = "00000000-0000-4000-8000-000000000001";
  const location = {
    href: managedFrameHref(
      "https://suno.com/home/advanced",
      nonce
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
  prepareContentBridgeContext(context, { pageReady: false });
  vm.createContext(context);
  vm.runInContext(bridgeSource, context);

  // The first document_start poll may beat the asynchronously loaded
  // MAIN-world runner. It must keep polling instead of abandoning the only
  // managed-frame connection attempt.
  timers.shift().callback();
  assert.equal(connections, 0);
  assert.deepEqual(posted, []);
  context.document.documentElement.setAttribute(pageReadyAttribute, nonce);
  timers.shift().callback();
  assert.equal(connections, 0);
  assert.deepEqual(posted, []);
  now = 499;
  timers.shift().callback();
  assert.equal(connections, 0);
  assert.deepEqual(posted, []);
  now = 500;
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
      href: managedFrameHref(
        "https://suno.com/home/advanced",
        "00000000-0000-4000-8000-000000000001"
      ),
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
  prepareContentBridgeContext(context);
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

test("content bridge discards a token after same-document route drift", async () => {
  let portMessageListener;
  let realmWindow;
  let windowMessageListener;
  const posted = [];
  const timers = [];
  let now = 0;
  const nonce = "00000000-0000-4000-8000-000000000001";
  const context = {
    chrome: {
      runtime: {
        connect() {
          return {
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
    location: {
      href: managedFrameHref(
        "https://suno.com/home/advanced",
        nonce
      ),
      origin: "https://suno.com"
    },
    Promise,
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
  prepareContentBridgeContext(context);
  vm.createContext(context);
  vm.runInContext(bridgeSource, context);
  timers.shift()();
  now = 500;
  timers.shift()();
  realmWindow = vm.runInContext("window", context);

  const execution = portMessageListener({
    type: "sunox-managed-frame-execute-v2",
    requestId: "request-route-drift",
    provider: "turnstile"
  });
  context.location.href = managedFrameHref(
    "https://suno.com/library",
    nonce
  );
  windowMessageListener({
    data: {
      source: "sunox-page-v1",
      requestId: "request-route-drift",
      token: "must-not-escape"
    },
    origin: "https://suno.com",
    source: realmWindow
  });
  await execution;

  assert.deepEqual(JSON.parse(JSON.stringify(posted.at(-1))), {
    type: "sunox-managed-frame-result-v2",
    requestId: "request-route-drift",
    token: null,
    errorCode: "page_not_ready"
  });
});

test("content bridge rejects provider-mismatched main-world errors", async () => {
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
      href: managedFrameHref(
        "https://suno.com/home/advanced",
        "00000000-0000-4000-8000-000000000001"
      ),
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
  prepareContentBridgeContext(context);
  vm.createContext(context);
  vm.runInContext(bridgeSource, context);
  timers.shift()();
  now = 500;
  timers.shift()();
  realmWindow = vm.runInContext("window", context);

  const execution = portMessageListener({
    type: "sunox-managed-frame-execute-v2",
    requestId: "request-forged-main-world-error",
    provider: "hcaptcha"
  });
  windowMessageListener({
    data: {
      source: "sunox-page-v1",
      requestId: "request-forged-main-world-error",
      token: null,
      error: sentinel,
      errorCode: "turnstile_error_200"
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
      href: managedFrameHref(
        "https://suno.com/home/advanced",
        "00000000-0000-4000-8000-000000000001"
      ),
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
  preparePageBridgeContext(context);
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

test("page bridge discards a token after same-document route drift", async () => {
  const listeners = new Map();
  const pageResults = [];
  let finishHcaptcha;
  let realmWindow;
  const nonce = "00000000-0000-4000-8000-000000000001";
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
        return new Promise((resolve) => {
          finishHcaptcha = resolve;
        });
      },
      remove() {},
      render() {
        return "hcaptcha-widget";
      }
    },
    location: {
      href: managedFrameHref(
        "https://suno.com/home/advanced",
        nonce
      ),
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
  preparePageBridgeContext(context);
  vm.createContext(context);
  vm.runInContext(pageSource, context);
  realmWindow = vm.runInContext("window", context);

  const execution = listeners.get("message")({
    data: {
      source: "sunox-extension-v1",
      requestId: "request-page-route-drift",
      provider: "hcaptcha"
    },
    origin: "https://suno.com",
    source: realmWindow
  });
  for (let attempt = 0; attempt < 30 && !finishHcaptcha; attempt += 1) {
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  assert.equal(typeof finishHcaptcha, "function");
  context.location.href = managedFrameHref(
    "https://suno.com/library",
    nonce
  );
  finishHcaptcha({ response: "must-not-escape" });
  await execution;
  await flushAsync();

  assert.deepEqual(JSON.parse(JSON.stringify(pageResults)), [{
    source: "sunox-page-v1",
    requestId: "request-page-route-drift",
    errorCode: "page_not_ready"
  }]);
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
      href: managedFrameHref(
        "https://suno.com/home/advanced",
        "00000000-0000-4000-8000-000000000001"
      ),
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
  preparePageBridgeContext(context);
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
      href: managedFrameHref(
        "https://suno.com/home/advanced",
        "00000000-0000-4000-8000-000000000001"
      ),
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
  preparePageBridgeContext(context);
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
      href: managedFrameHref(
        "https://suno.com/home/advanced",
        "00000000-0000-4000-8000-000000000001"
      ),
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
  preparePageBridgeContext(context);
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
        href: managedFrameHref(
          "https://suno.com/home/advanced",
          "00000000-0000-4000-8000-000000000001"
        ),
        origin: "https://suno.com"
      },
      Promise,
      setInterval,
      setTimeout,
      turnstile: {
        execute() {},
        remove() {},
        render(_container, options) {
          queueMicrotask(() => {
            options[callbackName](callbackArgument);
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
    preparePageBridgeContext(context);
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

test("page bridge allows Turnstile error and timeout recovery before success", async () => {
  for (const [callbackName, callbackArgument, expectedReturn] of [
    ["error-callback", 200500, false],
    ["timeout-callback", undefined, undefined]
  ]) {
    const listeners = new Map();
    const pageResults = [];
    let callbackReturn;
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
        href: managedFrameHref(
          "https://suno.com/home/advanced",
          "00000000-0000-4000-8000-000000000001"
        ),
        origin: "https://suno.com"
      },
      Promise,
      setInterval,
      setTimeout,
      turnstile: {
        execute() {},
        remove() {},
        render(_container, options) {
          queueMicrotask(() => {
            callbackReturn = options[callbackName](callbackArgument);
            options.callback("recovered-turnstile-token");
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
    preparePageBridgeContext(context);
    vm.createContext(context);
    vm.runInContext(pageSource, context);
    realmWindow = vm.runInContext("window", context);

    await listeners.get("message")({
      data: {
        source: "sunox-extension-v1",
        requestId: `request-recover-${callbackName}`,
        provider: "turnstile"
      },
      origin: "https://suno.com",
      source: realmWindow
    });
    await flushAsync();

    assert.equal(callbackReturn, expectedReturn);
    assert.deepEqual(JSON.parse(JSON.stringify(pageResults)), [{
      source: "sunox-page-v1",
      requestId: `request-recover-${callbackName}`,
      token: "recovered-turnstile-token"
    }]);
  }
});

test("page bridge reports the last safe Turnstile family at its recovery cap", async () => {
  const listeners = new Map();
  const pageResults = [];
  const timers = new Map();
  const timeoutDelays = [];
  let nextTimeoutId = 0;
  let now = 0;
  let options;
  let realmWindow;
  const context = {
    clearInterval() {},
    clearTimeout(timeoutId) {
      timers.delete(timeoutId);
    },
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
      href: managedFrameHref(
        "https://suno.com/home/advanced",
        "00000000-0000-4000-8000-000000000001"
      ),
      origin: "https://suno.com"
    },
    Promise,
    setInterval(callback) {
      queueMicrotask(callback);
      return 1;
    },
    setTimeout(callback, delay) {
      nextTimeoutId += 1;
      timers.set(nextTimeoutId, callback);
      timeoutDelays.push(delay);
      return nextTimeoutId;
    },
    turnstile: {
      execute() {},
      remove() {},
      render(_container, renderedOptions) {
        options = renderedOptions;
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
  preparePageBridgeContext(context);
  vm.createContext(context);
  vm.runInContext(pageSource, context);
  realmWindow = vm.runInContext("window", context);

  const execution = listeners.get("message")({
    data: {
      source: "sunox-extension-v1",
      requestId: "request-turnstile-family-deadline",
      provider: "turnstile"
    },
    origin: "https://suno.com",
    source: realmWindow
  });
  await flushAsync();

  assert.deepEqual(timeoutDelays, [15_000]);
  now = 10_000;
  assert.equal(options["error-callback"](300030), false);
  now = 25_000;
  assert.equal(options["error-callback"](300031), false);
  assert.deepEqual(timeoutDelays, [15_000, 15_000, 5_000]);

  const deadline = [...timers.values()].at(-1);
  deadline();
  await execution;
  await flushAsync();

  assert.deepEqual(JSON.parse(JSON.stringify(pageResults)), [{
    source: "sunox-page-v1",
    requestId: "request-turnstile-family-deadline",
    errorCode: "turnstile_error_300"
  }]);
});

test("page bridge reports a distinct silent Turnstile callback deadline", async () => {
  const listeners = new Map();
  const pageResults = [];
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
      href: managedFrameHref(
        "https://suno.com/home/advanced",
        "00000000-0000-4000-8000-000000000001"
      ),
      origin: "https://suno.com"
    },
    Promise,
    setInterval,
    setTimeout(callback, delay) {
      if (delay === 15_000) queueMicrotask(callback);
      return 1;
    },
    turnstile: {
      execute() {},
      remove() {},
      render() {
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
  preparePageBridgeContext(context);
  vm.createContext(context);
  vm.runInContext(pageSource, context);
  realmWindow = vm.runInContext("window", context);

  await listeners.get("message")({
    data: {
      source: "sunox-extension-v1",
      requestId: "request-turnstile-no-callback",
      provider: "turnstile"
    },
    origin: "https://suno.com",
    source: realmWindow
  });
  await flushAsync();

  assert.deepEqual(JSON.parse(JSON.stringify(pageResults)), [{
    source: "sunox-page-v1",
    requestId: "request-turnstile-no-callback",
    errorCode: "turnstile_no_callback"
  }]);
});

test("page bridge settles only once when a Turnstile error arrives after success", async () => {
  const listeners = new Map();
  const pageResults = [];
  const timeoutDelays = [];
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
    location: {
      href: managedFrameHref(
        "https://suno.com/home/advanced",
        "00000000-0000-4000-8000-000000000001"
      ),
      origin: "https://suno.com"
    },
    Promise,
    setInterval,
    setTimeout(callback, delay) {
      timeoutDelays.push(delay);
      return setTimeout(callback, delay);
    },
    turnstile: {
      execute() {},
      remove() {},
      render(_container, renderedOptions) {
        options = renderedOptions;
        queueMicrotask(() => options.callback("first-turnstile-token"));
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
  preparePageBridgeContext(context);
  vm.createContext(context);
  vm.runInContext(pageSource, context);
  realmWindow = vm.runInContext("window", context);

  await listeners.get("message")({
    data: {
      source: "sunox-extension-v1",
      requestId: "request-turnstile-late-error",
      provider: "turnstile"
    },
    origin: "https://suno.com",
    source: realmWindow
  });
  assert.equal(options["error-callback"](200500), false);
  await flushAsync();

  assert.deepEqual(timeoutDelays, [15_000]);
  assert.deepEqual(JSON.parse(JSON.stringify(pageResults)), [{
    source: "sunox-page-v1",
    requestId: "request-turnstile-late-error",
    token: "first-turnstile-token"
  }]);
});

test("page bridge clears its deadline when the Turnstile SDK throws synchronously", async () => {
  for (const failingMethod of ["render", "execute"]) {
    const listeners = new Map();
    const pageResults = [];
    const timers = new Map();
    let nextTimeoutId = 0;
    let realmWindow;
    const context = {
      clearInterval() {},
      clearTimeout(timeoutId) {
        timers.delete(timeoutId);
      },
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
        href: managedFrameHref(
          "https://suno.com/home/advanced",
          "00000000-0000-4000-8000-000000000001"
        ),
        origin: "https://suno.com"
      },
      Promise,
      setInterval(callback) {
        queueMicrotask(callback);
        return 1;
      },
      setTimeout(callback) {
        nextTimeoutId += 1;
        timers.set(nextTimeoutId, callback);
        return nextTimeoutId;
      },
      turnstile: {
        execute() {
          if (failingMethod === "execute") {
            throw new Error("FORGED___turnstile_execute_secret");
          }
        },
        remove() {},
        render() {
          if (failingMethod === "render") {
            throw new Error("FORGED___turnstile_render_secret");
          }
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
    preparePageBridgeContext(context);
    vm.createContext(context);
    vm.runInContext(pageSource, context);
    realmWindow = vm.runInContext("window", context);

    await listeners.get("message")({
      data: {
        source: "sunox-extension-v1",
        requestId: `request-turnstile-${failingMethod}-throw`,
        provider: "turnstile"
      },
      origin: "https://suno.com",
      source: realmWindow
    });
    await flushAsync();

    assert.equal(timers.size, 0);
    assert.equal(
      JSON.stringify(pageResults).includes("FORGED___turnstile"),
      false
    );
    assert.deepEqual(JSON.parse(JSON.stringify(pageResults)), [{
      source: "sunox-page-v1",
      requestId: `request-turnstile-${failingMethod}-throw`,
      errorCode: "challenge_failed"
    }]);
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
      href: managedFrameHref(
        "https://suno.com/home/advanced",
        "00000000-0000-4000-8000-000000000001"
      ),
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
  preparePageBridgeContext(context);
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
      href: managedFrameHref(
        "https://suno.com/home/advanced",
        "00000000-0000-4000-8000-000000000001"
      ),
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
  preparePageBridgeContext(context);
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
      href: managedFrameHref(
        "https://suno.com/home/advanced",
        "00000000-0000-4000-8000-000000000001"
      ),
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
  preparePageBridgeContext(context);
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
        assert.equal(body.runtime_build, runtimeBuild);
        assert.equal(
          body.proof,
          bridgeProof(secret, "sunox-bridge-client-v3", [
            29_764,
            body.client_nonce,
            body.server_nonce,
            runtimeBuild,
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
        assert.equal(body.runtime_build, runtimeBuild);
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
        runtimeBuild,
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
    pageUrl: "https://suno.com/home/advanced#sunox-browser-bridge"
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
    pageUrl: "https://suno.com/home/advanced#sunox-browser-bridge"
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
    pageUrl: "https://suno.com/home/advanced#sunox-browser-bridge"
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
        getURL(path) {
          return `chrome-extension://abcdefghijklmnopabcdefghijklmnop/${path}`;
        },
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

  const initialPing = ping();
  assert.match(initialPing.clientId, /^offscreen-[0-9a-f-]{36}$/);
  assert.deepEqual(JSON.parse(JSON.stringify(initialPing)), {
    busy: false,
    busySince: null,
    clientId: initialPing.clientId,
    type: "sunox-offscreen-pong-v1",
    pollWorkerAgeMs: null,
    pollWorkerHealthy: false
  });

  pollWorkerMessageListener({ data: { type: "sunox-poll" } });
  now += 250;
  assert.deepEqual(JSON.parse(JSON.stringify(ping())), {
    busy: false,
    busySince: null,
    clientId: initialPing.clientId,
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
        getURL(path) {
          return `chrome-extension://abcdefghijklmnopabcdefghijklmnop/${path}`;
        },
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
  const busyPing = dispatch({ type: "sunox-offscreen-ping-v1" });
  assert.match(busyPing.clientId, /^offscreen-[0-9a-f-]{36}$/);
  assert.deepEqual(
    JSON.parse(JSON.stringify(busyPing)),
    {
      busy: true,
      busySince: 1_000,
      clientId: busyPing.clientId,
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
  const frameOperations = [];
  const claimedPageUrls = [];
  const resolvedPageUrl = "https://suno.com/create/v3";
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
        getURL(path) {
          return `chrome-extension://abcdefghijklmnopabcdefghijklmnop/${path}`;
        },
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
          if (message.type === "sunox-frame-environment-prepare-v1") {
            return { accepted: true, pageUrl: resolvedPageUrl };
          }
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
        let credentialless = false;
        let referrerPolicy = "";
        let src = "";
        const sandboxTokens = [];
        const frame = {
          get credentialless() {
            return credentialless;
          },
          set credentialless(value) {
            credentialless = value;
            frameOperations.push(["credentialless", value]);
          },
          get referrerPolicy() {
            return referrerPolicy;
          },
          set referrerPolicy(value) {
            referrerPolicy = value;
            frameOperations.push(["referrerPolicy", value]);
          },
          get src() {
            return src;
          },
          set src(value) {
            src = value;
            frameOperations.push(["src", value]);
          },
          addEventListener() {},
          removeEventListener() {},
          remove() {},
          sandbox: {
            add(...tokens) {
              sandboxTokens.push(...tokens);
              frameOperations.push(["sandbox", ...tokens]);
            },
            tokens: sandboxTokens
          },
          style: {}
        };
        return frame;
      }
    },
    Promise,
    setTimeout: detachedSetTimeout,
    URL,
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
        async claimChallenge({ pageUrl }) {
          claimedPageUrls.push(pageUrl);
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
  assert.deepEqual(claimedPageUrls, ["https://suno.com/"]);
  assert.equal(frames.length, 1);
  assert.equal(frames[0].credentialless, true);
  assert.equal(
    frames[0].referrerPolicy,
    "strict-origin-when-cross-origin"
  );
  assert.deepEqual(frames[0].sandbox.tokens, [
    "allow-forms",
    "allow-same-origin",
    "allow-scripts"
  ]);
  const srcIndex = frameOperations.findIndex(([operation]) => (
    operation === "src"
  ));
  assert.ok(srcIndex > -1);
  for (const prerequisite of [
    "credentialless",
    "referrerPolicy",
    "sandbox"
  ]) {
    assert.ok(
      frameOperations.findIndex(([operation]) => operation === prerequisite)
        < srcIndex,
      `${prerequisite} must be fixed before assigning iframe.src`
    );
  }
  const managedFrameUrl = new URL(frames[0].src);
  const networkNonce = managedFrameUrl.searchParams.get("__sunox_bridge");
  const fragmentNonce = managedFrameUrl.hash.slice(
    "#sunox-browser-bridge=".length
  );
  assert.equal(managedFrameUrl.origin, "https://suno.com");
  assert.equal(managedFrameUrl.pathname, "/create/v3");
  assert.deepEqual([...managedFrameUrl.searchParams.keys()], [
    "__sunox_bridge"
  ]);
  assert.match(networkNonce, /^[0-9a-f-]{36}$/);
  assert.equal(fragmentNonce, networkNonce);
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

function offscreenReadinessHarness({
  environmentResponse = {
    accepted: true,
    pageUrl: "https://suno.com/home/advanced"
  },
  onExecute = null,
  onFrameAppended = null,
  onRelease = null,
  onRetire = null
} = {}) {
  const runtimeListeners = new Set();
  const submitted = [];
  const frames = [];
  const timers = new Map();
  const executeMessages = [];
  const environmentMessages = [];
  let claims = 0;
  let now = 10_000;
  let nextTimerId = 1;
  const sender = {
    id: "abcdefghijklmnopabcdefghijklmnop"
  };
  const dispatchRuntimeMessage = (message) => {
    for (const listener of [...runtimeListeners]) {
      listener(message, sender, () => {});
    }
  };
  const fireFrameEvent = (frame, type) => {
    frame.eventListeners.get(type)?.();
  };
  const frameNonce = (frame) =>
    new URL(frame.src).hash.slice("#sunox-browser-bridge=".length);
  const frameNetworkNonce = (frame) =>
    new URL(frame.src).searchParams.get("__sunox_bridge");
  const context = {
    chrome: {
      runtime: {
        id: sender.id,
        getURL(path) {
          return `chrome-extension://abcdefghijklmnopabcdefghijklmnop/${path}`;
        },
        onMessage: {
          addListener(listener) {
            runtimeListeners.add(listener);
          },
          removeListener(listener) {
            runtimeListeners.delete(listener);
          }
        },
        async sendMessage(message) {
          if (message.type === "sunox-frame-environment-prepare-v1") {
            environmentMessages.push(structuredClone(message));
            return typeof environmentResponse === "function"
              ? await environmentResponse(message)
              : structuredClone(environmentResponse);
          }
          if (message.type === "sunox-frame-environment-release-v1") {
            environmentMessages.push(structuredClone(message));
            if (onRelease) {
              return await onRelease({
                dispatchRuntimeMessage,
                frames,
                message
              });
            }
            return { accepted: true };
          }
          if (message.type === "sunox-frame-environment-retire-v1") {
            environmentMessages.push(structuredClone(message));
            if (onRetire) {
              return await onRetire({
                dispatchRuntimeMessage,
                frames,
                message
              });
            }
            return { accepted: true };
          }
          if (message.type !== "sunox-managed-frame-execute-v2") return undefined;
          executeMessages.push(message);
          if (onExecute) {
            return await onExecute({
              dispatchRuntimeMessage,
              frames,
              message
            });
          }
          return { accepted: true };
        }
      }
    },
    clearTimeout(timerId) {
      timers.delete(timerId);
    },
    crypto,
    Date: {
      now() {
        return now;
      }
    },
    document: {
      body: {
        appendChild(frame) {
          frames.push(frame);
          onFrameAppended?.({
            dispatchRuntimeMessage,
            fireFrameEvent,
            frame,
            frameNonce: frameNonce(frame),
            index: frames.length - 1
          });
        }
      },
      createElement(name) {
        assert.equal(name, "iframe");
        const eventListeners = new Map();
        return {
          eventListeners,
          removed: false,
          addEventListener(type, listener) {
            eventListeners.set(type, listener);
          },
          removeEventListener(type, listener) {
            if (eventListeners.get(type) === listener) {
              eventListeners.delete(type);
            }
          },
          remove() {
            this.removed = true;
          },
          sandbox: {
            add() {}
          },
          style: {}
        };
      }
    },
    Promise,
    setTimeout(callback, delay) {
      const timerId = nextTimerId++;
      timers.set(timerId, {
        callback,
        delay,
        dueAt: now + delay
      });
      return timerId;
    },
    URL,
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
                provider: "turnstile",
                requestId: "request-cold-retry",
                transportReceipt: "receipt-cold-retry"
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
  const startedAt = now;
  return {
    dispatchRuntimeMessage,
    environmentMessages,
    executeMessages,
    frameNetworkNonce,
    frameNonce,
    frames,
    get claims() {
      return claims;
    },
    get elapsedMs() {
      return now - startedAt;
    },
    runtimeListeners,
    start() {
      dispatchRuntimeMessage({ type: "sunox-offscreen-start-v1" });
    },
    submitted,
    timers,
    runTimer(timerId) {
      const timer = timers.get(timerId);
      assert.ok(timer, `missing timer ${timerId}`);
      timers.delete(timerId);
      now = Math.max(now, timer.dueAt);
      timer.callback();
    },
    timerIdByDelay(delay) {
      return [...timers.entries()].find(([, timer]) => timer.delay === delay)?.[0];
    }
  };
}

test("offscreen refuses malformed prepared routes before creating an iframe", async () => {
  for (const pageUrl of [
    undefined,
    "https://attacker.example/create",
    "https://suno.com/create?unexpected=1",
    "https://suno.com/create#unexpected",
    "https://suno.com:8443/create"
  ]) {
    const harness = offscreenReadinessHarness({
      environmentResponse: {
        accepted: true,
        ...(pageUrl === undefined ? {} : { pageUrl })
      }
    });
    harness.start();
    await flushAsync();
    await flushAsync();

    assert.equal(harness.claims, 1);
    assert.equal(harness.frames.length, 0);
    assert.equal(harness.executeMessages.length, 0);
    assert.equal(harness.submitted.length, 1);
    assert.equal(harness.submitted[0].token, null);
    assert.match(
      harness.submitted[0].error,
      /challenge environment is unavailable/
    );
  }
});

test("offscreen releases a prepare nonce even when its response is lost", async () => {
  let attemptedNonce;
  const harness = offscreenReadinessHarness({
    environmentResponse(message) {
      attemptedNonce = message.nonce;
      throw new Error("simulated prepare response loss");
    }
  });
  harness.start();
  await flushAsync();
  await flushAsync();

  assert.match(attemptedNonce, /^[0-9a-f-]{36}$/);
  assert.equal(harness.frames.length, 0);
  assert.equal(harness.submitted.length, 1);
  assert.equal(harness.submitted[0].token, null);
  const clientId = harness.environmentMessages[0].clientId;
  assert.match(clientId, /^offscreen-[0-9a-f-]{36}$/);
  assert.deepEqual(harness.environmentMessages, [{
    type: "sunox-frame-environment-prepare-v1",
    clientId,
    nonce: attemptedNonce,
    previousNonce: null,
    provider: "turnstile"
  }, {
    type: "sunox-frame-environment-release-v1",
    clientId,
    nonce: attemptedNonce
  }]);
});

test("offscreen bounds initial environment preparation to nine seconds", async () => {
  const unresolvedPreparation = new Promise(() => {});
  const harness = offscreenReadinessHarness({
    environmentResponse() {
      return unresolvedPreparation;
    }
  });
  harness.start();
  await flushAsync();

  assert.equal(managedFramePrepareTimeoutMs, 9_000);
  assert.equal(harness.frames.length, 0);
  harness.runTimer(harness.timerIdByDelay(managedFramePrepareTimeoutMs));
  await flushAsync();
  await flushAsync();

  assert.equal(harness.elapsedMs, managedFramePrepareTimeoutMs);
  assert.equal(harness.frames.length, 0);
  assert.equal(harness.submitted.length, 1);
  assert.equal(
    harness.submitted[0].error,
    "Managed Suno challenge environment preparation timed out"
  );
  assert.deepEqual(
    harness.environmentMessages.map((message) => message.type),
    [
      "sunox-frame-environment-prepare-v1",
      "sunox-frame-environment-release-v1"
    ]
  );
  assert.equal(
    harness.environmentMessages[0].nonce,
    harness.environmentMessages[1].nonce
  );
});

test("offscreen retries one silent pre-execution frame without waiting for load", async () => {
  const harness = offscreenReadinessHarness({
    onExecute({ dispatchRuntimeMessage, message }) {
      queueMicrotask(() => {
        dispatchRuntimeMessage({
          type: "sunox-managed-frame-result-v2",
          nonce: message.nonce,
          requestId: message.requestId,
          token: "cold-retry-token"
        });
      });
      return { accepted: true };
    },
    onFrameAppended({
      dispatchRuntimeMessage,
      frameNonce,
      index
    }) {
      queueMicrotask(() => {
        if (index !== 1) return;
        dispatchRuntimeMessage({
          type: "sunox-managed-frame-ready-v2",
          nonce: frameNonce
        });
      });
    }
  });
  harness.start();
  await flushAsync();

  assert.equal(harness.frames.length, 1);
  const firstNonce = harness.frameNonce(harness.frames[0]);
  assert.equal(
    harness.frameNetworkNonce(harness.frames[0]),
    firstNonce
  );
  const warmupTimerId = harness.timerIdByDelay(3_000);
  assert.ok(
    warmupTimerId,
    "the silent first frame must use the warmup grace without a load event"
  );
  harness.runTimer(warmupTimerId);
  await flushAsync();
  await flushAsync();

  assert.equal(harness.frames.length, 2);
  const secondNonce = harness.frameNonce(harness.frames[1]);
  assert.equal(
    harness.frameNetworkNonce(harness.frames[1]),
    secondNonce
  );
  assert.notEqual(secondNonce, firstNonce);
  assert.equal(harness.frames.every((frame) => frame.removed), true);
  assert.equal(harness.frames.every((frame) => frame.eventListeners.size === 0), true);
  assert.equal(harness.claims, 1);
  assert.equal(harness.executeMessages.length, 1);
  assert.equal(harness.runtimeListeners.size, 1);
  assert.equal(harness.timers.size, 0);
  const clientId = harness.environmentMessages[0].clientId;
  assert.deepEqual(harness.environmentMessages, [{
    type: "sunox-frame-environment-prepare-v1",
    clientId,
    nonce: firstNonce,
    previousNonce: null,
    provider: "turnstile"
  }, {
    type: "sunox-frame-environment-retire-v1",
    clientId,
    nonce: firstNonce
  }, {
    type: "sunox-frame-environment-prepare-v1",
    clientId,
    nonce: secondNonce,
    previousNonce: firstNonce,
    provider: "turnstile"
  }, {
    type: "sunox-frame-environment-release-v1",
    clientId,
    nonce: secondNonce
  }, {
    type: "sunox-frame-environment-release-v1",
    clientId,
    nonce: firstNonce
  }]);
  assert.deepEqual(JSON.parse(JSON.stringify(harness.submitted)), [{
    transportReceipt: "receipt-cold-retry",
    requestId: "request-cold-retry",
    token: "cold-retry-token",
    error: null
  }]);
});

test("offscreen bounds second preparation by the shared readiness deadline", async () => {
  const unresolvedRotation = new Promise(() => {});
  const harness = offscreenReadinessHarness({
    environmentResponse(message) {
      return message.previousNonce === null
        ? {
            accepted: true,
            pageUrl: "https://suno.com/home/advanced"
          }
        : unresolvedRotation;
    }
  });
  harness.start();
  await flushAsync();

  const firstNonce = harness.frameNonce(harness.frames[0]);
  harness.runTimer(harness.timerIdByDelay(managedFrameWarmupGraceMs));
  await flushAsync();
  await flushAsync();

  assert.equal(harness.frames.length, 1);
  const secondPrepare = harness.environmentMessages.at(-1);
  assert.equal(secondPrepare.type, "sunox-frame-environment-prepare-v1");
  assert.equal(secondPrepare.previousNonce, firstNonce);
  assert.notEqual(secondPrepare.nonce, firstNonce);
  const remainingReadinessMs =
    managedFrameReadyTimeoutMs - managedFrameWarmupGraceMs;
  harness.runTimer(harness.timerIdByDelay(remainingReadinessMs));
  await flushAsync();
  await flushAsync();

  assert.equal(harness.elapsedMs, managedFrameReadyTimeoutMs);
  assert.equal(harness.frames.length, 1);
  assert.equal(harness.submitted.length, 1);
  assert.equal(
    harness.submitted[0].error,
    "Managed Suno challenge environment rotation exceeded the shared readiness deadline"
  );
  assert.deepEqual(
    harness.environmentMessages.map((message) => [
      message.type,
      message.nonce
    ]),
    [
      ["sunox-frame-environment-prepare-v1", firstNonce],
      ["sunox-frame-environment-retire-v1", firstNonce],
      ["sunox-frame-environment-prepare-v1", secondPrepare.nonce],
      ["sunox-frame-environment-release-v1", secondPrepare.nonce],
      ["sunox-frame-environment-release-v1", firstNonce]
    ],
    "a timed-out rotation must release the possibly committed new nonce first"
  );
});

test("iframe DOM errors are terminal in either network-error ordering", async () => {
  for (const order of ["dom-first", "network-first"]) {
    const harness = offscreenReadinessHarness({
      onFrameAppended({
        dispatchRuntimeMessage,
        fireFrameEvent,
        frame,
        frameNonce
      }) {
        queueMicrotask(() => {
          const fireDomError = () => fireFrameEvent(frame, "error");
          const reportNetworkError = () => dispatchRuntimeMessage({
            type: "sunox-managed-frame-diagnostic-v1",
            nonce: frameNonce,
            reason: "managed_network_error"
          });
          if (order === "dom-first") {
            fireDomError();
            reportNetworkError();
          } else {
            reportNetworkError();
            fireDomError();
          }
        });
      }
    });
    harness.start();
    await flushAsync();
    await flushAsync();

    assert.equal(harness.frames.length, 1, order);
    assert.equal(harness.frames[0].removed, true, order);
    assert.equal(harness.executeMessages.length, 0, order);
    assert.equal(
      harness.environmentMessages.some(
        (message) =>
          message.type === "sunox-frame-environment-retire-v1"
      ),
      false,
      order
    );
    assert.equal(harness.submitted.length, 1, order);
    assert.equal(harness.submitted[0].token, null, order);
    assert.match(
      harness.submitted[0].error,
      order === "dom-first"
        ? /Managed Suno frame failed to load \(attempt=1\/2/
        : /Managed Suno frame port was rejected \(managed_network_error\)/,
      order
    );
  }
});

test("offscreen waits for retire ACK before removing, retrying, or executing", async () => {
  let resolveRetirement;
  const retirementAck = new Promise((resolve) => {
    resolveRetirement = resolve;
  });
  const harness = offscreenReadinessHarness({
    onExecute({ dispatchRuntimeMessage, message }) {
      queueMicrotask(() => {
        dispatchRuntimeMessage({
          type: "sunox-managed-frame-result-v2",
          nonce: message.nonce,
          requestId: message.requestId,
          token: "retire-ordered-token"
        });
      });
      return { accepted: true };
    },
    onFrameAppended({
      dispatchRuntimeMessage,
      frameNonce,
      index
    }) {
      if (index !== 1) return;
      queueMicrotask(() => {
        dispatchRuntimeMessage({
          type: "sunox-managed-frame-ready-v2",
          nonce: frameNonce
        });
      });
    },
    onRetire() {
      return retirementAck;
    }
  });
  harness.start();
  await flushAsync();

  const firstFrame = harness.frames[0];
  const firstNonce = harness.frameNonce(firstFrame);
  harness.runTimer(harness.timerIdByDelay(3_000));
  await flushAsync();

  const clientId = harness.environmentMessages[0].clientId;
  assert.deepEqual(harness.environmentMessages, [{
    type: "sunox-frame-environment-prepare-v1",
    clientId,
    nonce: firstNonce,
    previousNonce: null,
    provider: "turnstile"
  }, {
    type: "sunox-frame-environment-retire-v1",
    clientId,
    nonce: firstNonce
  }]);
  assert.equal(firstFrame.removed, false);
  assert.equal(harness.frames.length, 1);
  assert.equal(harness.executeMessages.length, 0);

  harness.dispatchRuntimeMessage({
    type: "sunox-managed-frame-ready-v2",
    nonce: firstNonce
  });
  await flushAsync();
  assert.equal(
    harness.executeMessages.length,
    0,
    "a late ready event must not execute while retirement is pending"
  );
  assert.equal(firstFrame.removed, false);
  assert.equal(harness.frames.length, 1);

  resolveRetirement({ accepted: true });
  await flushAsync();
  await flushAsync();
  await flushAsync();

  assert.equal(firstFrame.removed, true);
  assert.equal(harness.frames.length, 2);
  assert.equal(harness.executeMessages.length, 1);
  assert.equal(harness.submitted.length, 1);
  assert.equal(harness.submitted[0].token, "retire-ordered-token");
});

test("offscreen never retries when retirement is rejected", async () => {
  const harness = offscreenReadinessHarness({
    onRetire() {
      return { accepted: false };
    }
  });
  harness.start();
  await flushAsync();

  harness.runTimer(harness.timerIdByDelay(3_000));
  await flushAsync();
  await flushAsync();

  assert.equal(harness.frames.length, 1);
  assert.equal(harness.frames[0].removed, true);
  assert.equal(harness.executeMessages.length, 0);
  assert.equal(harness.submitted.length, 1);
  assert.equal(harness.submitted[0].token, null);
  assert.equal(
    harness.submitted[0].error,
    "Managed Suno frame could not retire its first readiness attempt safely"
  );
});

test("a pending release never races an older nonce release", async () => {
  const unresolvedRelease = new Promise(() => {});
  const harness = offscreenReadinessHarness({
    onFrameAppended({
      fireFrameEvent,
      frame,
      index
    }) {
      if (index === 1) {
        queueMicrotask(() => fireFrameEvent(frame, "error"));
      }
    },
    onRelease() {
      return unresolvedRelease;
    }
  });
  harness.start();
  await flushAsync();

  const firstNonce = harness.frameNonce(harness.frames[0]);
  harness.runTimer(harness.timerIdByDelay(managedFrameWarmupGraceMs));
  await flushAsync();
  await flushAsync();

  assert.equal(harness.frames.length, 2);
  const secondNonce = harness.frameNonce(harness.frames[1]);
  assert.deepEqual(
    harness.environmentMessages.map((message) => [
      message.type,
      message.nonce
    ]),
    [
      ["sunox-frame-environment-prepare-v1", firstNonce],
      ["sunox-frame-environment-retire-v1", firstNonce],
      ["sunox-frame-environment-prepare-v1", secondNonce],
      ["sunox-frame-environment-release-v1", secondNonce]
    ]
  );
  assert.equal(harness.submitted.length, 0);
  assert.equal(managedFrameReleaseTimeoutMs, 500);

  harness.runTimer(
    harness.timerIdByDelay(managedFrameReleaseTimeoutMs)
  );
  await flushAsync();
  await flushAsync();

  assert.equal(
    harness.environmentMessages.some((message) => (
      message.type === "sunox-frame-environment-release-v1"
      && message.nonce === firstNonce
    )),
    false,
    "the timed-out second release owns cleanup and forbids a concurrent old release"
  );
  assert.equal(harness.submitted.length, 1);
  assert.equal(harness.submitted[0].token, null);
});

test("offscreen readiness attempts share one absolute deadline", async () => {
  const harness = offscreenReadinessHarness();
  harness.start();
  await flushAsync();

  harness.runTimer(harness.timerIdByDelay(3_000));
  await flushAsync();
  assert.equal(harness.frames.length, 2);
  harness.runTimer(harness.timerIdByDelay(42_000));
  await flushAsync();
  await flushAsync();

  assert.equal(harness.elapsedMs, 45_000);
  assert.equal(harness.claims, 1);
  assert.equal(harness.executeMessages.length, 0);
  assert.equal(harness.frames.every((frame) => frame.removed), true);
  assert.equal(harness.frames.every((frame) => frame.eventListeners.size === 0), true);
  assert.equal(harness.runtimeListeners.size, 1);
  assert.equal(harness.timers.size, 0);
  assert.equal(harness.submitted.length, 1);
  assert.equal(harness.submitted[0].token, null);
  assert.match(harness.submitted[0].error, /shared 45 second deadline/);
});

test("offscreen records startup failure stages and retries only once", async () => {
  const harness = offscreenReadinessHarness({
    onFrameAppended({
      dispatchRuntimeMessage,
      frameNonce
    }) {
      queueMicrotask(() => {
        dispatchRuntimeMessage({
          type: "sunox-managed-frame-stage-v1",
          nonce: frameNonce,
          stage: "runner_injected"
        });
        dispatchRuntimeMessage({
          type: "sunox-managed-frame-stage-v1",
          nonce: frameNonce,
          stage: "runner_error"
        });
      });
    }
  });
  harness.start();
  await flushAsync();
  await flushAsync();
  await flushAsync();

  assert.equal(harness.frames.length, 2);
  assert.equal(harness.executeMessages.length, 0);
  assert.equal(harness.frames.every((frame) => frame.removed), true);
  assert.equal(harness.runtimeListeners.size, 1);
  assert.equal(harness.timers.size, 0);
  assert.equal(harness.submitted.length, 1);
  assert.equal(harness.submitted[0].token, null);
  assert.match(
    harness.submitted[0].error,
    /startup failed \(attempt=2\/2, stage=runner_error\)/
  );
});

test("offscreen never retries after challenge execution begins", async () => {
  const harness = offscreenReadinessHarness({
    onExecute({ dispatchRuntimeMessage, message }) {
      queueMicrotask(() => {
        dispatchRuntimeMessage({
          type: "sunox-managed-frame-disconnected-v2",
          nonce: message.nonce
        });
      });
      return { accepted: true };
    },
    onFrameAppended({
      dispatchRuntimeMessage,
      fireFrameEvent,
      frame,
      frameNonce
    }) {
      queueMicrotask(() => {
        fireFrameEvent(frame, "load");
        dispatchRuntimeMessage({
          type: "sunox-managed-frame-ready-v2",
          nonce: frameNonce
        });
      });
    }
  });
  harness.start();
  await flushAsync();
  await flushAsync();

  assert.equal(harness.frames.length, 1);
  assert.equal(harness.frames[0].removed, true);
  assert.equal(harness.frames[0].eventListeners.size, 0);
  assert.equal(harness.claims, 1);
  assert.equal(harness.executeMessages.length, 1);
  assert.equal(harness.runtimeListeners.size, 1);
  assert.equal(harness.timers.size, 0);
  assert.deepEqual(JSON.parse(JSON.stringify(harness.submitted)), [{
    transportReceipt: "receipt-cold-retry",
    requestId: "request-cold-retry",
    token: null,
    error: "Managed Suno frame disconnected after challenge execution began"
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

function managedPageGateActivated(
  source,
  href,
  { framed = true, nested = false } = {}
) {
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
    document: {
      body: elementHarness("body"),
      head: elementHarness("head"),
      readyState: "complete"
    },
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
  context.parent = framed
    ? nested ? {} : context.top
    : context;
  context.addEventListener = (type) => {
    if (type === "message") messageListenerRegistered = true;
  };
  context.postMessage = () => {};
  context.globalThis = context;
  if (source === bridgeSource) {
    prepareContentBridgeContext(context);
  } else {
    preparePageBridgeContext(context);
  }
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
  const query = `__sunox_bridge=${nonce}`;
  const invalidUrls = [
    ["missing network nonce", `https://suno.com/create${hash}`],
    [
      "mismatched nonce",
      `https://suno.com/create?__sunox_bridge=${managedNonce}${hash}`
    ],
    ["wrong origin", `https://www.suno.com/create?${query}${hash}`],
    [
      "credentials",
      `https://user:password@suno.com/home/advanced?${query}${hash}`
    ],
    ["port", `https://suno.com:8443/home/advanced?${query}${hash}`],
    [
      "duplicate query",
      `https://suno.com/home/advanced?${query}`
      + "&__clerk_handshake=one&__clerk_handshake=two"
      + hash
    ],
    [
      "extra query",
      `https://suno.com/home/advanced?${query}`
      + "&__clerk_handshake=one&unexpected=two"
      + hash
    ],
    [
      "oversized handshake",
      `https://suno.com/home/advanced?${query}`
      + `&__clerk_handshake=${"x".repeat(65_537)}${hash}`
    ],
    [
      "oversized URL",
      `https://suno.com/home/advanced?${query}`
      + `&__clerk_handshake=${"x".repeat(131_073)}${hash}`
    ],
    [
      "wrong hash",
      `https://suno.com/home/advanced?${query}`
      + `#sunox-browser-bridge=${nonce}-unexpected`
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
  const href = managedFrameHref(
    "https://suno.com/create/v3",
    "00000000-0000-4000-8000-000000000001"
  );
  for (const source of [bridgeSource, pageSource]) {
    assert.equal(managedPageGateActivated(source, href), true);
    assert.equal(
      managedPageGateActivated(source, href, { framed: false }),
      false
    );
    assert.equal(
      managedPageGateActivated(source, href, { nested: true }),
      false
    );
  }
});

test("managed page scripts reject redirect parameters on the nonce-bound route", () => {
  const href = managedFrameHref(
    "https://suno.com/create/v3",
    "00000000-0000-4000-8000-000000000001",
    { clerkHandshake: "opaque-return" }
  );
  for (const source of [bridgeSource, pageSource]) {
    assert.equal(managedPageGateActivated(source, href), false);
  }
});

test("manifest and scripts expose only the invisible offscreen-frame transport", () => {
  assert.equal(runtimeBuild, "0.3.41");
  assert.equal(manifest.version, "__SUNOX_BRIDGE_RUNTIME_BUILD__");
  assert.equal(manifest.version_name, "__SUNOX_BRIDGE_RUNTIME_BUILD__");
  assert.equal(manifest.minimum_chrome_version, "128");
  assert.equal(manifest.permissions.includes("declarativeNetRequestFeedback"), false);
  assert.ok(manifest.permissions.includes("declarativeNetRequestWithHostAccess"));
  assert.equal(manifest.permissions.includes("activeTab"), false);
  assert.ok(manifest.permissions.includes("offscreen"));
  assert.ok(manifest.permissions.includes("webRequest"));
  assert.equal(manifest.permissions.includes("tabs"), false);
  assert.equal(manifest.permissions.includes("storage"), false);
  assert.equal(manifest.permissions.includes("system.display"), false);
  assert.equal(
    manifest.host_permissions.includes("https://auth.suno.com/*"),
    false
  );
  assert.deepEqual(manifest.host_permissions, [
    "http://127.0.0.1/*",
    "https://suno.com/*"
  ]);
  assert.ok(manifest.content_scripts.every((script) => script.all_frames === true));
  assert.ok(
    manifest.content_scripts.every(
      (script) =>
        script.matches.length === 1
        && script.matches[0] === "https://suno.com/*"
    )
  );
  assert.equal(
    manifest.content_scripts.some((script) =>
      script.matches.some((match) => match.includes("auth.suno.com"))
    ),
    false
  );
  assert.deepEqual(manifest.content_scripts.map((script) => ({
    js: script.js,
    runAt: script.run_at,
    world: script.world ?? "ISOLATED"
  })), [{
    js: ["bridge.js"],
    runAt: "document_start",
    world: "ISOLATED"
  }]);
  assert.equal(
    manifest.content_scripts.some((script) => (
      script.world === "MAIN" || script.js.includes("page.js")
    )),
    false
  );
  assert.deepEqual(manifest.web_accessible_resources, [{
    resources: ["page.js"],
    matches: ["https://suno.com/*"]
  }]);
  assert.equal(
    manifest.web_accessible_resources.some((entry) => (
      entry.resources.some((resource) => resource.includes("*"))
      || entry.matches.some((match) => match === "<all_urls>")
    )),
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
  assert.ok(offscreenSource.includes("runtimeMessageBeforeDeadline"));
  assert.equal(managedFramePrepareTimeoutMs, 9_000);
  assert.equal(managedFrameReleaseTimeoutMs, 500);
  assert.ok(offscreenSource.includes("sunox-frame-environment-retire-v1"));
  assert.ok(offscreenSource.includes("retirementPending"));
  assert.ok(serviceWorkerSource.includes("declarativeNetRequest"));
  assert.ok(serviceWorkerSource.includes("addRules"));
  assert.ok(serviceWorkerSource.includes("content-security-policy"));
  assert.ok(serviceWorkerSource.includes("x-frame-options"));
  assert.ok(serviceWorkerSource.includes("challengeDocumentCsp"));
  assert.equal(serviceWorkerSource.includes("cspWithoutFrameAncestors"), false);
  const startBootstrapSource = serviceWorkerSource.slice(
    serviceWorkerSource.indexOf("function startBootstrap"),
    serviceWorkerSource.indexOf("chrome.runtime.onConnect")
  );
  assert.ok(startBootstrapSource.includes("console.warn("));
  assert.equal(startBootstrapSource.includes("console.error("), false);
  assert.equal(serviceWorkerSource.includes("chrome.windows"), false);
  for (const tabsApi of [
    "chrome.tabs.create",
    "chrome.tabs.update",
    "chrome.tabs.remove",
    "chrome.tabs.reload",
    "chrome.tabs.query"
  ]) {
    assert.equal(serviceWorkerSource.includes(tabsApi), false);
  }
  assert.equal(serviceWorkerSource.includes("chrome.storage"), false);
  assert.ok(serviceWorkerSource.includes("isOffscreenSender(sender)"));
  assert.ok(
    serviceWorkerSource.includes(
      "frameEnvironmentPromise = prepareFrameEnvironmentForOwner"
    )
  );
  assert.ok(serviceWorkerSource.includes("pendingEnvironmentOwnerDocumentId"));
  assert.ok(serviceWorkerSource.includes("requireCurrentOffscreenOwner"));
  assert.ok(serviceWorkerSource.includes("offscreenOwnerBinding"));
  assert.ok(serviceWorkerSource.includes("OFFSCREEN_CLIENT_ID_PATTERN"));
  assert.equal(offscreenSource.includes("chrome.runtime.getContexts"), false);
  assert.equal(offscreenSource.includes("ownerDocumentId"), false);
  assert.ok(
    serviceWorkerSource.includes("pendingEnvironmentNonce !== nonce")
  );
  assert.ok(serviceWorkerSource.includes("sunox-frame-environment-retire-v1"));
  assert.ok(serviceWorkerSource.includes("network.retiring"));
  assert.equal(
    [serviceWorkerSource, offscreenSource, bridgeSource].some((source) =>
      source.includes("sunox-managed-window")
    ),
    false
  );
  assert.ok(bridgeSource.includes('"sunox-managed-frame-v2"'));
  assert.ok(bridgeSource.includes('chrome.runtime.getURL("page.js")'));
  assert.ok(bridgeSource.includes("document.head.appendChild(mainRunner)"));
  assert.ok(bridgeSource.includes("sunox-managed-frame-execute-v2"));
  assert.ok(bridgeSource.includes("sunox-managed-frame-keepalive-v2"));
  assert.ok(bridgeSource.includes("sunox-managed-frame-result-v2"));
  for (const source of [
    serviceWorkerSource,
    offscreenSource,
    bridgeSource,
    pageSource
  ]) {
    assert.equal(source.includes("/home/advanced"), false);
  }
  const sdkReadyTimeout = numericConstant(
    pageSource,
    "CHALLENGE_SDK_READY_TIMEOUT_MS"
  );
  const hcaptchaSilentTimeout = numericConstant(
    pageSource,
    "HCAPTCHA_SILENT_TIMEOUT_MS"
  );
  const idleTimeout = numericConstant(pageSource, "TURNSTILE_IDLE_TIMEOUT_MS");
  const recoveryTimeout = numericConstant(
    pageSource,
    "TURNSTILE_RECOVERY_TIMEOUT_MS"
  );
  const pageTimeout = numericConstant(bridgeSource, "challengePageTimeoutMs");
  const frameWarmupGrace = numericConstant(
    offscreenSource,
    "managedFrameWarmupGraceMs"
  );
  const frameReadyTimeout = numericConstant(
    offscreenSource,
    "managedFrameReadyTimeoutMs"
  );
  const frameResultTimeout = numericConstant(
    offscreenSource,
    "managedFrameResultTimeoutMs"
  );
  const initialRuleFetchTimeout = numericConstant(
    serviceWorkerSource,
    "INITIAL_RULE_FETCH_TIMEOUT_MS"
  );
  const offscreenBusyMaxAge = numericConstant(
    serviceWorkerSource,
    "OFFSCREEN_BUSY_MAX_AGE_MS"
  );
  assert.ok(
    pageTimeout > sdkReadyTimeout + idleTimeout + hcaptchaSilentTimeout
  );
  assert.equal(recoveryTimeout, 30_000);
  assert.ok(recoveryTimeout > idleTimeout);
  assert.ok(pageTimeout > recoveryTimeout);
  assert.ok(frameResultTimeout > pageTimeout);
  assert.ok(
    offscreenBusyMaxAge
      > initialRuleFetchTimeout + frameReadyTimeout + frameResultTimeout
  );
  assert.equal(frameWarmupGrace, 3_000);
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
  assert.ok(offscreenSource.includes("managedFrame.credentialless = true"));
  assert.equal(offscreenSource.includes("chrome.windows"), false);
  assert.equal(offscreenSource.includes("chrome.tabs"), false);
  assert.equal(
    [serviceWorkerSource, offscreenSource, bridgeSource, pageSource]
      .some((source) => source.includes("window.open(")),
    false
  );
});
