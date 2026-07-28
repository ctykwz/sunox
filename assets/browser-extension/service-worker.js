const POLL_ALARM = "sunox-bridge-poll";
const OFFSCREEN_PATH = "offscreen.html";
const SUNO_FRAME_RULE_ID = 29_764;
const CLERK_FRAME_RULE_ID = 29_765;
const FRAME_RULE_IDS = [SUNO_FRAME_RULE_ID, CLERK_FRAME_RULE_ID];
const MANAGED_PAGE_URL = "https://suno.com/create";
const MANAGED_PAGE_ORIGIN = "https://suno.com";
const MANAGED_PAGE_FILTER = "^https://suno\\.com/create/?(?:[?#].*)?$";
const MANAGED_PAGE_HASH_PREFIX = "#sunox-browser-bridge=";
const CLERK_HANDSHAKE_FILTER =
  "^https://auth\\.suno\\.com/v1/client/handshake(?:\\?.*)?$";
const SUNO_FRAME_INITIATORS = [
  chrome.runtime.id,
  "auth.suno.com",
  "suno.com"
];
const CLERK_FRAME_INITIATORS = [
  chrome.runtime.id,
  "suno.com"
];
const MANAGED_NONCE_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const EMBED_RULE_REFRESH_MS = 15 * 60 * 1000;
const INITIAL_RULE_FETCH_TIMEOUT_MS = 8_000;
const REUSED_RULE_REFRESH_TIMEOUT_MS = 2_000;
const OFFSCREEN_PING_TIMEOUT_MS = 1_500;
const OFFSCREEN_POLL_MAX_AGE_MS = 5_000;
const OFFSCREEN_BUSY_MAX_AGE_MS = 125_000;
const MAX_TOKEN_LENGTH = 16_384;
const MANAGED_CHALLENGE_ERROR_MESSAGES = Object.freeze({
  challenge_expired:
    "silent_challenge_unavailable: the challenge token expired",
  challenge_failed:
    "silent_challenge_unavailable: the challenge provider failed",
  challenge_sdk_unavailable:
    "silent_challenge_unavailable: the challenge SDK did not become ready",
  challenge_timeout:
    "silent_challenge_unavailable: the challenge did not finish before its deadline",
  interactive_browser_required:
    "interactive_browser_required: the challenge requires visible browser interaction",
  invalid_challenge_token:
    "Managed Suno frame returned an invalid challenge token",
  page_not_ready:
    "Managed Suno frame was not canonical and ready for challenge execution",
  page_unavailable:
    "Managed Suno frame was busy or unavailable",
  silent_challenge_unavailable:
    "silent_challenge_unavailable: the challenge could not complete silently",
  unsupported_browser:
    "silent_challenge_unavailable: the challenge provider is unsupported in this browser"
});
let creatingOffscreenDocument;
let bootstrapPromise;
let frameEnvironmentPromise;
let embedRuleReady = false;
let embedRuleCheckedAt = 0;
let environmentReady = false;
let managedFrame;

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

function managedPageDetails(value) {
  if (typeof value !== "string" || value.length > 131_072) return null;
  try {
    const url = new URL(value);
    if (
      url.origin !== MANAGED_PAGE_ORIGIN
      || !["/create", "/create/"].includes(url.pathname)
      || url.username
      || url.password
      || url.search
      || !url.hash.startsWith(MANAGED_PAGE_HASH_PREFIX)
    ) return null;
    const nonce = url.hash.slice(MANAGED_PAGE_HASH_PREFIX.length);
    return MANAGED_NONCE_PATTERN.test(nonce) ? { nonce } : null;
  } catch {
    return null;
  }
}

function hasExactValues(actual, expected) {
  return Array.isArray(actual)
    && actual.length === expected.length
    && actual.every((value, index) => value === expected[index]);
}

function hasExactKeys(value, expected) {
  return value
    && typeof value === "object"
    && hasExactValues(Object.keys(value).sort(), [...expected].sort());
}

function isReusableSunoRule(rule) {
  if (
    rule?.id !== SUNO_FRAME_RULE_ID
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
    || !hasExactValues(
      rule.condition.initiatorDomains,
      SUNO_FRAME_INITIATORS
    )
    || !hasExactValues(rule.condition.resourceTypes, ["sub_frame"])
    || !hasExactValues(rule.condition.tabIds, [chrome.tabs.TAB_ID_NONE])
    || !Array.isArray(rule.action.responseHeaders)
    || ![1, 2].includes(rule.action.responseHeaders.length)
  ) return false;

  let sawCsp = false;
  let sawXFrameOptions = false;
  for (const header of rule.action.responseHeaders) {
    const name = header?.header?.toLowerCase();
    if (name === "x-frame-options") {
      if (sawXFrameOptions || header.operation !== "remove") return false;
      sawXFrameOptions = true;
    } else if (name === "content-security-policy") {
      const valid = header.operation === "remove"
        || (
          header.operation === "set"
          && typeof header.value === "string"
          && header.value.length > 0
        );
      if (sawCsp || !valid) return false;
      sawCsp = true;
    } else {
      return false;
    }
  }
  return sawXFrameOptions;
}

