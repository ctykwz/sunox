const POLL_ALARM = "sunox-bridge-poll";
const OFFSCREEN_PATH = "offscreen.html";
const SUNO_FRAME_RULE_ID = 29_764;
const SUNO_COOKIE_RULE_ID = 29_765;
const FRAME_RULE_IDS = [SUNO_FRAME_RULE_ID, SUNO_COOKIE_RULE_ID];
// This is only a redirect-following discovery entrypoint. The final clean
// same-origin URL returned by Suno is the route used for DNR and iframe
// navigation; the seed pathname is never an authorization boundary.
const MANAGED_PAGE_DISCOVERY_URL = "https://suno.com/";
const MANAGED_PAGE_ORIGIN = "https://suno.com";
const MANAGED_PAGE_QUERY_PARAMETER = "__sunox_bridge";
const MANAGED_PAGE_HASH_PREFIX = "#sunox-browser-bridge=";
const SUNO_FRAME_INITIATORS = [chrome.runtime.id];
const CHALLENGE_PROVIDERS = new Set(["hcaptcha", "turnstile"]);
const OFFSCREEN_LIFECYCLE_MESSAGE_TYPES = new Set([
  "sunox-frame-environment-prepare-v1",
  "sunox-frame-environment-release-v1",
  "sunox-frame-environment-retire-v1",
  "sunox-managed-frame-execute-v2"
]);
const MANAGED_NONCE_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const OFFSCREEN_CLIENT_ID_PATTERN =
  /^offscreen-[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const INITIAL_RULE_FETCH_TIMEOUT_MS = 8_000;
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
  turnstile_error_100:
    "silent_challenge_unavailable: Turnstile error callback (family 100)",
  turnstile_error_110:
    "silent_challenge_unavailable: Turnstile error callback (family 110)",
  turnstile_error_200:
    "silent_challenge_unavailable: Turnstile error callback (family 200)",
  turnstile_error_300:
    "silent_challenge_unavailable: Turnstile error callback (family 300)",
  turnstile_error_400:
    "silent_challenge_unavailable: Turnstile error callback (family 400)",
  turnstile_error_600:
    "silent_challenge_unavailable: Turnstile error callback (family 600)",
  turnstile_error_unknown:
    "silent_challenge_unavailable: Turnstile reported an unrecognized provider error",
  turnstile_interaction_timeout:
    "interactive_browser_required: the Turnstile interaction timed out",
  turnstile_no_callback:
    "silent_challenge_unavailable: Turnstile produced no callback before the silent deadline",
  unsupported_browser:
    "silent_challenge_unavailable: the challenge provider is unsupported in this browser"
});
let creatingOffscreenDocument;
let bootstrapPromise;
let frameEnvironmentPromise;
let frameRuleCleanupNonce = null;
let frameRuleCleanupOwnerDocumentId = null;
let frameRuleCleanupPromise = null;
let pendingEnvironmentNonce = null;
let pendingEnvironmentOwnerDocumentId = null;
let activeFrameEnvironment = null;
let offscreenOwnerBinding = null;
let offscreenOwnerBindingPromise = null;
let staleFrameRulesCleared = false;
let managedFrame;

function challengeDocumentCsp(provider) {
  if (!CHALLENGE_PROVIDERS.has(provider)) {
    throw new Error("A supported challenge provider is required");
  }
  const extensionSource = `chrome-extension://${chrome.runtime.id}`;
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

function exactManagedPageFilter(value, nonce) {
  if (!MANAGED_NONCE_PATTERN.test(nonce)) {
    throw new Error("A valid managed frame nonce is required");
  }
  const managedUrl = new URL(value);
  managedUrl.searchParams.set(MANAGED_PAGE_QUERY_PARAMETER, nonce);
  const escaped = managedUrl.href.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const escapedFragment = `${MANAGED_PAGE_HASH_PREFIX}${nonce}`
    .replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  // The query nonce is part of the network request/cache key. The matching
  // fragment is optional here because URL fragments are not sent over HTTP;
  // both content-script worlds require it before they can connect.
  return `^${escaped}(?:${escapedFragment})?$`;
}

function cleanManagedPageUrl(value) {
  if (typeof value !== "string" || value.length > 2_048) return null;
  try {
    const url = new URL(value);
    if (
      url.origin !== MANAGED_PAGE_ORIGIN
      || url.username
      || url.password
      || url.search
      || url.hash
    ) return null;
    return url.href;
  } catch {
    return null;
  }
}

function managedNetworkCandidateDetails(value) {
  if (typeof value !== "string" || value.length > 131_072) return null;
  try {
    const url = new URL(value);
    if (
      url.origin !== MANAGED_PAGE_ORIGIN
      || url.username
      || url.password
    ) return null;
    const keys = [...url.searchParams.keys()];
    const values = url.searchParams.getAll(MANAGED_PAGE_QUERY_PARAMETER);
    if (
      keys.length !== 1
      || keys[0] !== MANAGED_PAGE_QUERY_PARAMETER
      || values.length !== 1
      || !MANAGED_NONCE_PATTERN.test(values[0])
    ) return null;
    const nonce = values[0];
    if (
      url.hash
      && url.hash !== `${MANAGED_PAGE_HASH_PREFIX}${nonce}`
    ) return null;
    url.search = "";
    url.hash = "";
    return { nonce, pageUrl: url.href };
  } catch {
    return null;
  }
}

function managedPageCandidateDetails(value) {
  const details = managedNetworkCandidateDetails(value);
  if (!details) return null;
  try {
    return new URL(value).hash === `${MANAGED_PAGE_HASH_PREFIX}${details.nonce}`
      ? details
      : null;
  } catch {
    return null;
  }
}

function managedPageDetails(value) {
  const details = managedPageCandidateDetails(value);
  return details
    && details.pageUrl === activeFrameEnvironment?.pageUrl
    && details.nonce === activeFrameEnvironment?.nonce
    ? details
    : null;
}

async function currentEmbeddingEnvironment(signal) {
  const response = await fetch(MANAGED_PAGE_DISCOVERY_URL, {
    method: "GET",
    cache: "no-store",
    credentials: "omit",
    redirect: "follow",
    signal
  });
  try {
    if (!response.ok) {
      throw new Error(
        `Could not inspect the Suno embedding policy (HTTP ${response.status})`
      );
    }
    const pageUrl = cleanManagedPageUrl(response.url);
    if (!pageUrl) {
      throw new Error(
        "Suno redirected the managed challenge page away from a clean same-origin URL"
      );
    }
    const contentType = response.headers.get("content-type") || "";
    if (!/^text\/html(?:\s*;|$)/i.test(contentType)) {
      throw new Error(
        "Suno returned a non-HTML managed challenge route"
      );
    }
    return { pageUrl };
  } finally {
    await response.body?.cancel().catch(() => {});
  }
}

async function embeddingEnvironmentWithTimeout(timeoutMs) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), timeoutMs);
  try {
    return await currentEmbeddingEnvironment(controller.signal);
  } finally {
    clearTimeout(timeout);
  }
}

