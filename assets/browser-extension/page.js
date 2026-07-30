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
  if (!initialPage) return;
  const managedNonce = initialPage.nonce;
  const managedPageUrl = initialPage.pageUrl;
  const controlledDocumentReady = () =>
    document.documentElement?.getAttribute(
      CONTROLLED_DOCUMENT_ATTRIBUTE
    ) === managedNonce;
  const currentManagedPage = () => {
    const details = managedPageDetails();
    return details?.nonce === managedNonce
      && details.pageUrl === managedPageUrl
      && controlledDocumentReady()
      ? details
      : null;
  };
  const isManagedPage = () => currentManagedPage() !== null;
  const isExecutionReadyPage = () => {
    return currentManagedPage() !== null;
  };
  if (globalThis.__sunoxBridgePageLoaded) return;
  globalThis.__sunoxBridgePageLoaded = true;

  const HCAPTCHA_PROVIDER = "hcaptcha";
  const TURNSTILE_PROVIDER = "turnstile";
  const HCAPTCHA_SITEKEY = "d65453de-3f1a-4aac-9366-a0f06e52b2ce";
  const TURNSTILE_SITEKEY = "0x4AAAAAADI7xDNyj-3LcIbi";
  const HCAPTCHA_SCRIPT = "https://hcaptcha-endpoint-prod.suno.com/1/api.js?render=explicit";
  const TURNSTILE_SCRIPT = "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";
  const CHALLENGE_SDK_READY_TIMEOUT_MS = 15000;
  const HCAPTCHA_SILENT_TIMEOUT_MS = 15000;
  const TURNSTILE_IDLE_TIMEOUT_MS = 15000;
  const TURNSTILE_SHARED_ATTEMPT_BUDGET_MS = 30000;
  const TURNSTILE_NO_CALLBACK_ATTEMPTS = 2;
  const TURNSTILE_HIDDEN_STYLE = "position:fixed;z-index:-50;opacity:0;pointer-events:none";
  const HCAPTCHA_ENDPOINT = "https://hcaptcha-endpoint-prod.suno.com";
  const HCAPTCHA_ASSET_HOST = "https://hcaptcha-assets-prod.suno.com";
  const HCAPTCHA_IMAGE_HOST = "https://hcaptcha-imgs-prod.suno.com";
  const HCAPTCHA_REPORT_API = "https://hcaptcha-reportapi-prod.suno.com";
  const TURNSTILE_ERROR_FAMILIES =
    new Set(["100", "110", "200", "300", "400", "600"]);
  let activeRequest = null;

  function challengeErrorCode(error) {
    const message = error instanceof Error ? error.message : "";
    if (message.startsWith("interactive_browser_required:")) {
      return "interactive_browser_required";
    }
    if (message.startsWith("silent_challenge_unavailable:")) {
      return "silent_challenge_unavailable";
    }
    if (
      message === "turnstile_no_callback"
      || message === "turnstile_interaction_timeout"
      || /^turnstile_error_(?:100|110|200|300|400|600|unknown)$/.test(message)
    ) return message;
    if (message === "Challenge SDK did not become ready") {
      return "challenge_sdk_unavailable";
    }
    if (message.includes("unsupported")) return "unsupported_browser";
    if (message.includes("expired")) return "challenge_expired";
    if (message.includes("empty token")) return "invalid_challenge_token";
    return "challenge_failed";
  }

  function turnstileErrorCode(errorCode) {
    const value = typeof errorCode === "number" || typeof errorCode === "string"
      ? String(errorCode)
      : "";
    const family = /^\d{6}$/.test(value) ? value.slice(0, 3) : "";
    return TURNSTILE_ERROR_FAMILIES.has(family)
      ? `turnstile_error_${family}`
      : "turnstile_error_unknown";
  }

  function waitFor(probe, timeoutMs = CHALLENGE_SDK_READY_TIMEOUT_MS) {
    return new Promise((resolve, reject) => {
      const startedAt = Date.now();
      const interval = setInterval(() => {
        const value = probe();
        if (value) {
          clearInterval(interval);
          resolve(value);
        } else if (Date.now() - startedAt >= timeoutMs) {
          clearInterval(interval);
          reject(new Error("Challenge SDK did not become ready"));
        }
      }, 100);
    });
  }

  async function withTimeout(promise, timeoutMs, message) {
    let timeout;
    try {
      return await Promise.race([
        promise,
        new Promise((_, reject) => {
          timeout = setTimeout(() => reject(new Error(message)), timeoutMs);
        })
      ]);
    } finally {
      clearTimeout(timeout);
    }
  }

  async function loadSdk(kind) {
    const probe = kind === HCAPTCHA_PROVIDER
      ? () => globalThis.hcaptcha?.render && globalThis.hcaptcha
      : () => globalThis.turnstile?.render && globalThis.turnstile?.execute && globalThis.turnstile;
    const available = probe();
    if (available) return available;

    const marker = `script[data-sunox-${kind}]`;
    if (!document.querySelector(marker)) {
      const script = document.createElement("script");
      script.src = kind === HCAPTCHA_PROVIDER ? HCAPTCHA_SCRIPT : TURNSTILE_SCRIPT;
      script.async = true;
      script.defer = true;
      script.dataset[`sunox${kind[0].toUpperCase()}${kind.slice(1)}`] = "true";
      document.head.appendChild(script);
    }
    return waitFor(probe);
  }

  async function managedBody() {
    return waitFor(() => document.body);
  }

  async function solveHcaptcha() {
    const body = await managedBody();
    const hcaptcha = await loadSdk(HCAPTCHA_PROVIDER);
    const container = document.createElement("div");
    container.style.cssText = "position:fixed;top:-9999px;left:-9999px;pointer-events:none";
    body.appendChild(container);
    let widgetId;
    try {
      let interactionRequired;
      const interactiveChallenge = new Promise((_, reject) => {
        interactionRequired = () => reject(new Error(
          "interactive_browser_required: hCaptcha requires visible browser interaction"
        ));
      });
      widgetId = hcaptcha.render(container, {
        sitekey: HCAPTCHA_SITEKEY,
        size: "invisible",
        sentry: false,
        endpoint: HCAPTCHA_ENDPOINT,
        assethost: HCAPTCHA_ASSET_HOST,
        imghost: HCAPTCHA_IMAGE_HOST,
        reportapi: HCAPTCHA_REPORT_API,
        "open-callback": interactionRequired,
        "close-callback": interactionRequired
      });
      let result;
      try {
        result = await withTimeout(
          Promise.race([
            hcaptcha.execute(widgetId, { async: true }),
            interactiveChallenge
          ]),
          HCAPTCHA_SILENT_TIMEOUT_MS,
          "silent_challenge_unavailable: hCaptcha could not complete silently within 15 seconds"
        );
      } catch (error) {
        if (
          error instanceof Error
          && (
            error.message.startsWith("interactive_browser_required:")
            || error.message.startsWith("silent_challenge_unavailable:")
          )
        ) throw error;
        throw new Error(
          "silent_challenge_unavailable: hCaptcha could not complete silently"
        );
      }
      if (!result?.response) throw new Error("hCaptcha returned an empty token");
      return result.response;
    } finally {
      if (widgetId !== undefined) {
        try { hcaptcha.remove(widgetId); } catch {}
      }
      container.remove();
    }
  }

  async function solveTurnstileAttempt(body, turnstile, recoveryDeadline) {
    if (Date.now() >= recoveryDeadline) {
      throw new Error("turnstile_no_callback");
    }
    const container = document.createElement("div");
    // Match Suno's current generation widget for the silent attempt. Moving
    // the container outside the viewport can prevent the managed challenge
    // from starting; any request for interaction is rejected immediately.
    container.style.cssText = TURNSTILE_HIDDEN_STYLE;
    body.appendChild(container);
    let widgetId;
    try {
      return await new Promise((resolve, reject) => {
        let settled = false;
        let terminalErrorCode = "turnstile_no_callback";
        let timeout;
        const settle = (callback) => {
          if (settled) return;
          settled = true;
          clearTimeout(timeout);
          callback();
        };
        const fail = (message) => settle(() => reject(new Error(message)));
        const finish = (token) => {
          if (Date.now() > recoveryDeadline) {
            fail(terminalErrorCode);
          } else if (token) {
            settle(() => resolve(token));
          } else {
            fail("Turnstile returned an empty token");
          }
        };
        const scheduleDeadline = (timeoutMs) => {
          clearTimeout(timeout);
          const remainingMs = Math.max(0, recoveryDeadline - Date.now());
          timeout = setTimeout(
            () => fail(terminalErrorCode),
            Math.min(timeoutMs, remainingMs)
          );
        };
        scheduleDeadline(TURNSTILE_IDLE_TIMEOUT_MS);
        try {
          widgetId = turnstile.render(container, {
            sitekey: TURNSTILE_SITEKEY,
            execution: "execute",
            appearance: "interaction-only",
            callback: finish,
            "error-callback": (errorCode) => {
              if (settled) return false;
              terminalErrorCode = turnstileErrorCode(errorCode);
              // Match Suno Web's current recovery contract: keep waiting while
              // Turnstile performs its default automatic retry. The bounded
              // absolute deadline still returns only the allowlisted family.
              scheduleDeadline(TURNSTILE_IDLE_TIMEOUT_MS);
              return false;
            },
            "expired-callback": () => fail("Turnstile token expired"),
            "timeout-callback": () => {
              if (settled) return;
              terminalErrorCode = "turnstile_interaction_timeout";
            },
            "unsupported-callback": () => fail("Turnstile is unsupported in this browser"),
            "before-interactive-callback": () => fail(
              "interactive_browser_required: Turnstile requires visible browser interaction"
            ),
            "after-interactive-callback": () => {}
          });
          if (!settled) turnstile.execute(widgetId);
        } catch {
          fail("Turnstile execution failed");
        }
      });
    } finally {
      let cleanupFailed = false;
      if (widgetId !== undefined) {
        try {
          turnstile.remove(widgetId);
        } catch {
          cleanupFailed = true;
        }
      }
      try {
        container.remove();
      } catch {
        cleanupFailed = true;
      }
      if (cleanupFailed) {
        throw new Error("Turnstile widget cleanup failed");
      }
    }
  }

  async function solveTurnstile() {
    const body = await managedBody();
    const turnstile = await loadSdk(TURNSTILE_PROVIDER);
    // The content bridge owns a 50-second page deadline. A cold SDK load may
    // consume 15 seconds, so both fresh widgets share one absolute 30-second
    // budget after the SDK is ready. The second widget keeps the remaining
    // time for either a token or the provider's bounded same-widget recovery.
    const recoveryDeadline =
      Date.now() + TURNSTILE_SHARED_ATTEMPT_BUDGET_MS;
    for (
      let attempt = 1;
      attempt <= TURNSTILE_NO_CALLBACK_ATTEMPTS;
      attempt += 1
    ) {
      try {
        return await solveTurnstileAttempt(
          body,
          turnstile,
          recoveryDeadline
        );
      } catch (error) {
        if (
          !(error instanceof Error)
          || error.message !== "turnstile_no_callback"
          || attempt === TURNSTILE_NO_CALLBACK_ATTEMPTS
        ) throw error;
        // Suno Web removes the prior generation widget before each queued
        // verification. Recreate it once for the CLI when the provider emits
        // no callback at all; never create a fresh widget for interactive or
        // classified failures.
      }
    }
    throw new Error("turnstile_no_callback");
  }

  window.addEventListener("message", async (event) => {
    if (!isManagedPage()) return;
    if (event.source !== window || event.origin !== location.origin) return;
    const request = event.data;
    if (request?.source !== "sunox-extension-v1" || !request.requestId || activeRequest) return;
    if (request.provider !== HCAPTCHA_PROVIDER && request.provider !== TURNSTILE_PROVIDER) return;
    if (!isExecutionReadyPage()) {
      window.postMessage({
        source: "sunox-page-v1",
        requestId: request.requestId,
        errorCode: "page_not_ready"
      }, location.origin);
      return;
    }

    activeRequest = request.requestId;
    try {
      const token = request.provider === TURNSTILE_PROVIDER
        ? await solveTurnstile()
        : await solveHcaptcha();
      window.postMessage(
        isExecutionReadyPage()
          ? {
              source: "sunox-page-v1",
              requestId: request.requestId,
              token
            }
          : {
              source: "sunox-page-v1",
              requestId: request.requestId,
              errorCode: "page_not_ready"
            },
        location.origin
      );
    } catch (error) {
      window.postMessage({
        source: "sunox-page-v1",
        requestId: request.requestId,
        errorCode: isExecutionReadyPage()
          ? challengeErrorCode(error)
          : "page_not_ready"
      }, location.origin);
    } finally {
      activeRequest = null;
    }
  });
  if (controlledDocumentReady()) {
    document.documentElement.setAttribute(
      PAGE_READY_ATTRIBUTE,
      managedNonce
    );
  }
})();