function isReusableClerkRule(rule) {
  return rule?.id === CLERK_FRAME_RULE_ID
    && rule.priority === 1
    && hasExactKeys(rule, ["id", "priority", "action", "condition"])
    && hasExactKeys(rule.action, ["type", "responseHeaders"])
    && rule.action.type === "modifyHeaders"
    && Array.isArray(rule.action.responseHeaders)
    && rule.action.responseHeaders.length === 1
    && hasExactKeys(
      rule.action.responseHeaders[0],
      ["header", "operation"]
    )
    && rule.action.responseHeaders[0].header.toLowerCase()
      === "x-frame-options"
    && rule.action.responseHeaders[0].operation === "remove"
    && hasExactKeys(rule.condition, [
      "initiatorDomains",
      "regexFilter",
      "resourceTypes",
      "tabIds"
    ])
    && rule.condition.regexFilter === CLERK_HANDSHAKE_FILTER
    && hasExactValues(
      rule.condition.initiatorDomains,
      CLERK_FRAME_INITIATORS
    )
    && hasExactValues(rule.condition.resourceTypes, ["sub_frame"])
    && hasExactValues(rule.condition.tabIds, [chrome.tabs.TAB_ID_NONE]);
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
    throw new Error(
      `Could not inspect the Suno embedding policy (HTTP ${response.status})`
    );
  }
  const finalUrl = new URL(response.url);
  if (
    finalUrl.origin !== MANAGED_PAGE_ORIGIN
    || !["/create", "/create/"].includes(finalUrl.pathname)
    || finalUrl.username
    || finalUrl.password
    || finalUrl.search
    || finalUrl.hash
  ) {
    throw new Error(
      "Suno redirected the managed challenge page away from the clean canonical URL"
    );
  }
  const currentCsp = response.headers.get("content-security-policy");
  if (typeof currentCsp !== "string" || currentCsp.trim().length === 0) {
    throw new Error("Suno returned no current content security policy");
  }
  const preservedCsp = cspWithoutFrameAncestors(currentCsp);
  if (!preservedCsp) {
    throw new Error(
      "Suno's current content security policy cannot be preserved safely"
    );
  }
  return preservedCsp;
}

async function embeddingCspWithTimeout(timeoutMs) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), timeoutMs);
  try {
    return await currentEmbeddingCsp(controller.signal);
  } finally {
    clearTimeout(timeout);
  }
}

function frameRules(preservedCsp) {
  if (typeof preservedCsp !== "string" || preservedCsp.length === 0) {
    throw new Error("A current non-empty Suno CSP is required");
  }
  const sunoHeaders = [
    {
      header: "content-security-policy",
      operation: "set",
      value: preservedCsp
    },
    { header: "x-frame-options", operation: "remove" }
  ];
  return [{
    id: SUNO_FRAME_RULE_ID,
    priority: 1,
    action: {
      type: "modifyHeaders",
      responseHeaders: sunoHeaders
    },
    condition: {
      regexFilter: MANAGED_PAGE_FILTER,
      initiatorDomains: SUNO_FRAME_INITIATORS,
      resourceTypes: ["sub_frame"],
      tabIds: [chrome.tabs.TAB_ID_NONE]
    }
  }, {
    id: CLERK_FRAME_RULE_ID,
    priority: 1,
    action: {
      type: "modifyHeaders",
      responseHeaders: [
        { header: "x-frame-options", operation: "remove" }
      ]
    },
    condition: {
      regexFilter: CLERK_HANDSHAKE_FILTER,
      initiatorDomains: CLERK_FRAME_INITIATORS,
      resourceTypes: ["sub_frame"],
      tabIds: [chrome.tabs.TAB_ID_NONE]
    }
  }];
}

async function installFrameRules(preservedCsp) {
  await chrome.declarativeNetRequest.updateDynamicRules({
    removeRuleIds: FRAME_RULE_IDS
  });
  await chrome.declarativeNetRequest.updateSessionRules({
    removeRuleIds: FRAME_RULE_IDS,
    addRules: frameRules(preservedCsp)
  });
}

