const POLL_ALARM = "sunox-bridge-poll";
const OFFSCREEN_PATH = "offscreen.html";
const EMBED_RULE_ID = 29_764;
const MANAGED_PAGE_URL = "https://suno.com/create";
const MANAGED_PAGE_FILTER = "^https://suno\\.com/create/?(?:[?#].*)?$";
const MANAGED_FRAME_ORIGIN = "https://suno.com";
const EMBED_RULE_REFRESH_MS = 15 * 60 * 1000;
const INITIAL_RULE_FETCH_TIMEOUT_MS = 8_000;
const REUSED_RULE_REFRESH_TIMEOUT_MS = 2_000;
const OFFSCREEN_PING_TIMEOUT_MS = 1_500;
const MAX_TOKEN_LENGTH = 16_384;
let creatingOffscreenDocument;
let bootstrapPromise;
let embedRuleReady = false;
let embedRuleCheckedAt = 0;
let legacyCleanupDone = false;
let managedFramePort;
let managedFrameDocumentId;

function cspWithoutFrameAncestors(value) {
  if (typeof value !== "string") return null;
  const policies = value
    .split(",")
    .map((policy) => policy
      .split(";")
      .map((directive) => directive.trim())
      .filter(Boolean)
      .filter((directive) => !/^frame-ancestors(?:\s|$)/i.test(directive)))
    .filter((directives) => directives.length > 0)
    .map((directives) => `${directives.join("; ")};`);
  return policies.join(", ");
}

function isManagedCreatePath(pathname) {
  return pathname === "/create" || pathname === "/create/";
}

async function currentEmbeddingCsp(signal) {
  const response = await fetch(MANAGED_PAGE_URL, {
    method: "HEAD",
    cache: "no-store",
    credentials: "include",
    redirect: "follow",
    signal
  });
  if (!response.ok) {
    throw new Error(`Could not inspect the Suno embedding policy (HTTP ${response.status})`);
  }
  const finalUrl = new URL(response.url);
  if (
    finalUrl.origin !== MANAGED_FRAME_ORIGIN
    || !isManagedCreatePath(finalUrl.pathname)
  ) {
    throw new Error(`Suno redirected the managed challenge page to ${finalUrl.origin}${finalUrl.pathname}`);
  }
  const original = response.headers.get("content-security-policy");
  return cspWithoutFrameAncestors(original);
}

function hasExactValues(actual, expected) {
  return Array.isArray(actual)
    && actual.length === expected.length
    && actual.every((value, index) => value === expected[index]);
}

function hasExactKeys(value, expected) {
  if (!value || typeof value !== "object") return false;
  return hasExactValues(Object.keys(value).sort(), [...expected].sort());
}

function isReusableEmbedRule(rule) {
  if (
    rule?.id !== EMBED_RULE_ID
    || rule.priority !== 1
    || !hasExactKeys(rule, ["id", "priority", "action", "condition"])
    || !hasExactKeys(rule.action, ["type", "responseHeaders"])
    || rule.action.type !== "modifyHeaders"
    || !hasExactKeys(rule.condition, [
      "initiatorDomains",
      "regexFilter",
      "resourceTypes",
      "tabIds"
    ])
    || rule.condition.regexFilter !== MANAGED_PAGE_FILTER
    || !hasExactValues(rule.condition.initiatorDomains, [chrome.runtime.id])
    || !hasExactValues(rule.condition.resourceTypes, ["sub_frame"])
    || !hasExactValues(rule.condition.tabIds, [chrome.tabs.TAB_ID_NONE])
    || !Array.isArray(rule.action.responseHeaders)
    || ![1, 2].includes(rule.action.responseHeaders.length)
  ) return false;

  let sawContentSecurityPolicy = false;
  let sawXFrameOptions = false;
  for (const header of rule.action.responseHeaders) {
    if (!header || typeof header.header !== "string") return false;
    const name = header.header.toLowerCase();
    if (name === "x-frame-options") {
      if (sawXFrameOptions || header.operation !== "remove" || header.value !== undefined) {
        return false;
      }
      sawXFrameOptions = true;
      continue;
    }
    if (name === "content-security-policy") {
      if (sawContentSecurityPolicy) return false;
      const removesHeader = header.operation === "remove" && header.value === undefined;
      const setsHeader = header.operation === "set"
        && typeof header.value === "string"
        && header.value.length > 0;
      if (!removesHeader && !setsHeader) return false;
      sawContentSecurityPolicy = true;
      continue;
    }
    return false;
  }
  return sawXFrameOptions;
}