function frameRules(environment, nonce, provider) {
  const { pageUrl } = environment;
  const regexFilter = exactManagedPageFilter(pageUrl, nonce);
  const controlledCsp = challengeDocumentCsp(provider);
  if (typeof regexFilter !== "string" || regexFilter.length === 0) {
    throw new Error("An exact managed Suno page filter is required");
  }
  const sunoHeaders = [
    {
      header: "content-security-policy",
      operation: "set",
      value: controlledCsp
    },
    {
      header: "content-security-policy-report-only",
      operation: "remove"
    },
    { header: "x-frame-options", operation: "remove" },
    { header: "set-cookie", operation: "remove" },
    { header: "clear-site-data", operation: "remove" },
    { header: "cross-origin-embedder-policy", operation: "remove" },
    { header: "cross-origin-opener-policy", operation: "remove" },
    { header: "cross-origin-resource-policy", operation: "remove" },
    { header: "refresh", operation: "remove" },
    { header: "location", operation: "remove" },
    { header: "link", operation: "remove" },
    { header: "content-disposition", operation: "remove" },
    { header: "www-authenticate", operation: "remove" },
    { header: "proxy-authenticate", operation: "remove" },
    { header: "report-to", operation: "remove" },
    { header: "reporting-endpoints", operation: "remove" },
    { header: "nel", operation: "remove" },
    {
      header: "content-type",
      operation: "set",
      value: "text/html; charset=utf-8"
    },
    {
      header: "x-content-type-options",
      operation: "set",
      value: "nosniff"
    },
    {
      header: "referrer-policy",
      operation: "set",
      value: "strict-origin-when-cross-origin"
    },
    {
      header: "permissions-policy",
      operation: "set",
      value:
        "accelerometer=(), autoplay=(), camera=(), display-capture=(), "
        + "encrypted-media=(), fullscreen=(), geolocation=(), gyroscope=(), "
        + "magnetometer=(), microphone=(), midi=(), payment=(), "
        + "picture-in-picture=(), publickey-credentials-get=(), "
        + "screen-wake-lock=(), serial=(), usb=(), web-share=(), "
        + "xr-spatial-tracking=()"
    },
    {
      header: "cache-control",
      operation: "set",
      value: "no-store"
    },
    { header: "pragma", operation: "set", value: "no-cache" },
    { header: "expires", operation: "set", value: "0" }
  ];
  return [{
    id: SUNO_FRAME_RULE_ID,
    priority: 1_000,
    action: {
      type: "modifyHeaders",
      responseHeaders: sunoHeaders
    },
    condition: {
      regexFilter,
      isUrlFilterCaseSensitive: true,
      initiatorDomains: SUNO_FRAME_INITIATORS,
      requestMethods: ["get"],
      resourceTypes: ["sub_frame"],
      tabIds: [chrome.tabs.TAB_ID_NONE]
    }
  }, {
    id: SUNO_COOKIE_RULE_ID,
    priority: 1_000,
    action: {
      type: "modifyHeaders",
      requestHeaders: [
        { header: "cookie", operation: "remove" },
        { header: "authorization", operation: "remove" },
        { header: "if-modified-since", operation: "remove" },
        { header: "if-none-match", operation: "remove" },
        {
          header: "cache-control",
          operation: "set",
          value: "no-cache"
        },
        { header: "pragma", operation: "set", value: "no-cache" }
      ]
    },
    condition: {
      regexFilter,
      isUrlFilterCaseSensitive: true,
      initiatorDomains: SUNO_FRAME_INITIATORS,
      requestMethods: ["get"],
      resourceTypes: ["sub_frame"],
      tabIds: [chrome.tabs.TAB_ID_NONE]
    }
  }];
}

async function replaceSessionFrameRules(environment, nonce, provider) {
  await chrome.declarativeNetRequest.updateSessionRules({
    removeRuleIds: FRAME_RULE_IDS,
    addRules: frameRules(environment, nonce, provider)
  });
}