async function clearFrameRules() {
  embedRuleReady = false;
  embedRuleCheckedAt = 0;
  const results = await Promise.allSettled([
    chrome.declarativeNetRequest.updateDynamicRules({
      removeRuleIds: FRAME_RULE_IDS
    }),
    chrome.declarativeNetRequest.updateSessionRules({
      removeRuleIds: FRAME_RULE_IDS
    })
  ]);
  const failed = results.find((result) => result.status === "rejected");
  if (failed) throw failed.reason;
}

function ensureFrameEnvironment() {
  if (!frameEnvironmentPromise) {
    frameEnvironmentPromise = ensureFrameRules()
      .then(() => {
        environmentReady = true;
      })
      .catch((error) => {
        environmentReady = false;
        throw error;
      })
      .finally(() => {
        frameEnvironmentPromise = null;
      });
  }
  return frameEnvironmentPromise;
}

async function ensureFrameRules() {
  if (
    embedRuleReady
    && Date.now() - embedRuleCheckedAt < EMBED_RULE_REFRESH_MS
  ) return;
  const rules = await chrome.declarativeNetRequest
    .getSessionRules()
    .catch(() => []);
  try {
    const preservedCsp = await embeddingCspWithTimeout(
      rules.some(isReusableSunoRule)
        && rules.some(isReusableClerkRule)
        ? REUSED_RULE_REFRESH_TIMEOUT_MS
        : INITIAL_RULE_FETCH_TIMEOUT_MS
    );
    await installFrameRules(preservedCsp);
    embedRuleReady = true;
    embedRuleCheckedAt = Date.now();
    return;
  } catch (error) {
    try {
      await clearFrameRules();
    } catch (cleanupError) {
      throw new AggregateError(
        [error, cleanupError],
        "Suno frame policy refresh and fail-closed cleanup both failed"
      );
    }
    throw error;
  }
}

function isOffscreenSender(sender) {
  return sender?.id === chrome.runtime.id
    && !sender.tab
    && sender.url === chrome.runtime.getURL(OFFSCREEN_PATH);
}

function managedChallengeErrorMessage(errorCode) {
  return typeof errorCode === "string"
    && Object.hasOwn(MANAGED_CHALLENGE_ERROR_MESSAGES, errorCode)
    ? MANAGED_CHALLENGE_ERROR_MESSAGES[errorCode]
    : MANAGED_CHALLENGE_ERROR_MESSAGES.challenge_failed;
}

function managedFrameSenderDetails(sender) {
  if (
    sender?.id !== chrome.runtime.id
    || sender.tab
    || sender.origin !== MANAGED_PAGE_ORIGIN
    || sender.frameId === 0
  ) return null;
  return managedPageDetails(sender.url);
}

function managedFrameSenderRejectionReason(sender) {
  if (sender?.id !== chrome.runtime.id) return "extension_id_mismatch";
  if (sender.tab) return "sender_has_tab";
  if (sender.origin !== MANAGED_PAGE_ORIGIN) return "origin_mismatch";
  if (sender.frameId === 0) return "top_level_frame";
  return managedPageDetails(sender.url)
    ? "unknown_sender_mismatch"
    : "managed_url_invalid";
}

function notifyOffscreen(message) {
  chrome.runtime.sendMessage(message).catch(() => {});
}

function rejectManagedFramePort(
  port,
  permanent = true,
  reason = "managed_frame_rejected"
) {
  const nonce = managedPageDetails(port.sender?.url)?.nonce;
  if (permanent && nonce) {
    notifyOffscreen({
      type: "sunox-managed-frame-diagnostic-v1",
      nonce,
      reason
    });
  }
  if (permanent) {
    try {
      port.postMessage({ type: "sunox-managed-frame-rejected-v2" });
    } catch {}
  }
  try {
    port.disconnect();
  } catch {}
}