async function installEmbedRule(preservedCsp) {
  const responseHeaders = [
    { header: "x-frame-options", operation: "remove" }
  ];
  if (preservedCsp !== null) {
    responseHeaders.unshift(
      preservedCsp
        ? {
          header: "content-security-policy",
          operation: "set",
          value: preservedCsp
        }
        : { header: "content-security-policy", operation: "remove" }
    );
  }
  await chrome.declarativeNetRequest.updateSessionRules({
    removeRuleIds: [EMBED_RULE_ID],
    addRules: [{
      id: EMBED_RULE_ID,
      priority: 1,
      action: {
        type: "modifyHeaders",
        responseHeaders
      },
      condition: {
        regexFilter: MANAGED_PAGE_FILTER,
        initiatorDomains: [chrome.runtime.id],
        resourceTypes: ["sub_frame"],
        tabIds: [chrome.tabs.TAB_ID_NONE]
      }
    }]
  });
}

async function embeddingCspWithTimeout(timeoutMs) {
  const controller = new AbortController();
  const timeout = setTimeout(
    () => controller.abort(),
    timeoutMs
  );
  try {
    return await currentEmbeddingCsp(controller.signal);
  } finally {
    clearTimeout(timeout);
  }
}

async function refreshReusableEmbedRule() {
  const preservedCsp = await embeddingCspWithTimeout(REUSED_RULE_REFRESH_TIMEOUT_MS);
  await installEmbedRule(preservedCsp);
}

async function ensureEmbedRule() {
  if (
    embedRuleReady
    && Date.now() - embedRuleCheckedAt < EMBED_RULE_REFRESH_MS
  ) return;
  try {
    const rules = await chrome.declarativeNetRequest.getSessionRules();
    if (rules.some(isReusableEmbedRule)) {
      // Keep a known-good session rule usable across service-worker restarts,
      // but refresh its copied CSP on a bounded best-effort schedule.
      await refreshReusableEmbedRule().catch(() => {});
      embedRuleReady = true;
      embedRuleCheckedAt = Date.now();
      return;
    }
  } catch {
    // A fresh rule can still be installed when session-rule inspection fails.
  }

  const preservedCsp = await embeddingCspWithTimeout(INITIAL_RULE_FETCH_TIMEOUT_MS);
  await installEmbedRule(preservedCsp);
  embedRuleReady = true;
  embedRuleCheckedAt = Date.now();
}

async function offscreenResponds() {
  let timeout;
  try {
    const response = await Promise.race([
      chrome.runtime.sendMessage({ type: "sunox-offscreen-ping-v1" }),
      new Promise((_, reject) => {
        timeout = setTimeout(
          () => reject(new Error("Offscreen heartbeat timed out")),
          OFFSCREEN_PING_TIMEOUT_MS
        );
      })
    ]);
    return response?.type === "sunox-offscreen-pong-v1";
  } catch {
    return false;
  } finally {
    clearTimeout(timeout);
  }
}

async function ensureOffscreenDocument() {
  const documentUrl = chrome.runtime.getURL(OFFSCREEN_PATH);
  const contexts = await chrome.runtime.getContexts({
    contextTypes: ["OFFSCREEN_DOCUMENT"],
    documentUrls: [documentUrl]
  });
  if (contexts.length > 0) {
    if (await offscreenResponds()) return;
    await chrome.offscreen.closeDocument().catch(() => {});
  }

  if (!creatingOffscreenDocument) {
    creatingOffscreenDocument = chrome.offscreen.createDocument({
      url: OFFSCREEN_PATH,
      reasons: ["IFRAME_SCRIPTING", "WORKERS"],
      justification: "Run the local Sunox listener and an invisible Suno challenge iframe without opening a browser tab or window."
    }).finally(() => {
      creatingOffscreenDocument = null;
    });
  }
  await creatingOffscreenDocument;
}