function managedNetworkObservation(details) {
  const candidate = managedNetworkCandidateDetails(details.url);
  if (
    !candidate
    || !activeFrameEnvironment
    || candidate.nonce !== activeFrameEnvironment.nonce
    || candidate.pageUrl !== activeFrameEnvironment.pageUrl
  ) return null;
  if (activeFrameEnvironment.network.retiring) return null;
  const extensionOrigin = `chrome-extension://${chrome.runtime.id}`;
  if (details?.tabId !== chrome.tabs.TAB_ID_NONE) {
    invalidateManagedNetwork("managed_tab_context_mismatch");
    return null;
  }
  if (details.type !== "sub_frame") {
    invalidateManagedNetwork("managed_resource_type_mismatch");
    return null;
  }
  if (details.parentFrameId !== 0) {
    invalidateManagedNetwork("managed_parent_frame_mismatch");
    return null;
  }
  if (details.initiator !== extensionOrigin) {
    invalidateManagedNetwork("managed_initiator_mismatch");
    return null;
  }
  return { candidate, network: activeFrameEnvironment.network };
}

function normalizedHeaders(headers) {
  if (!Array.isArray(headers)) return null;
  const result = new Map();
  for (const header of headers) {
    if (
      !header
      || typeof header.name !== "string"
      || typeof header.value !== "string"
      || Object.hasOwn(header, "binaryValue")
    ) return null;
    const name = header.name.toLowerCase();
    const values = result.get(name) || [];
    values.push(header.value);
    result.set(name, values);
  }
  return result;
}

function singletonHeader(headers, name) {
  const values = headers?.get(name);
  return values?.length === 1 ? values[0] : null;
}

function invalidateManagedNetwork(reason) {
  const environment = activeFrameEnvironment;
  if (
    !environment
    || environment.network.invalid
  ) return;
  environment.network.invalid = true;
  environment.network.invalidReason = reason;
  environment.network.requestHeadersVerified = false;
  environment.network.responseVerified = false;
  notifyOffscreen({
    type: "sunox-managed-frame-diagnostic-v1",
    nonce: environment.nonce,
    reason
  });
}

function reportManagedNetworkStage(stage) {
  const environment = activeFrameEnvironment;
  if (!environment || environment.network.invalid) return;
  environment.network.lastStage = stage;
  notifyOffscreen({
    type: "sunox-managed-frame-stage-v1",
    nonce: environment.nonce,
    stage
  });
}

function bindManagedNetworkRequest(details) {
  const observation = managedNetworkObservation(details);
  if (!observation) return;
  const { network } = observation;
  if (network.invalid) return;
  if (
    typeof activeFrameEnvironment.ownerDocumentId !== "string"
    || typeof details.parentDocumentId !== "string"
    || details.parentDocumentId !== activeFrameEnvironment.ownerDocumentId
  ) {
    invalidateManagedNetwork("managed_parent_document_mismatch");
    return;
  }
  if (network.requestId && network.requestId !== details.requestId) {
    invalidateManagedNetwork("multiple_managed_requests");
    return;
  }
  network.requestId = details.requestId;
  network.frameId = details.frameId;
  network.parentDocumentId = typeof details.parentDocumentId === "string"
    ? details.parentDocumentId
    : null;
  reportManagedNetworkStage("network_request_bound");
}

function verifyManagedRequestHeaders(details) {
  const observation = managedNetworkObservation(details);
  if (!observation) return;
  const { network } = observation;
  if (
    network.invalid
    || !network.requestId
    || network.requestId !== details.requestId
    || network.frameId !== details.frameId
  ) {
    invalidateManagedNetwork("managed_request_identity_mismatch");
    return;
  }
  const headers = normalizedHeaders(details.requestHeaders);
  const cacheControl = singletonHeader(headers, "cache-control");
  const pragma = singletonHeader(headers, "pragma");
  let unsafeReason = null;
  if (!headers) {
    unsafeReason = "managed_request_headers_invalid";
  } else if (
    headers.has("cookie")
    || headers.has("authorization")
  ) {
    unsafeReason = "managed_request_credentials_present";
  } else if (
    headers.has("if-none-match")
    || headers.has("if-modified-since")
    || cacheControl?.toLowerCase() !== "no-cache"
    || pragma?.toLowerCase() !== "no-cache"
  ) {
    unsafeReason = "managed_request_cache_unsafe";
  }
  if (unsafeReason) {
    invalidateManagedNetwork(unsafeReason);
    return;
  }
  network.requestHeadersVerified = true;
  reportManagedNetworkStage("network_request_headers_verified");
}

function rejectManagedRedirect(details) {
  if (managedNetworkObservation(details)) {
    invalidateManagedNetwork("managed_redirect_rejected");
  }
}