function bindManagedFramePort(port) {
  const details = managedFrameSenderDetails(port.sender);
  if (!details) {
    rejectManagedFramePort(
      port,
      true,
      managedFrameSenderRejectionReason(port.sender)
    );
    return;
  }
  if (!environmentReady) {
    rejectManagedFramePort(port, false);
    return;
  }
  if (managedFrame) {
    rejectManagedFramePort(port, true, "managed_frame_busy");
    return;
  }

  const state = {
    completed: false,
    executing: false,
    nonce: details.nonce,
    port,
    requestId: null
  };
  managedFrame = state;
  port.onMessage.addListener((message) => {
    if (
      managedFrame !== state
      || state.completed
      || !state.executing
      || message?.type !== "sunox-managed-frame-result-v2"
      || message.requestId !== state.requestId
    ) return;
    state.completed = true;
    const token = typeof message.token === "string"
      && message.token.length > 0
      && message.token.length <= MAX_TOKEN_LENGTH
      ? message.token
      : null;
    notifyOffscreen({
      type: "sunox-managed-frame-result-v2",
      nonce: state.nonce,
      requestId: state.requestId,
      token,
      error: token
        ? null
        : managedChallengeErrorMessage(message.errorCode)
    });
  });
  port.onDisconnect.addListener(() => {
    if (managedFrame !== state) return;
    managedFrame = null;
    if (state.executing && !state.completed) {
      notifyOffscreen({
        type: "sunox-managed-frame-disconnected-v2",
        nonce: state.nonce
      });
    }
  });
  notifyOffscreen({
    type: "sunox-managed-frame-ready-v2",
    nonce: state.nonce
  });
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
    if (response?.type !== "sunox-offscreen-pong-v1") return false;
    const pollWorkerHealthy = response.pollWorkerHealthy === true
      && Number.isFinite(response.pollWorkerAgeMs)
      && response.pollWorkerAgeMs >= 0
      && response.pollWorkerAgeMs <= OFFSCREEN_POLL_MAX_AGE_MS;
    const busyAgeMs = response.busy === true
      && Number.isFinite(response.busySince)
      ? Math.max(0, Date.now() - response.busySince)
      : Number.POSITIVE_INFINITY;
    return response.busy === true
      ? busyAgeMs <= OFFSCREEN_BUSY_MAX_AGE_MS
      : pollWorkerHealthy;
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
      justification:
        "Poll the local Sunox listener and run an invisible Suno challenge frame without creating a browser tab or window."
    }).finally(() => {
      creatingOffscreenDocument = null;
    });
  }
  await creatingOffscreenDocument;
}

async function ensurePollAlarm() {
  if (await chrome.alarms.get(POLL_ALARM)) return;
  await chrome.alarms.create(POLL_ALARM, {
    delayInMinutes: 0.5,
    periodInMinutes: 0.5
  });
}

async function bootstrap() {
  environmentReady = false;
  await ensurePollAlarm();
  await ensureOffscreenDocument();
  const response = await chrome.runtime.sendMessage({
    type: "sunox-offscreen-start-v1"
  });
  if (response?.accepted !== true) {
    throw new Error("Offscreen Browser Bridge did not acknowledge polling startup");
  }
  await ensureFrameEnvironment();
}

function ensureBootstrapped() {
  if (!bootstrapPromise) {
    bootstrapPromise = bootstrap().finally(() => {
      bootstrapPromise = null;
    });
  }
  return bootstrapPromise;
}

function startBootstrap(trigger) {
  ensureBootstrapped().catch((error) => {
    environmentReady = false;
    console.error(
      `[Sunox Browser Bridge] Bootstrap failed (${trigger}).`,
      error
    );
  });
}

chrome.runtime.onConnect.addListener((port) => {
  if (port.name === "sunox-managed-frame-v2") bindManagedFramePort(port);
});

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (
    message?.type === "sunox-frame-environment-prepare-v1"
    && isOffscreenSender(sender)
  ) {
    ensureFrameEnvironment()
      .then(() => sendResponse({ accepted: true }))
      .catch(() => sendResponse({
        accepted: false,
        error: "challenge_environment_unavailable"
      }));
    return true;
  }
  if (
    message?.type !== "sunox-managed-frame-execute-v2"
    || !isOffscreenSender(sender)
  ) return false;
  if (
    !managedFrame
    || managedFrame.executing
    || message.nonce !== managedFrame.nonce
    || typeof message.requestId !== "string"
    || message.requestId.length === 0
    || message.requestId.length > 128
    || !["hcaptcha", "turnstile"].includes(message.provider)
  ) {
    sendResponse({ accepted: false });
    return false;
  }
  managedFrame.executing = true;
  managedFrame.requestId = message.requestId;
  try {
    managedFrame.port.postMessage({
      type: "sunox-managed-frame-execute-v2",
      requestId: message.requestId,
      provider: message.provider
    });
    sendResponse({ accepted: true });
  } catch {
    rejectManagedFramePort(managedFrame.port, false);
    sendResponse({ accepted: false });
  }
  return false;
});

chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === POLL_ALARM) startBootstrap("poll alarm");
});
chrome.runtime.onInstalled.addListener(() => startBootstrap("install"));
chrome.runtime.onStartup.addListener(() => startBootstrap("browser startup"));
startBootstrap("service-worker load");
