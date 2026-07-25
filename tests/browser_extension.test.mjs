import assert from "node:assert/strict";
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

function numericConstant(source, name) {
  const match = source.match(new RegExp(`const ${name} = ([0-9_]+);`));
  assert.ok(match, `missing numeric constant ${name}`);
  return Number(match[1].replaceAll("_", ""));
}

function serviceWorkerHarness({
  offscreenExists = false,
  offscreenReady = true,
  csp = "frame-ancestors 'none'; font-src 'self'",
  legacyTabs = [],
  existingRules = [],
  responseOk = true,
  responseStatus = 200,
  responseUrl = "https://suno.com/create"
} = {}) {
  let documentExists = offscreenExists;
  const calls = {
    alarms: [],
    closedOffscreenDocuments: 0,
    heartbeats: 0,
    offscreenDocuments: [],
    removedTabs: [],
    runtimeMessages: [],
    sessionRuleReads: 0,
    cspFetches: 0,
    cspFetchOptions: [],
    sessionRuleUpdates: []
  };
  const listeners = {};
  const chrome = {
    alarms: {
      async create(name, options) {
        calls.alarms.push({ name, options });
      },
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
        calls.sessionRuleReads += 1;
        return existingRules;
      },
      async updateSessionRules(options) {
        calls.sessionRuleUpdates.push(options);
      }
    },
    offscreen: {
      async closeDocument() {
        calls.closedOffscreenDocuments += 1;
        documentExists = false;
      },
      async createDocument(options) {
        calls.offscreenDocuments.push(options);
        documentExists = true;
      }
    },
    runtime: {
      id: "abcdefghijklmnopabcdefghijklmnop",
      getURL(path) {
        return `chrome-extension://abcdefghijklmnopabcdefghijklmnop/${path}`;
      },
      async getContexts() {
        return documentExists ? [{ contextType: "OFFSCREEN_DOCUMENT" }] : [];
      },
      async sendMessage(message) {
        if (message.type === "sunox-offscreen-ping-v1") {
          calls.heartbeats += 1;
          return offscreenReady ? { type: "sunox-offscreen-pong-v1" } : undefined;
        }
        calls.runtimeMessages.push(message);
        return undefined;
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
      }
    },
    tabs: {
      TAB_ID_NONE: -1,
      async query() {
        return legacyTabs;
      },
      async remove(tabId) {
        calls.removedTabs.push(tabId);
      }
    }
  };
  const context = {
    AbortController,
    chrome,
    clearTimeout,
    fetch: async (_url, options) => {
      calls.cspFetches += 1;
      calls.cspFetchOptions.push(options);
      return {
        ok: responseOk,
        status: responseStatus,
        url: responseUrl,
        headers: {
          get(name) {
            return name === "content-security-policy" ? csp : null;
          }
        }
      };
    },
    Promise,
    setTimeout,
    URL
  };
  context.globalThis = context;
  vm.createContext(context);
  vm.runInContext(serviceWorkerSource, context);
  return { calls, listeners };
}

test("bootstrap installs a narrowly scoped iframe-header rule and creates no browser window", async () => {
  const { calls } = serviceWorkerHarness();
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.equal(calls.sessionRuleUpdates.length, 1);
  const rule = calls.sessionRuleUpdates[0].addRules[0];
  assert.equal(rule.action.type, "modifyHeaders");
  assert.deepEqual(
    Array.from(
      rule.action.responseHeaders,
      ({ header, operation, value }) => ({ header, operation, value })
    ),
    [
      {
        header: "content-security-policy",
        operation: "set",
        value: "font-src 'self';"
      },
      { header: "x-frame-options", operation: "remove", value: undefined }
    ]
  );
  assert.equal(rule.condition.regexFilter, "^https://suno\\.com/create/?(?:[?#].*)?$");
  const managedUrl = new RegExp(rule.condition.regexFilter);
  assert.equal(managedUrl.test("https://suno.com/create#sunox-browser-bridge"), true);
  assert.equal(managedUrl.test("https://suno.com/create/?mode=custom#bridge"), true);
  assert.equal(managedUrl.test("https://suno.com/"), false);
  assert.deepEqual(Array.from(rule.condition.initiatorDomains), [
    "abcdefghijklmnopabcdefghijklmnop"
  ]);
  assert.deepEqual(Array.from(rule.condition.resourceTypes), ["sub_frame"]);
  assert.deepEqual(Array.from(rule.condition.tabIds), [-1]);

  assert.equal(calls.offscreenDocuments.length, 1);
  assert.deepEqual(Array.from(calls.offscreenDocuments[0].reasons), [
    "IFRAME_SCRIPTING",
    "WORKERS"
  ]);
  assert.equal(serviceWorkerSource.includes("chrome.windows.create"), false);
  assert.equal(serviceWorkerSource.includes("chrome.tabs.create"), false);
  assert.equal(calls.cspFetchOptions[0].method, "HEAD");
});