function verifyManagedResponse(details) {
  const observation = managedNetworkObservation(details);
  if (!observation) return;
  const { network } = observation;
  if (
    network.invalid
    || !network.requestHeadersVerified
    || network.requestId !== details.requestId
    || network.frameId !== details.frameId
  ) {
    invalidateManagedNetwork("managed_response_identity_mismatch");
    return;
  }
  const headers = normalizedHeaders(details.responseHeaders);
  const forbidden = [
    "clear-site-data",
    "content-disposition",
    "content-security-policy-report-only",
    "cross-origin-embedder-policy",
    "cross-origin-opener-policy",
    "cross-origin-resource-policy",
    "link",
    "location",
    "nel",
    "proxy-authenticate",
    "refresh",
    "report-to",
    "reporting-endpoints",
    "set-cookie",
    "www-authenticate",
    "x-frame-options"
  ];
  let unsafeReason = null;
  if (!headers) {
    unsafeReason = "managed_response_headers_invalid";
  } else if (
    !Number.isInteger(details.statusCode)
    || details.statusCode < 200
    || details.statusCode >= 300
  ) {
    unsafeReason = "managed_response_status_invalid";
  } else if (details.fromCache !== false) {
    unsafeReason = "managed_response_cache_unsafe";
  } else if (forbidden.some((name) => headers.has(name))) {
    unsafeReason = "managed_response_side_effect_header";
  } else if (
    singletonHeader(headers, "content-security-policy")
    !== challengeDocumentCsp(activeFrameEnvironment.provider)
  ) {
    unsafeReason = "managed_response_csp_invalid";
  } else if (
    singletonHeader(headers, "content-type")
      ?.toLowerCase() !== "text/html; charset=utf-8"
    || singletonHeader(headers, "x-content-type-options")
      ?.toLowerCase() !== "nosniff"
  ) {
    unsafeReason = "managed_response_type_invalid";
  } else if (
    singletonHeader(headers, "referrer-policy")
      ?.toLowerCase() !== "strict-origin-when-cross-origin"
    || singletonHeader(headers, "permissions-policy") === null
  ) {
    unsafeReason = "managed_response_policy_invalid";
  } else if (
    singletonHeader(headers, "cache-control")
      ?.toLowerCase() !== "no-store"
    || singletonHeader(headers, "pragma")?.toLowerCase() !== "no-cache"
    || singletonHeader(headers, "expires") !== "0"
  ) {
    unsafeReason = "managed_response_cache_control_invalid";
  }
  if (unsafeReason) {
    invalidateManagedNetwork(unsafeReason);
    return;
  }
  network.responseVerified = true;
  reportManagedNetworkStage("network_response_verified");
}

function captureManagedNetworkError(details) {
  const observation = managedNetworkObservation(details);
  if (!observation || observation.network.responseVerified) return;
  invalidateManagedNetwork("managed_network_error");
}

async function installFreshFrameRules(environment, nonce, provider) {
  await chrome.declarativeNetRequest.updateDynamicRules({
    removeRuleIds: FRAME_RULE_IDS
  });
  await replaceSessionFrameRules(environment, nonce, provider);
}

async function removeFrameRules() {
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
  staleFrameRulesCleared = true;
}

function beginFrameRuleCleanup(nonce = null) {
  if (frameRuleCleanupPromise) return frameRuleCleanupPromise;
  // A failed remove must remain observable as dirty state so the next
  // bootstrap/alarm retries it instead of assuming the transient rules are
  // gone.
  staleFrameRulesCleared = false;
  frameRuleCleanupOwnerDocumentId =
    activeFrameEnvironment?.ownerDocumentId
    ?? pendingEnvironmentOwnerDocumentId;
  activeFrameEnvironment = null;
  frameRuleCleanupNonce = nonce;
  frameRuleCleanupPromise = removeFrameRules().finally(() => {
    frameRuleCleanupNonce = null;
    frameRuleCleanupOwnerDocumentId = null;
    frameRuleCleanupPromise = null;
  });
  return frameRuleCleanupPromise;
}

function ensureFrameEnvironment(
  nonce,
  previousNonce,
  provider,
  ownerDocumentId
) {
  if (
    !MANAGED_NONCE_PATTERN.test(nonce)
    || !CHALLENGE_PROVIDERS.has(provider)
    || typeof ownerDocumentId !== "string"
    || ownerDocumentId.length === 0
    || ownerDocumentId.length > 128
  ) {
    return Promise.reject(new Error("A valid managed frame nonce is required"));
  }
  if (frameEnvironmentPromise) {
    return pendingEnvironmentNonce === nonce
      && pendingEnvironmentOwnerDocumentId === ownerDocumentId
      ? frameEnvironmentPromise
      : Promise.reject(new Error("A managed frame environment is being prepared"));
  }
  pendingEnvironmentNonce = nonce;
  pendingEnvironmentOwnerDocumentId = ownerDocumentId;
  // Register the complete owner lookup and DNR transaction before the first
  // await. If Chrome replaces the offscreen document during that window,
  // orphan recovery can now see, await, and clean this operation.
  frameEnvironmentPromise = prepareFrameEnvironmentForOwner(
    nonce,
    previousNonce,
    provider,
    ownerDocumentId
  ).finally(() => {
    frameEnvironmentPromise = null;
    pendingEnvironmentNonce = null;
    pendingEnvironmentOwnerDocumentId = null;
  });
  return frameEnvironmentPromise;
}

async function prepareFrameEnvironmentForOwner(
  nonce,
  previousNonce,
  provider,
  ownerDocumentId
) {
  await requireCurrentOffscreenOwner(ownerDocumentId);
  if (frameRuleCleanupPromise) await frameRuleCleanupPromise;
  await requireCurrentOffscreenOwner(ownerDocumentId);
  if (activeFrameEnvironment) {
    if (activeFrameEnvironment.ownerDocumentId !== ownerDocumentId) {
      const staleNonce = activeFrameEnvironment.nonce;
      retireManagedFrame(staleNonce);
      await beginFrameRuleCleanup(staleNonce);
      await requireCurrentOffscreenOwner(ownerDocumentId);
      return await installFreshFrameEnvironment(
        nonce,
        provider,
        ownerDocumentId
      );
    }
    if (activeFrameEnvironment.nonce === nonce) {
      return activeFrameEnvironment.provider === provider
        && activeFrameEnvironment.ownerDocumentId === ownerDocumentId
        ? Promise.resolve(activeFrameEnvironment.pageUrl)
        : Promise.reject(new Error("The managed frame provider changed"));
    }
    if (
      MANAGED_NONCE_PATTERN.test(previousNonce)
      && activeFrameEnvironment.nonce === previousNonce
    ) {
      if (managedFrame) {
        if (
          managedFrame.nonce !== previousNonce
          || managedFrame.executing
        ) {
          return Promise.reject(
            new Error("A managed frame environment is already active")
          );
        }
        // Retire the old idle port before disconnecting it. Chrome may
        // deliver onDisconnect asynchronously; its identity guard then keeps
        // that late callback from releasing the newly rotated reservation.
        const retiredFrame = managedFrame;
        managedFrame = null;
        try {
          retiredFrame.port.disconnect();
        } catch {}
      }
      const previousEnvironment = activeFrameEnvironment;
      return await rotateFrameEnvironment(
        previousEnvironment,
        nonce,
        provider,
        ownerDocumentId
      );
    }
    return Promise.reject(
      new Error("A managed frame environment is already active")
    );
  }
  return await installFreshFrameEnvironment(
    nonce,
    provider,
    ownerDocumentId
  );
}

