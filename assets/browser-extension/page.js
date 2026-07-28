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
  if (!initialPage) return;
  const managedNonce = initialPage.nonce;
  const currentManagedPage = () => {
    const details = managedPageDetails();
    return details?.nonce === managedNonce ? details : null;
  };
  const isManagedPage = () => currentManagedPage() !== null;
  const isExecutionReadyPage = () => {
    const details = currentManagedPage();
    return details?.clerkReturn === false;
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
  const TURNSTILE_HIDDEN_STYLE = "position:fixed;z-index:-50;opacity:0;pointer-events:none";
  const HCAPTCHA_ENDPOINT = "https://hcaptcha-endpoint-prod.suno.com";
  const HCAPTCHA_ASSET_HOST = "https://hcaptcha-assets-prod.suno.com";
  const HCAPTCHA_IMAGE_HOST = "https://hcaptcha-imgs-prod.suno.com";
  const HCAPTCHA_REPORT_API = "https://hcaptcha-reportapi-prod.suno.com";
  let activeRequest = null;

  function challengeErrorCode(error) {
    const message = error instanceof Error ? error.message : "";
    if (message.startsWith("interactive_browser_required:")) {
      return "interactive_browser_required";
    }
    if (message.startsWith("silent_challenge_unavailable:")) {
      return "silent_challenge_unavailable";
    }
    if (message === "Challenge SDK did not become ready") {
      return "challenge_sdk_unavailable";
    }
    if (message.includes("unsupported")) return "unsupported_browser";
    if (message.includes("expired")) return "challenge_expired";
    if (message.includes("empty token")) return "invalid_challenge_token";
    return "challenge_failed";
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

  async function solveTurnstile() {
    const body = await managedBody();
    const turnstile = await loadSdk(TURNSTILE_PROVIDER);
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
        let timeout;
        const settle = (callback) => {
          if (settled) return;
          settled = true;
          clearTimeout(timeout);
          callback();
        };
        const fail = (message) => settle(() => reject(new Error(message)));
        const finish = (token) => token
          ? settle(() => resolve(token))
          : fail("Turnstile returned an empty token");
        const scheduleDeadline = (timeoutMs, message) => {
          clearTimeout(timeout);
          timeout = setTimeout(() => fail(message), timeoutMs);
        };
        scheduleDeadline(
          TURNSTILE_IDLE_TIMEOUT_MS,
          "silent_challenge_unavailable: Turnstile produced no callback within 15 seconds"
        );
        widgetId = turnstile.render(container, {
          sitekey: TURNSTILE_SITEKEY,
          execution: "execute",
          appearance: "interaction-only",
          callback: finish,
          "error-callback": () => fail(
            "silent_challenge_unavailable: Turnstile failed silently"
          ),
          "expired-callback": () => fail("Turnstile token expired"),
          "timeout-callback": () => fail(
            "silent_challenge_unavailable: Turnstile could not complete silently"
          ),
          "unsupported-callback": () => fail("Turnstile is unsupported in this browser"),
          "before-interactive-callback": () => fail(
            "interactive_browser_required: Turnstile requires visible browser interaction"
          ),
          "after-interactive-callback": () => {}
        });
        turnstile.execute(widgetId);
      });
    } finally {
      if (widgetId !== undefined) {
        try { turnstile.remove(widgetId); } catch {}
      }
      container.remove();
    }
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
      window.postMessage({ source: "sunox-page-v1", requestId: request.requestId, token }, location.origin);
    } catch (error) {
      window.postMessage({
        source: "sunox-page-v1",
        requestId: request.requestId,
        errorCode: challengeErrorCode(error)
      }, location.origin);
    } finally {
      activeRequest = null;
    }
  });
})();