test("bootstrap reuses a valid session rule when its bounded refresh fails", async () => {
  const responseHeaders = [
    {
      header: "content-security-policy",
      operation: "set",
      value: "font-src 'self';"
    },
    { header: "x-frame-options", operation: "remove" }
  ];
  const existingRules = [{
    id: 29764,
    priority: 1,
    action: {
      type: "modifyHeaders",
      responseHeaders
    },
    condition: {
      regexFilter: "^https://suno\\.com/create/?(?:[?#].*)?$",
      initiatorDomains: ["abcdefghijklmnopabcdefghijklmnop"],
      resourceTypes: ["sub_frame"],
      tabIds: [-1]
    }
  }];
  const { calls } = serviceWorkerHarness({
    existingRules,
    responseOk: false,
    responseStatus: 503
  });
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.equal(calls.sessionRuleReads, 1);
  assert.equal(calls.cspFetches, 1);
  assert.equal(calls.sessionRuleUpdates.length, 0);
  assert.equal(calls.offscreenDocuments.length, 1);
});

test("bootstrap refreshes the copied CSP on a reusable session rule", async () => {
  const existingRules = [{
    id: 29764,
    priority: 1,
    action: {
      type: "modifyHeaders",
      responseHeaders: [
        {
          header: "content-security-policy",
          operation: "set",
          value: "font-src 'none';"
        },
        { header: "x-frame-options", operation: "remove" }
      ]
    },
    condition: {
      regexFilter: "^https://suno\\.com/create/?(?:[?#].*)?$",
      initiatorDomains: ["abcdefghijklmnopabcdefghijklmnop"],
      resourceTypes: ["sub_frame"],
      tabIds: [-1]
    }
  }];
  const { calls } = serviceWorkerHarness({
    existingRules,
    csp: "frame-ancestors 'none'; font-src 'self'"
  });
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.equal(calls.cspFetches, 1);
  assert.equal(calls.sessionRuleUpdates.length, 1);
  assert.equal(
    calls.sessionRuleUpdates[0].addRules[0].action.responseHeaders[0].value,
    "font-src 'self';"
  );
  assert.equal(calls.offscreenDocuments.length, 1);
});

test("bootstrap preserves every non-embedding directive from multiple CSP policies", async () => {
  const { calls } = serviceWorkerHarness({
    csp: "frame-ancestors 'none'; default-src 'self', script-src 'self'; frame-ancestors 'none'"
  });
  await new Promise((resolve) => setTimeout(resolve, 0));

  const headers = calls.sessionRuleUpdates[0].addRules[0].action.responseHeaders;
  assert.equal(
    headers[0].value,
    "default-src 'self';, script-src 'self';"
  );
});

test("bootstrap leaves CSP untouched when Suno's HEAD response has no CSP", async () => {
  const { calls } = serviceWorkerHarness({ csp: null });
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.equal(calls.sessionRuleUpdates.length, 1);
  assert.deepEqual(
    JSON.parse(JSON.stringify(calls.sessionRuleUpdates[0].addRules[0].action.responseHeaders)),
    [{ header: "x-frame-options", operation: "remove" }]
  );
});

test("bootstrap rejects a redirect away from the managed create page", async () => {
  const { calls } = serviceWorkerHarness({
    responseUrl: "https://suno.com/"
  });
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.equal(calls.sessionRuleUpdates.length, 0);
  assert.equal(calls.offscreenDocuments.length, 0);
});

test("bootstrap removes a legacy 0.2 managed popup exactly once", async () => {
  const { calls, listeners } = serviceWorkerHarness({
    legacyTabs: [{
      id: 71,
      url: "https://suno.com/create#sunox-browser-bridge",
      windowId: 72
    }]
  });
  await new Promise((resolve) => setTimeout(resolve, 0));
  listeners.alarm({ name: "sunox-bridge-poll" });
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.deepEqual(calls.removedTabs, [71]);
  assert.equal(serviceWorkerSource.includes("chrome.windows.remove"), false);
});