async function currentOffscreenOwnerDocumentId() {
  const documentUrl = chrome.runtime.getURL(OFFSCREEN_PATH);
  const contexts = await chrome.runtime.getContexts({
    contextTypes: ["OFFSCREEN_DOCUMENT"],
    documentUrls: [documentUrl]
  });
  if (
    contexts.length !== 1
    || contexts[0].contextType !== "OFFSCREEN_DOCUMENT"
    || contexts[0].documentUrl !== documentUrl
    || contexts[0].tabId !== chrome.tabs.TAB_ID_NONE
    || typeof contexts[0].documentId !== "string"
    || contexts[0].documentId.length === 0
    || contexts[0].documentId.length > 128
  ) {
    throw new Error("The offscreen owner document identity is unavailable");
  }
  return contexts[0].documentId;
}

async function requireCurrentOffscreenOwner(ownerDocumentId) {
  if (await currentOffscreenOwnerDocumentId() !== ownerDocumentId) {
    throw new Error("The Browser Bridge offscreen owner changed");
  }
}

async function rotateFrameEnvironment(
  environment,
  nonce,
  provider,
  ownerDocumentId
) {
  if (environment.network.invalid || !environment.network.retiring) {
    throw new Error("The prior managed frame environment cannot be reused");
  }
  let rulesReplaced = false;
  try {
    await replaceSessionFrameRules(environment, nonce, provider);
    rulesReplaced = true;
    await requireCurrentOffscreenOwner(ownerDocumentId);
    activeFrameEnvironment = {
      nonce,
      network: createManagedNetworkState(),
      ownerDocumentId,
      pageUrl: environment.pageUrl,
      provider
    };
    return environment.pageUrl;
  } catch (error) {
    if (rulesReplaced) await beginFrameRuleCleanup(environment.nonce);
    throw error;
  }
}

function createManagedNetworkState() {
  return {
    contentDocumentId: null,
    frameId: null,
    invalid: false,
    invalidReason: null,
    lastStage: "environment_prepared",
    parentDocumentId: null,
    requestHeadersVerified: false,
    requestId: null,
    responseVerified: false,
    retiring: false
  };
}

async function installFreshFrameEnvironment(nonce, provider, ownerDocumentId) {
  try {
    const environment = await embeddingEnvironmentWithTimeout(
      INITIAL_RULE_FETCH_TIMEOUT_MS
    );
    await installFreshFrameRules(environment, nonce, provider);
    await requireCurrentOffscreenOwner(ownerDocumentId);
    activeFrameEnvironment = {
      nonce,
      network: createManagedNetworkState(),
      ownerDocumentId,
      pageUrl: environment.pageUrl,
      provider
    };
    staleFrameRulesCleared = true;
    return environment.pageUrl;
  } catch (error) {
    try {
      await beginFrameRuleCleanup();
    } catch (cleanupError) {
      throw new AggregateError(
        [error, cleanupError],
        "Suno frame policy refresh and fail-closed cleanup both failed"
      );
    }
    throw error;
  }
}

async function releaseFrameEnvironment(nonce) {
  if (!MANAGED_NONCE_PATTERN.test(nonce)) return false;
  if (frameEnvironmentPromise && pendingEnvironmentNonce !== nonce) {
    return false;
  }
  if (frameEnvironmentPromise) {
    await frameEnvironmentPromise.catch(() => {});
  }
  if (frameRuleCleanupPromise) {
    if (frameRuleCleanupNonce !== nonce) return false;
    retireManagedFrame(nonce);
    await frameRuleCleanupPromise;
    return true;
  }
  if (!activeFrameEnvironment) {
    retireManagedFrame(nonce);
    return true;
  }
  if (activeFrameEnvironment && activeFrameEnvironment.nonce !== nonce) {
    if (activeFrameEnvironment.network.retiring) {
      const retiredNonce = activeFrameEnvironment.nonce;
      retireManagedFrame(retiredNonce);
      await beginFrameRuleCleanup(retiredNonce);
      return true;
    }
    return false;
  }
  retireManagedFrame(nonce);
  await beginFrameRuleCleanup(nonce);
  return true;
}

function retireManagedFrame(nonce) {
  if (!managedFrame || managedFrame.nonce !== nonce) return;
  const retiredFrame = managedFrame;
  managedFrame = null;
  try {
    retiredFrame.port.disconnect();
  } catch {}
}

function retireFrameEnvironmentForRetry(nonce) {
  if (
    !MANAGED_NONCE_PATTERN.test(nonce)
    || !activeFrameEnvironment
    || activeFrameEnvironment.nonce !== nonce
    || activeFrameEnvironment.network.invalid
  ) return false;
  if (
    managedFrame
    && (
      managedFrame.nonce !== nonce
      || managedFrame.executing
    )
  ) return false;
  activeFrameEnvironment.network.retiring = true;
  retireManagedFrame(nonce);
  return true;
}

function isOffscreenSender(sender) {
  return sender?.id === chrome.runtime.id
    && !sender.tab
    && sender.origin === `chrome-extension://${chrome.runtime.id}`
    && sender.url === chrome.runtime.getURL(OFFSCREEN_PATH);
}