async function ensureLegacyContextCleaned() {
  if (legacyCleanupDone) return;
  const tabs = await chrome.tabs.query({ url: "https://suno.com/*" });
  const tabIds = tabs
    .filter((tab) => (
      tab.url === "https://suno.com/create#sunox-browser-bridge"
      && Number.isInteger(tab.id)
    ))
    .map((tab) => tab.id);
  await Promise.allSettled(
    tabIds.map((tabId) => chrome.tabs.remove(tabId))
  );
  legacyCleanupDone = true;
}

async function ensurePollAlarm() {
  if (await chrome.alarms.get(POLL_ALARM)) return;
  await chrome.alarms.create(POLL_ALARM, {
    delayInMinutes: 0.5,
    periodInMinutes: 0.5
  });
}

function isManagedFrameSender(sender) {
  if (
    sender?.id !== chrome.runtime.id
    || sender.tab
    || (
      typeof sender.origin === "string"
      && sender.origin !== MANAGED_FRAME_ORIGIN
    )
    || typeof sender.url !== "string"
  ) return false;
  try {
    const url = new URL(sender.url);
    return url.origin === MANAGED_FRAME_ORIGIN && isManagedCreatePath(url.pathname);
  } catch {
    return false;
  }
}

function isOffscreenSender(sender) {
  return sender?.id === chrome.runtime.id
    && !sender.tab
    && sender.url === chrome.runtime.getURL(OFFSCREEN_PATH);
}

function notifyOffscreen(message) {
  chrome.runtime.sendMessage(message).catch(() => {});
}

chrome.runtime.onConnect.addListener((port) => {
  if (port.name !== "sunox-managed-frame-v1") {
    return;
  }
  if (!isManagedFrameSender(port.sender)) {
    port.disconnect();
    return;
  }

  if (
    managedFramePort
    && (
      !port.sender.documentId
      || !managedFrameDocumentId
      || port.sender.documentId !== managedFrameDocumentId
    )
  ) {
    port.postMessage({ type: "sunox-managed-frame-rejected-v1" });
    port.disconnect();
    return;
  }
  managedFramePort?.disconnect();
  managedFramePort = port;
  managedFrameDocumentId = port.sender.documentId;
  notifyOffscreen({ type: "sunox-managed-frame-ready-v1" });

  port.onMessage.addListener((message) => {
    if (
      message?.type !== "sunox-managed-frame-result-v1"
      || typeof message.requestId !== "string"
    ) return;
    const token = typeof message.token === "string"
      && message.token.length > 0
      && message.token.length <= MAX_TOKEN_LENGTH
      ? message.token
      : null;
    notifyOffscreen({
      type: "sunox-managed-frame-result-v1",
      requestId: message.requestId,
      token,
      error: token
        ? null
        : typeof message.error === "string" && message.error
          ? message.error.slice(0, 900)
          : "Managed Suno frame returned an invalid challenge token"
    });
  });

  port.onDisconnect.addListener(() => {
    if (managedFramePort !== port) return;
    managedFramePort = null;
    managedFrameDocumentId = null;
    notifyOffscreen({ type: "sunox-managed-frame-disconnected-v1" });
  });
});

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (
    message?.type !== "sunox-managed-frame-execute-v1"
    || !isOffscreenSender(sender)
  ) return false;
  if (
    !managedFramePort
    || typeof message.requestId !== "string"
    || !["hcaptcha", "turnstile"].includes(message.provider)
  ) {
    sendResponse({ accepted: false });
    return false;
  }
  try {
    managedFramePort.postMessage({
      type: "sunox-managed-frame-execute-v1",
      requestId: message.requestId,
      provider: message.provider
    });
    sendResponse({ accepted: true });
  } catch {
    managedFramePort = null;
    managedFrameDocumentId = null;
    sendResponse({ accepted: false });
  }
  return false;
});

async function bootstrap() {
  await ensurePollAlarm();
  await ensureLegacyContextCleaned();
  await ensureEmbedRule();
  await ensureOffscreenDocument();
}

function ensureBootstrapped() {
  if (!bootstrapPromise) {
    bootstrapPromise = bootstrap().finally(() => {
      bootstrapPromise = null;
    });
  }
  return bootstrapPromise;
}

chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === POLL_ALARM) ensureBootstrapped().catch(() => {});
});
chrome.runtime.onInstalled.addListener(() => ensureBootstrapped().catch(() => {}));
chrome.runtime.onStartup.addListener(() => ensureBootstrapped().catch(() => {}));
ensureBootstrapped().catch(() => {});
