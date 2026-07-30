(() => {
  const MANAGED_PAGE_HASH_PREFIX = "#sunox-browser-bridge=";
  const MANAGED_NONCE_PATTERN =
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
  const CONTROLLED_DOCUMENT_ATTRIBUTE = "data-sunox-managed-nonce";
  const PAGE_READY_ATTRIBUTE = "data-sunox-page-ready";

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
        || url.username
        || url.password
      ) return null;
      if (!url.hash.startsWith(MANAGED_PAGE_HASH_PREFIX)) return null;
      const nonce = url.hash.slice(MANAGED_PAGE_HASH_PREFIX.length);
      if (!MANAGED_NONCE_PATTERN.test(nonce)) return null;
      url.hash = "";
      if (url.href !== "https://suno.com/") return null;
      return { nonce, pageUrl: url.href };
    } catch {
      return null;
    }
  };

  const initialPage = managedPageDetails();
  if (!initialPage || globalThis.__sunoxBridgeContentLoaded) return;
  const reportStage = (stage) => {
    chrome.runtime.sendMessage({
      type: "sunox-managed-frame-stage-report-v1",
      nonce: initialPage.nonce,
      stage
    }).catch(() => {});
  };
  const installControlledDocument = (nonce) => {
    if (typeof window.stop !== "function") return false;
    // ISOLATED document_start scripts are not subject to the host page's CSP.
    // Stop before host DOM construction or host script execution, then replace
    // the response with an empty extension-controlled challenge document.
    window.stop();
    if (
      typeof document.createElement !== "function"
    ) return false;
    try {
      let html = document.documentElement;
      if (!html) {
        html = document.createElement("html");
        document.appendChild(html);
      }
      if (typeof html.replaceChildren !== "function") return false;
      const head = document.createElement("head");
      const meta = document.createElement("meta");
      const title = document.createElement("title");
      const body = document.createElement("body");
      meta.setAttribute("charset", "utf-8");
      title.textContent = "Sunox Challenge";
      head.append(meta, title);
      for (const attribute of [...html.attributes]) {
        html.removeAttribute(attribute.name);
      }
      html.replaceChildren(head, body);
      html.setAttribute(CONTROLLED_DOCUMENT_ATTRIBUTE, nonce);
    } catch {
      return false;
    }
    return document.documentElement?.getAttribute(
      CONTROLLED_DOCUMENT_ATTRIBUTE
    ) === nonce;
  };
  if (!installControlledDocument(initialPage.nonce)) {
    reportStage("controlled_document_install_failed");
    return;
  }
  reportStage("controlled_document");
  const mainRunner = document.createElement("script");
  mainRunner.addEventListener("load", () => reportStage("runner_loaded"), {
    once: true
  });
  mainRunner.addEventListener("error", () => reportStage("runner_error"), {
    once: true
  });
  mainRunner.src = chrome.runtime.getURL("page.js");
  mainRunner.dataset.sunoxManagedRunner = initialPage.nonce;
  document.head.appendChild(mainRunner);
  reportStage("runner_injected");
  globalThis.__sunoxBridgeContentLoaded = true;
  const managedNonce = initialPage.nonce;
  const managedPageUrl = initialPage.pageUrl;
  const controlledDocumentReady = () =>
    document.documentElement?.getAttribute(
      CONTROLLED_DOCUMENT_ATTRIBUTE
    ) === managedNonce
    && document.documentElement?.getAttribute(
      PAGE_READY_ATTRIBUTE
    ) === managedNonce;
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
    "turnstile_error_100",
    "turnstile_error_110",
    "turnstile_error_200",
    "turnstile_error_300",
    "turnstile_error_400",
    "turnstile_error_600",
    "turnstile_error_unknown",
    "turnstile_interaction_timeout",
    "turnstile_no_callback",
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
    return details?.nonce === managedNonce
      && details.pageUrl === managedPageUrl
      && controlledDocumentReady()
      ? details
      : null;
  };

  const isExecutionReadyPage = () => {
    return currentManagedPage() !== null;
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
              && (
                challenge.provider === "turnstile"
                || !result.errorCode.startsWith("turnstile_")
              )
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
      const result = await executeInPage(challenge);
      return isExecutionReadyPage()
        ? result
        : { token: null, errorCode: "page_not_ready" };
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
      // The packaged MAIN-world runner is loaded asynchronously. A zero-delay
      // document_start poll can therefore run before page.js has marked the
      // controlled document ready. Keep polling that bounded iframe instead
      // of permanently losing the only Port connection attempt.
      if (!currentManagedPage()) {
        readinessHref = null;
        readinessSince = 0;
        scheduleReadinessPolling();
        return;
      }
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
        reportStage("page_ready");
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