function validOwnerDocumentId(value) {
  return typeof value === "string"
    && value.length > 0
    && value.length <= 128;
}

function offscreenSenderOwnerDocumentId(message, sender) {
  return isOffscreenSender(sender)
    && OFFSCREEN_CLIENT_ID_PATTERN.test(message?.clientId)
    && message.clientId === offscreenOwnerBinding?.clientId
    && validOwnerDocumentId(offscreenOwnerBinding.documentId)
    ? offscreenOwnerBinding.documentId
    : null;
}

function ownerCanReleaseEnvironment(ownerDocumentId) {
  if (!validOwnerDocumentId(ownerDocumentId)) return false;
  if (frameEnvironmentPromise) {
    return pendingEnvironmentOwnerDocumentId === ownerDocumentId;
  }
  if (activeFrameEnvironment) {
    return activeFrameEnvironment.ownerDocumentId === ownerDocumentId;
  }
  if (frameRuleCleanupPromise) {
    return frameRuleCleanupOwnerDocumentId === ownerDocumentId;
  }
  return true;
}

async function executeManagedFrameRequest(message, ownerDocumentId) {
  await requireCurrentOffscreenOwner(ownerDocumentId);
  if (
    activeFrameEnvironment?.ownerDocumentId !== ownerDocumentId
    || !managedFrame
    || managedFrame.executing
    || message.nonce !== managedFrame.nonce
    || typeof message.requestId !== "string"
    || message.requestId.length === 0
    || message.requestId.length > 128
    || !["hcaptcha", "turnstile"].includes(message.provider)
    || message.provider !== managedFrame.provider
  ) return false;
  managedFrame.executing = true;
  managedFrame.requestId = message.requestId;
  try {
    managedFrame.port.postMessage({
      type: "sunox-managed-frame-execute-v2",
      requestId: message.requestId,
      provider: message.provider
    });
    return true;
  } catch {
    rejectManagedFramePort(managedFrame.port, false);
    return false;
  }
}

async function lifecycleOwnerDocumentId(message, sender) {
  if (
    !isOffscreenSender(sender)
    || !OFFSCREEN_CLIENT_ID_PATTERN.test(message?.clientId)
  ) return null;
  const boundOwnerDocumentId = offscreenSenderOwnerDocumentId(message, sender);
  if (boundOwnerDocumentId) return boundOwnerDocumentId;
  const binding = await ensureOffscreenOwnerBinding(message.clientId);
  return binding?.clientId === message.clientId
    && validOwnerDocumentId(binding.documentId)
    ? binding.documentId
    : null;
}

async function handleOffscreenLifecycleMessage(message, sender) {
  const ownerDocumentId = await lifecycleOwnerDocumentId(message, sender);
  if (!ownerDocumentId) {
    return {
      accepted: false,
      ...(message.type === "sunox-frame-environment-prepare-v1"
        ? { error: "challenge_environment_unavailable" }
        : {})
    };
  }
  if (message.type === "sunox-frame-environment-prepare-v1") {
    const pageUrl = await ensureFrameEnvironment(
      message.nonce,
      message.previousNonce,
      message.provider,
      ownerDocumentId
    );
    return { accepted: true, pageUrl };
  }
  if (message.type === "sunox-frame-environment-retire-v1") {
    return {
      accepted: activeFrameEnvironment?.ownerDocumentId === ownerDocumentId
        && retireFrameEnvironmentForRetry(message.nonce)
    };
  }
  if (message.type === "sunox-frame-environment-release-v1") {
    return {
      accepted: ownerCanReleaseEnvironment(ownerDocumentId)
        && await releaseFrameEnvironment(message.nonce)
    };
  }
  return {
    accepted: await executeManagedFrameRequest(message, ownerDocumentId)
  };
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
  const details = managedPageDetails(sender.url);
  const network = activeFrameEnvironment?.network;
  return details
    && network
    && !network.invalid
    && !network.retiring
    && network.requestHeadersVerified
    && network.responseVerified
    && bindManagedSenderIdentity(sender, network)
    ? details
    : null;
}

function bindManagedSenderIdentity(sender, network) {
  if (!Number.isInteger(network?.frameId) || network.frameId <= 0) return false;
  if (
    Number.isInteger(sender?.frameId)
    && sender.frameId !== network.frameId
  ) return false;
  // Chrome currently omits both frameId and documentId for messages from a
  // no-tab content script. The verified offscreen parent, exact one-time URL,
  // and request/response lifecycle are therefore the primary identity. If a
  // browser version supplies an optional content document UUID, bind it and
  // reject any later mismatch or downgrade.
  if (
    network.contentDocumentId === null
    && typeof sender?.documentId === "string"
  ) {
    network.contentDocumentId = sender.documentId;
    return true;
  }
  return network.contentDocumentId === null
    || sender?.documentId === network.contentDocumentId;
}