test("bootstrap reuses a responsive existing offscreen document", async () => {
  const { calls } = serviceWorkerHarness({ offscreenExists: true });
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.equal(calls.sessionRuleUpdates.length, 1);
  assert.equal(calls.offscreenDocuments.length, 0);
  assert.equal(calls.closedOffscreenDocuments, 0);
  assert.equal(calls.heartbeats, 1);
});

test("bootstrap replaces an unresponsive offscreen document", async () => {
  const { calls } = serviceWorkerHarness({
    offscreenExists: true,
    offscreenReady: false
  });
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.equal(calls.closedOffscreenDocuments, 1);
  assert.equal(calls.offscreenDocuments.length, 1);
});

test("completed bootstrap is not cached across alarm health checks", async () => {
  const { calls, listeners } = serviceWorkerHarness();
  await new Promise((resolve) => setTimeout(resolve, 0));
  listeners.alarm({ name: "sunox-bridge-poll" });
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.equal(calls.offscreenDocuments.length, 1);
  assert.equal(calls.heartbeats, 1);
});

test("service worker relays only validated offscreen and managed-frame messages", async () => {
  const { calls, listeners } = serviceWorkerHarness();
  await new Promise((resolve) => setTimeout(resolve, 0));

  let frameMessageListener;
  const postedToFrame = [];
  const port = {
    name: "sunox-managed-frame-v1",
    sender: {
      documentId: "managed-document",
      id: "abcdefghijklmnopabcdefghijklmnop",
      origin: "https://suno.com",
      url: "https://suno.com/create/"
    },
    disconnect() {},
    postMessage(message) {
      postedToFrame.push(message);
    },
    onDisconnect: {
      addListener(listener) {
        port.disconnectListener = listener;
      }
    },
    onMessage: {
      addListener(listener) {
        frameMessageListener = listener;
      }
    }
  };
  listeners.connect(port);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(calls.runtimeMessages.at(-1).type, "sunox-managed-frame-ready-v1");

  const rejectedMessages = [];
  let competingDisconnected = false;
  listeners.connect({
    ...port,
    sender: {
      ...port.sender,
      documentId: "competing-document"
    },
    disconnect() {
      competingDisconnected = true;
    },
    postMessage(message) {
      rejectedMessages.push(message);
    }
  });
  assert.deepEqual(JSON.parse(JSON.stringify(rejectedMessages)), [{
    type: "sunox-managed-frame-rejected-v1"
  }]);
  assert.equal(competingDisconnected, true);

  let response;
  listeners.runtimeMessage(
    {
      type: "sunox-managed-frame-execute-v1",
      requestId: "request-1",
      provider: "hcaptcha",
      transportReceipt: "must-not-cross"
    },
    {
      id: "abcdefghijklmnopabcdefghijklmnop",
      url: "chrome-extension://abcdefghijklmnopabcdefghijklmnop/offscreen.html"
    },
    (value) => {
      response = value;
    }
  );
  assert.equal(response.accepted, true);
  assert.deepEqual(JSON.parse(JSON.stringify(postedToFrame)), [{
    type: "sunox-managed-frame-execute-v1",
    requestId: "request-1",
    provider: "hcaptcha"
  }]);

  frameMessageListener({
    type: "sunox-managed-frame-result-v1",
    requestId: "request-1",
    token: "token-value",
    transportReceipt: "must-not-cross"
  });
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.deepEqual(JSON.parse(JSON.stringify(calls.runtimeMessages.at(-1))), {
    type: "sunox-managed-frame-result-v1",
    requestId: "request-1",
    token: "token-value",
    error: null
  });
});

