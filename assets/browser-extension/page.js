(() => {
  if (globalThis.__sunoxBridgePageLoaded) return;
  globalThis.__sunoxBridgePageLoaded = true;
  if (window === window.top || window.parent !== window.top) return;

  const HCAPTCHA_PROVIDER = "hcaptcha";
  const TURNSTILE_PROVIDER = "turnstile";
  const HCAPTCHA_SITEKEY = "d65453de-3f1a-4aac-9366-a0f06e52b2ce";
  const TURNSTILE_SITEKEY = "0x4AAAAAADI7xDNyj-3LcIbi";
  const HCAPTCHA_SCRIPT = "https://hcaptcha-endpoint-prod.suno.com/1/api.js?render=explicit";
  const TURNSTILE_SCRIPT = "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";
  const CHALLENGE_SDK_READY_TIMEOUT_MS = 15000;
  const TURNSTILE_IDLE_TIMEOUT_MS = 15000;
  const TURNSTILE_INTERACTIVE_TIMEOUT_MS = 120000;
  const HCAPTCHA_ENDPOINT = "https://hcaptcha-endpoint-prod.suno.com";
  const HCAPTCHA_ASSET_HOST = "https://hcaptcha-assets-prod.suno.com";
  const HCAPTCHA_IMAGE_HOST = "https://hcaptcha-imgs-prod.suno.com";
  const HCAPTCHA_REPORT_API = "https://hcaptcha-reportapi-prod.suno.com";
  let activeRequest = null;

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

  async function solveHcaptcha() {
    const hcaptcha = await loadSdk(HCAPTCHA_PROVIDER);
    const container = document.createElement("div");
    container.style.cssText = "position:fixed;top:-9999px;left:-9999px;pointer-events:none";
    document.body.appendChild(container);
    let widgetId;
    try {
      widgetId = hcaptcha.render(container, {
        sitekey: HCAPTCHA_SITEKEY,
        size: "invisible",
        sentry: false,
        endpoint: HCAPTCHA_ENDPOINT,
        assethost: HCAPTCHA_ASSET_HOST,
        imghost: HCAPTCHA_IMAGE_HOST,
        reportapi: HCAPTCHA_REPORT_API
      });
      const result = await withTimeout(
        hcaptcha.execute(widgetId, { async: true }),
        25_000,
        "hCaptcha produced no token"
      );
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
    const turnstile = await loadSdk(TURNSTILE_PROVIDER);
    const container = document.createElement("div");
    container.style.cssText = "position:fixed;top:-9999px;left:-9999px;pointer-events:none";
    document.body.appendChild(container);
    let widgetId;
    try {
      return await new Promise((resolve, reject) => {
        let settled = false;
        let interactive = false;
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
          "Turnstile produced no callback within 15 seconds"
        );
        widgetId = turnstile.render(container, {
          sitekey: TURNSTILE_SITEKEY,
          execution: "execute",
          appearance: "interaction-only",
          callback: finish,
          "error-callback": (code) => scheduleDeadline(
            interactive ? TURNSTILE_INTERACTIVE_TIMEOUT_MS : TURNSTILE_IDLE_TIMEOUT_MS,
            `Turnstile did not recover after error ${code || "unknown"}`
          ),
          "expired-callback": () => fail("Turnstile token expired"),
          "timeout-callback": () => {},
          "unsupported-callback": () => fail("Turnstile is unsupported in this browser"),
          "before-interactive-callback": () => {
            interactive = true;
            scheduleDeadline(
              TURNSTILE_INTERACTIVE_TIMEOUT_MS,
              "Turnstile interactive challenge was abandoned"
            );
          },
          "after-interactive-callback": () => {
            interactive = false;
            scheduleDeadline(
              TURNSTILE_IDLE_TIMEOUT_MS,
              "Turnstile produced no token after interactive challenge completed"
            );
          }
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
    if (event.source !== window || event.origin !== location.origin) return;
    const request = event.data;
    if (request?.source !== "sunox-extension-v1" || !request.requestId || activeRequest) return;
    if (request.provider !== HCAPTCHA_PROVIDER && request.provider !== TURNSTILE_PROVIDER) return;

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
        error: error instanceof Error ? error.message : String(error)
      }, location.origin);
    } finally {
      activeRequest = null;
    }
  });
})();