function managedFrameSenderRejectionReason(sender) {
  if (sender?.id !== chrome.runtime.id) return "extension_id_mismatch";
  if (sender.tab) return "sender_has_tab";
  if (sender.origin !== MANAGED_PAGE_ORIGIN) return "origin_mismatch";
  if (sender.frameId === 0) return "top_level_frame";
  const candidate = managedPageCandidateDetails(sender.url);
  if (!candidate) return "managed_url_invalid";
  if (!activeFrameEnvironment) return "managed_environment_missing";
  if (candidate.nonce !== activeFrameEnvironment.nonce) {
    return "managed_nonce_mismatch";
  }
  if (candidate.pageUrl !== activeFrameEnvironment.pageUrl) {
    return "managed_route_mismatch";
  }
  const network = activeFrameEnvironment.network;
  if (network.invalid) return network.invalidReason || "managed_network_invalid";
  if (network.retiring) return "managed_environment_retired";
  if (!network.requestHeadersVerified) return "managed_request_unverified";
  if (!network.responseVerified) return "managed_response_unverified";
  if (
    typeof network.contentDocumentId === "string"
    && sender.documentId !== network.contentDocumentId
  ) {
    return typeof sender.documentId === "string"
      ? "managed_document_mismatch"
      : "managed_sender_document_missing";
  }
  if (
    Number.isInteger(sender.frameId)
    && sender.frameId !== network.frameId
  ) return "managed_frame_mismatch";
  if (!Number.isInteger(network.frameId) || network.frameId <= 0) {
    return "managed_network_frame_missing";
  }
  return "unknown_sender_mismatch";
}

function notifyOffscreen(message) {
  chrome.runtime.sendMessage(message).catch(() => {});
}

function rejectManagedFramePort(
  port,
  permanent = true,
  reason = "managed_frame_rejected"
) {
  const sender = port.sender;
  const candidate = managedPageCandidateDetails(sender?.url);
  const nonce = candidate?.nonce || (
    sender?.id === chrome.runtime.id
    && !sender.tab
    && sender.origin === MANAGED_PAGE_ORIGIN
      ? activeFrameEnvironment?.nonce
      : null
  );
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
  if (managedFrame) {
    rejectManagedFramePort(port, true, "managed_frame_busy");
    return;
  }

  const state = {
    completed: false,
    executing: false,
    nonce: details.nonce,
    port,
    provider: activeFrameEnvironment.provider,
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
    releaseFrameEnvironment(state.nonce).catch(() => {});
  });
  notifyOffscreen({
    type: "sunox-managed-frame-ready-v2",
    nonce: state.nonce
  });
}

async function offscreenStatus() {
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
    return response?.type === "sunox-offscreen-pong-v1"
      && OFFSCREEN_CLIENT_ID_PATTERN.test(response.clientId)
      ? response
      : null;
  } catch {
    return null;
  } finally {
    clearTimeout(timeout);
  }
}

function offscreenStatusIsHealthy(response) {
  if (!response) return false;
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
}

async function currentOffscreenOwnerBinding(response) {
  if (!response || !OFFSCREEN_CLIENT_ID_PATTERN.test(response.clientId)) {
    throw new Error("The Browser Bridge offscreen client identity is unavailable");
  }
  const ownerDocumentId = await currentOffscreenOwnerDocumentId();
  const confirmation = await offscreenStatus();
  if (confirmation?.clientId !== response.clientId) {
    throw new Error("The Browser Bridge offscreen client changed");
  }
  await requireCurrentOffscreenOwner(ownerDocumentId);
  return {
    clientId: response.clientId,
    documentId: ownerDocumentId
  };
}

function environmentOwnerDocumentId() {
  return pendingEnvironmentOwnerDocumentId
    ?? activeFrameEnvironment?.ownerDocumentId
    ?? frameRuleCleanupOwnerDocumentId;
}

async function adoptOffscreenOwnerBinding(candidateBinding) {
  const environmentOwner = environmentOwnerDocumentId();
  const ownerChanged = (
    offscreenOwnerBinding !== null
    && (
      offscreenOwnerBinding.clientId !== candidateBinding.clientId
      || offscreenOwnerBinding.documentId !== candidateBinding.documentId
    )
  ) || (
    validOwnerDocumentId(environmentOwner)
    && environmentOwner !== candidateBinding.documentId
  );
  if (ownerChanged) {
    offscreenOwnerBinding = null;
    await revokeOrphanedFrameEnvironment();
  }
  offscreenOwnerBinding = candidateBinding;
  return candidateBinding;
}

async function establishOffscreenOwnerBinding(clientId, status = null) {
  const response = status ?? await offscreenStatus();
  if (
    !response
    || response.clientId !== clientId
    || !OFFSCREEN_CLIENT_ID_PATTERN.test(clientId)
  ) return null;
  return await adoptOffscreenOwnerBinding(
    await currentOffscreenOwnerBinding(response)
  );
}

async function ensureOffscreenOwnerBinding(clientId, status = null) {
  if (
    OFFSCREEN_CLIENT_ID_PATTERN.test(clientId)
    && offscreenOwnerBinding?.clientId === clientId
  ) return offscreenOwnerBinding;
  if (offscreenOwnerBindingPromise) {
    await offscreenOwnerBindingPromise.catch(() => {});
    return offscreenOwnerBinding?.clientId === clientId
      ? offscreenOwnerBinding
      : null;
  }
  offscreenOwnerBindingPromise = establishOffscreenOwnerBinding(
    clientId,
    status
  ).finally(() => {
    offscreenOwnerBindingPromise = null;
  });
  return await offscreenOwnerBindingPromise;
}

async function revokeOrphanedFrameEnvironment() {
  if (managedFrame) {
    const retiredFrame = managedFrame;
    managedFrame = null;
    try {
      retiredFrame.port.disconnect();
    } catch {}
  }

  // A dead offscreen owner may have disappeared while its asynchronous policy
  // discovery was still installing rules. Let that operation reach a terminal
  // state, then remove anything it installed before a replacement owner starts.
  if (frameEnvironmentPromise) {
    await frameEnvironmentPromise.catch(() => {});
  }
  if (frameRuleCleanupPromise) await frameRuleCleanupPromise;
  if (activeFrameEnvironment || !staleFrameRulesCleared) {
    await beginFrameRuleCleanup();
  }
}