test("content bridge relays a managed-frame request and bounded page result", async () => {
  const windowListeners = new Map();
  const postedToServiceWorker = [];
  let portMessageListener;
  let realmWindow;
  const context = {
    chrome: {
      runtime: {
        connect() {
          return {
            disconnect() {},
            onDisconnect: {
              addListener() {}
            },
            onMessage: {
              addListener(listener) {
                portMessageListener = listener;
              }
            },
            postMessage(message) {
              postedToServiceWorker.push(message);
            }
          };
        }
      }
    },
    clearTimeout,
    location: {
      hostname: "suno.com",
      origin: "https://suno.com"
    },
    Promise,
    setTimeout(callback, delay) {
      const timer = setTimeout(callback, delay);
      timer.unref?.();
      return timer;
    }
  };
  context.window = context;
  context.top = {};
  context.parent = context.top;
  context.addEventListener = (type, listener) => {
    windowListeners.set(type, listener);
  };
  context.removeEventListener = (type, listener) => {
    if (windowListeners.get(type) === listener) windowListeners.delete(type);
  };
  context.postMessage = (message) => {
    if (message?.source !== "sunox-extension-v1") return;
    queueMicrotask(() => {
      windowListeners.get("message")?.({
        data: {
          source: "sunox-page-v1",
          requestId: message.requestId,
          token: "bridge-token"
        },
        origin: "https://suno.com",
        source: realmWindow
      });
    });
  };
  context.globalThis = context;
  vm.createContext(context);
  vm.runInContext(bridgeSource, context);
  realmWindow = vm.runInContext("window", context);

  portMessageListener({
    type: "sunox-managed-frame-execute-v1",
    requestId: "request-bridge",
    provider: "hcaptcha"
  });
  await flushAsync();

  assert.deepEqual(JSON.parse(JSON.stringify(postedToServiceWorker)), [{
    type: "sunox-managed-frame-result-v1",
    requestId: "request-bridge",
    token: "bridge-token",
    error: null
  }]);
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
      }
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
    ["expired-callback", undefined, /Turnstile token expired/],
    ["unsupported-callback", undefined, /Turnstile is unsupported/]
  ];

  for (const [callbackName, callbackArgument, expectedError] of cases) {
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
    assert.match(pageResults[0].error, expectedError);
  }
});

test("page bridge allows Turnstile recovery and tracks interactive deadlines", async () => {
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
          options["error-callback"]("network");
          options["before-interactive-callback"]();
          options["error-callback"]("interactive-network");
          options["timeout-callback"]();
          options["after-interactive-callback"]();
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

  assert.deepEqual(
    timeoutDelays,
    [15_000, 15_000, 120_000, 120_000, 15_000]
  );
  assert.deepEqual(JSON.parse(JSON.stringify(pageResults)), [{
    source: "sunox-page-v1",
    requestId: "request-turnstile-recovery",
    token: "recovered-turnstile-token"
  }]);
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
      origin: "https://suno.com"
    },
    Promise,
    setInterval,
    setTimeout(callback, delay) {
      return setTimeout(callback, delay === 25_000 ? 0 : delay);
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
  assert.match(pageResults[0].error, /produced no token/);
  assert.deepEqual(JSON.parse(JSON.stringify(pageResults[1])), {
    source: "sunox-page-v1",
    requestId: "request-recovered",
    token: "recovered-token"
  });
});

