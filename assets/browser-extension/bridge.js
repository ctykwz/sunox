(() => {
  const MANAGED_PAGE_HASH_PREFIX = "#sunox-browser-bridge=";
  const CLERK_RETURN_PARAMETER = "__clerk_handshake";
  const MANAGED_NONCE_PATTERN =
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

  const managedPageDetails = () => {
    if (
      window === window.top
      || window.parent !== window.top
      || location.href.length > 131_072
    ) return null;
    try {
      const url = new URL(location.href);
      if (
        url.origin !== "https://suno.com"
        || !["/create", "/create/"].includes(url.pathname)
        || url.username
        || url.password
        || !url.hash.startsWith(MANAGED_PAGE_HASH_PREFIX)
      ) return null;
      const nonce = url.hash.slice(MANAGED_PAGE_HASH_PREFIX.length);
      if (!MANAGED_NONCE_PATTERN.test(nonce)) return null;
      if (!url.search) return { clerkReturn: false, nonce };
      const keys = [...url.searchParams.keys()];
      const values = url.searchParams.getAll(CLERK_RETURN_PARAMETER);
      if (
        keys.length !== 1
        || keys[0] !== CLERK_RETURN_PARAMETER
        || values.length !== 1
        || values[0].length === 0
        || values[0].length > 65_536
      ) return null;
      return { clerkReturn: true, nonce };
    } catch {
      return null;
    }
  };

  const initialPage = managedPageDetails();
  if (!initialPage || globalThis.__sunoxBridgeContentLoaded) return;
  globalThis.__sunoxBridgeContentLoaded = true;
  const managedNonce = initialPage.nonce;
  const maxTokenLength = 16_384;
  const readinessPollMs = 100;
  const readinessStableMs = 500;
  const challengePageTimeoutMs = 50_000;
  const serviceWorkerKeepAliveMs = 20_000;
  const allowedErrorCodes = new Set([
    "challenge_expired",
    "challenge_failed",
    "challenge_sdk_unavailable",
    "challenge_timeout",
    "interactive_browser_required",
    "invalid_challenge_token",
    "page_not_ready",
    "page_unavailable",
    "silent_challenge_unavailable",
    "unsupported_browser"
  ]);
  let busy = false;
  let executionReceived = false;
  let port;
  let readinessHref = null;
  let readinessSince = 0;
  let readinessTimer;

  const currentManagedPage = () => {
    const details = managedPageDetails();
    return details?.nonce === managedNonce ? details : null;
  };

  const isExecutionReadyPage = () => {
    const details = currentManagedPage();
    return details?.clerkReturn === false
      && globalThis.document?.readyState === "complete";
  };

  function executeInPage(challenge) {
    return new Promise((resolve) => {
      const timeout = setTimeout(() => {
        window.removeEventListener("message", onResult);
        resolve({ errorCode: "challenge_timeout" });
      }, challengePageTimeoutMs);

      function onResult(event) {
        if (event.source !== window || event.origin !== location.origin) return;
        const result = event.data;
        if (
          result?.source !== "sunox-page-v1"
          || result.requestId !== challenge.requestId
        ) return;
        clearTimeout(timeout);
        window.removeEventListener("message", onResult);
        const token = typeof result.token === "string"
          && result.token.length > 0
          && result.token.length <= maxTokenLength
          ? result.token
          : null;
        resolve({
          token,
          errorCode: token
            ? null
            : allowedErrorCodes.has(result.errorCode)
              ? result.errorCode
              : "challenge_failed"
        });
      }

      window.addEventListener("message", onResult);
      window.postMessage({
        source: "sunox-extension-v1",
        requestId: challenge.requestId,
        provider: challenge.provider
      }, location.origin);
    });
  }

  async function execute(challenge) {
    if (busy || !isExecutionReadyPage()) {
      return { token: null, errorCode: "page_unavailable" };
    }
    busy = true;
    try {
      return await executeInPage(challenge);
    } catch {
      return { token: null, errorCode: "challenge_failed" };
    } finally {
      busy = false;
    }
  }

  function scheduleReadinessPolling(delay = readinessPollMs) {
    if (readinessTimer || port || executionReceived) return;
    readinessTimer = setTimeout(() => {
      readinessTimer = null;
      if (!currentManagedPage()) return;
      if (!isExecutionReadyPage()) {
        readinessHref = null;
        readinessSince = 0;
        scheduleReadinessPolling();
        return;
      }
      if (readinessHref !== location.href) {
        readinessHref = location.href;
        readinessSince = Date.now();
      }
      if (Date.now() - readinessSince >= readinessStableMs) {
        connect();
        return;
      }
      scheduleReadinessPolling();
    }, delay);
  }

  function connect() {
    if (port || executionReceived || !isExecutionReadyPage()) return;
    const connected = chrome.runtime.connect({
      name: "sunox-managed-frame-v2"
    });
    port = connected;
    connected.onMessage.addListener(async (message) => {
      if (message?.type === "sunox-managed-frame-rejected-v2") {
        executionReceived = true;
        connected.disconnect();
        return;
      }
      if (
        message?.type !== "sunox-managed-frame-execute-v2"
        || typeof message.requestId !== "string"
        || message.requestId.length === 0
        || message.requestId.length > 128
        || !["hcaptcha", "turnstile"].includes(message.provider)
      ) return;
      executionReceived = true;
      const keepAlive = setInterval(() => {
        if (port !== connected) return;
        try {
          connected.postMessage({
            type: "sunox-managed-frame-keepalive-v2",
            requestId: message.requestId
          });
        } catch {}
      }, serviceWorkerKeepAliveMs);
      let result;
      try {
        result = await execute({
          requestId: message.requestId,
          provider: message.provider
        });
      } finally {
        clearInterval(keepAlive);
      }
      if (port !== connected) return;
      connected.postMessage({
        type: "sunox-managed-frame-result-v2",
        requestId: message.requestId,
        token: result.token || null,
        errorCode: result.errorCode || null
      });
    });
    connected.onDisconnect.addListener(() => {
      if (port !== connected) return;
      port = null;
      if (!executionReceived && currentManagedPage()) {
        readinessHref = null;
        readinessSince = 0;
        scheduleReadinessPolling(500);
      }
    });
  }

  scheduleReadinessPolling(0);
})();