async function ensureOffscreenDocument() {
  const documentUrl = chrome.runtime.getURL(OFFSCREEN_PATH);
  const contexts = await chrome.runtime.getContexts({
    contextTypes: ["OFFSCREEN_DOCUMENT"],
    documentUrls: [documentUrl]
  });
  let ownerWasLost = contexts.length === 0
    && Boolean(
      managedFrame
      || activeFrameEnvironment
      || frameEnvironmentPromise
    );
  if (contexts.length > 0) {
    const status = await offscreenStatus();
    if (offscreenStatusIsHealthy(status)) {
      if (!await ensureOffscreenOwnerBinding(status.clientId, status)) {
        throw new Error("The Browser Bridge offscreen owner could not be bound");
      }
      return;
    }
    offscreenOwnerBinding = null;
    await chrome.offscreen.closeDocument().catch(() => {});
    ownerWasLost = true;
  } else {
    offscreenOwnerBinding = null;
  }
  if (ownerWasLost) await revokeOrphanedFrameEnvironment();
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
  const status = await offscreenStatus();
  if (!status) {
    throw new Error("The Browser Bridge offscreen document did not identify itself");
  }
  if (!await ensureOffscreenOwnerBinding(status.clientId, status)) {
    throw new Error("The Browser Bridge offscreen owner could not be bound");
  }
}

async function ensurePollAlarm() {
  if (await chrome.alarms.get(POLL_ALARM)) return;
  await chrome.alarms.create(POLL_ALARM, {
    delayInMinutes: 0.5,
    periodInMinutes: 0.5
  });
}

async function bootstrap() {
  if (frameRuleCleanupPromise) await frameRuleCleanupPromise;
  if (
    !staleFrameRulesCleared
    && !activeFrameEnvironment
    && !frameEnvironmentPromise
  ) {
    await beginFrameRuleCleanup();
  }
  await ensurePollAlarm();
  await ensureOffscreenDocument();
  const response = await chrome.runtime.sendMessage({
    type: "sunox-offscreen-start-v1"
  });
  if (
    response?.accepted !== true
    || response.clientId !== offscreenOwnerBinding?.clientId
  ) {
    throw new Error("Offscreen Browser Bridge did not acknowledge polling startup");
  }
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
    // All bootstrap triggers are background recovery paths. The next alarm or
    // lifecycle wake retries them, while real challenge failures are returned
    // to the CLI. Avoid creating a permanent red Chrome error badge for a
    // transient redirect or network failure that already self-recovers.
    console.warn(
      `[Sunox Browser Bridge] Bootstrap failed (${trigger}).`,
      error
    );
  });
}

chrome.runtime.onConnect.addListener((port) => {
  if (port.name === "sunox-managed-frame-v2") bindManagedFramePort(port);
});

const managedWebRequestFilter = {
  types: ["sub_frame"],
  urls: ["https://suno.com/*"]
};
chrome.webRequest.onBeforeRequest.addListener(
  bindManagedNetworkRequest,
  managedWebRequestFilter
);
chrome.webRequest.onSendHeaders.addListener(
  verifyManagedRequestHeaders,
  managedWebRequestFilter,
  ["requestHeaders", "extraHeaders"]
);
chrome.webRequest.onBeforeRedirect.addListener(
  rejectManagedRedirect,
  managedWebRequestFilter
);
chrome.webRequest.onResponseStarted.addListener(
  verifyManagedResponse,
  managedWebRequestFilter,
  ["responseHeaders", "extraHeaders"]
);
chrome.webRequest.onErrorOccurred.addListener(
  captureManagedNetworkError,
  managedWebRequestFilter
);

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message?.type === "sunox-managed-frame-stage-report-v1") {
    const details = managedFrameSenderDetails(sender);
    const allowedStage = [
      "controlled_document",
      "controlled_document_install_failed",
      "page_ready",
      "runner_error",
      "runner_injected",
      "runner_loaded"
    ].includes(message.stage);
    if (
      details
      && message.nonce === details.nonce
      && allowedStage
    ) {
      activeFrameEnvironment.network.lastStage = message.stage;
      notifyOffscreen({
        type: "sunox-managed-frame-stage-v1",
        nonce: details.nonce,
        stage: message.stage
      });
    } else if (
      allowedStage
      && MANAGED_NONCE_PATTERN.test(message.nonce)
      && message.nonce === activeFrameEnvironment?.nonce
      && sender?.id === chrome.runtime.id
      && !sender.tab
      && managedPageCandidateDetails(sender.url)?.nonce === message.nonce
    ) {
      const reason = managedFrameSenderRejectionReason(sender);
      if (
        reason === "managed_request_unverified"
        || reason === "managed_response_unverified"
      ) {
        reportManagedNetworkStage("content_report_pending_network");
      } else {
        notifyOffscreen({
          type: "sunox-managed-frame-diagnostic-v1",
          nonce: message.nonce,
          reason: `stage_report_rejected_${reason}`
        });
      }
    }
    return false;
  }
  if (
    OFFSCREEN_LIFECYCLE_MESSAGE_TYPES.has(message?.type)
    && isOffscreenSender(sender)
  ) {
    handleOffscreenLifecycleMessage(message, sender)
      .then(sendResponse)
      .catch(() => sendResponse({
        accepted: false,
        ...(message.type === "sunox-frame-environment-prepare-v1"
          ? { error: "challenge_environment_unavailable" }
          : {})
      }));
    return true;
  }
  return false;
});

chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === POLL_ALARM) startBootstrap("poll alarm");
});
chrome.runtime.onInstalled.addListener(() => startBootstrap("install"));
chrome.runtime.onStartup.addListener(() => startBootstrap("browser startup"));
startBootstrap("service-worker load");