test("offscreen context claims, executes, and submits a challenge without a tab", async () => {
  const runtimeListeners = new Set();
  const submitted = [];
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
          if (message.type !== "sunox-managed-frame-execute-v1") {
            return undefined;
          }
          queueMicrotask(() => {
            dispatchRuntimeMessage({
              type: "sunox-managed-frame-result-v1",
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
        appendChild() {
          dispatchRuntimeMessage({ type: "sunox-managed-frame-ready-v1" });
        }
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
  await flushAsync();
  await flushAsync();

  assert.equal(claims, 1);
  assert.deepEqual(JSON.parse(JSON.stringify(submitted)), [{
    transportReceipt: "receipt",
    requestId: "request-offscreen",
    token: "offscreen-token",
    error: null
  }]);
});

test("offscreen context replaces a failed provider frame before the next challenge", async () => {
  const runtimeListeners = new Set();
  const submitted = [];
  const challenges = [
    {
      provider: "turnstile",
      requestId: "request-failed",
      transportReceipt: "receipt-failed"
    },
    {
      provider: "hcaptcha",
      requestId: "request-recovered",
      transportReceipt: "receipt-recovered"
    }
  ];
  let claimIndex = 0;
  let framesCreated = 0;
  let framesRemoved = 0;
  let now = 0;
  let pollWorkerListener;
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
          if (message.type !== "sunox-managed-frame-execute-v1") {
            return undefined;
          }
          queueMicrotask(() => {
            dispatchRuntimeMessage({
              type: "sunox-managed-frame-result-v1",
              requestId: message.requestId,
              token: message.requestId === "request-recovered"
                ? "recovered-token"
                : null,
              error: message.requestId === "request-failed"
                ? "Turnstile produced no token"
                : null
            });
          });
          return { accepted: true };
        }
      }
    },
    clearTimeout,
    crypto,
    Date: {
      now() {
        now += 1000;
        return now;
      }
    },
    document: {
      body: {
        appendChild() {
          dispatchRuntimeMessage({ type: "sunox-managed-frame-ready-v1" });
        }
      },
      createElement() {
        framesCreated += 1;
        return {
          remove() {
            framesRemoved += 1;
          },
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
          return challenges[claimIndex++] || null;
        },
        async submitResult(message) {
          submitted.push(message);
          return { accepted: true };
        }
      }
    },
    Worker: class {
      addEventListener(type, listener) {
        if (type === "message") pollWorkerListener = listener;
      }
    }
  };
  context.globalThis = context;
  vm.createContext(context);
  vm.runInContext(offscreenSource, context);
  await flushAsync();
  await flushAsync();

  assert.equal(framesCreated, 1);
  assert.equal(framesRemoved, 1);
  assert.equal(submitted[0].token, null);
  assert.match(submitted[0].error, /Turnstile produced no token/);

  pollWorkerListener({ data: { type: "sunox-poll" } });
  await flushAsync();
  await flushAsync();

  assert.equal(framesCreated, 2);
  assert.equal(framesRemoved, 1);
  assert.equal(submitted[1].token, "recovered-token");
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

test("manifest and frame scripts expose only the invisible iframe transport", () => {
  assert.equal(manifest.version, "0.3.5");
  assert.equal(manifest.version_name, "__SUNOX_VERSION__");
  assert.ok(manifest.permissions.includes("declarativeNetRequestFeedback"));
  assert.ok(manifest.permissions.includes("declarativeNetRequestWithHostAccess"));
  assert.equal(manifest.permissions.includes("activeTab"), false);
  assert.ok(manifest.permissions.includes("offscreen"));
  assert.equal(manifest.permissions.includes("tabs"), false);
  assert.equal(manifest.permissions.includes("storage"), false);
  assert.ok(manifest.content_scripts.every((script) => script.all_frames === true));
  for (const path of [
    "/v2/challenge/hello",
    "/v2/challenge/claim",
    "/v2/challenge/result"
  ]) {
    assert.ok(loopbackTransportSource.includes(path));
  }
  for (const context of [
    "sunox-bridge-server-v2",
    "sunox-bridge-client-v2",
    "sunox-bridge-result-v2",
    "sunox-bridge-receipt-v2"
  ]) {
    assert.ok(loopbackTransportSource.includes(context));
  }
  assert.equal(loopbackTransportSource.includes("/v1/challenge/"), false);
  assert.equal(loopbackTransportSource.includes("sunox-bridge-receipt-v1"), false);

  assert.ok(offscreenSource.includes('document.createElement("iframe")'));
  assert.ok(serviceWorkerSource.includes('port.name !== "sunox-managed-frame-v1"'));
  assert.ok(offscreenSource.includes('managedFrame.sandbox.add("allow-forms"'));
  assert.ok(serviceWorkerSource.includes("isManagedFrameSender(port.sender)"));
  assert.ok(serviceWorkerSource.includes("isOffscreenSender(sender)"));
  assert.ok(offscreenSource.includes("sunox-managed-frame-execute-v1"));
  assert.ok(offscreenSource.includes("sunox-managed-frame-result-v1"));
  assert.ok(bridgeSource.includes('chrome.runtime.connect({ name: "sunox-managed-frame-v1" })'));
  assert.ok(bridgeSource.includes("sunox-managed-frame-execute-v1"));
  assert.ok(bridgeSource.includes("sunox-managed-frame-result-v1"));
  const sdkReadyTimeout = numericConstant(
    pageSource,
    "CHALLENGE_SDK_READY_TIMEOUT_MS"
  );
  const interactiveTimeout = numericConstant(
    pageSource,
    "TURNSTILE_INTERACTIVE_TIMEOUT_MS"
  );
  const idleTimeout = numericConstant(pageSource, "TURNSTILE_IDLE_TIMEOUT_MS");
  const pageTimeout = numericConstant(bridgeSource, "challengePageTimeoutMs");
  const frameReadyTimeout = numericConstant(offscreenSource, "frameReadyTimeoutMs");
  const frameResultTimeout = numericConstant(offscreenSource, "challengeTimeoutMs");
  assert.ok(
    pageTimeout > sdkReadyTimeout + 3 * idleTimeout + 2 * interactiveTimeout
  );
  assert.ok(frameResultTimeout > pageTimeout);
  assert.equal(frameReadyTimeout, 20_000);
  assert.ok(pageSource.includes("25_000"));
  assert.ok(pageSource.includes("window.parent !== window.top"));
  assert.ok(bridgeSource.includes("window.parent !== window.top"));
  assert.equal(offscreenSource.includes("contentWindow.postMessage"), false);
  assert.equal(offscreenSource.includes("chrome.windows"), false);
  assert.equal(offscreenSource.includes("chrome.tabs"), false);
});
