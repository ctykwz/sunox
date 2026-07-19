(() => {
  if (globalThis.__sunoxBridgePageLoaded) return;
  globalThis.__sunoxBridgePageLoaded = true;

  const HCAPTCHA_SITEKEY = "d65453de-3f1a-4aac-9366-a0f06e52b2ce";
  const TURNSTILE_SITEKEY = "0x4AAAAAADI7xDNyj-3LcIbi";
  const HCAPTCHA_SCRIPT = "https://hcaptcha-endpoint-prod.suno.com/1/api.js?render=explicit";
  const TURNSTILE_SCRIPT = "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";
  let activeRequest = null;

  function waitFor(probe, timeoutMs = 15000) {
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

  async function loadSdk(kind) {
    const probe = kind === "hcaptcha"
      ? () => globalThis.hcaptcha?.render && globalThis.hcaptcha
      : () => globalThis.turnstile?.render && globalThis.turnstile?.execute && globalThis.turnstile;
    const available = probe();
    if (available) return available;

    const marker = `script[data-sunox-${kind}]`;
    if (!document.querySelector(marker)) {
      const script = document.createElement("script");
      script.src = kind === "hcaptcha" ? HCAPTCHA_SCRIPT : TURNSTILE_SCRIPT;
      script.async = true;
      script.defer = true;
      script.dataset[`sunox${kind[0].toUpperCase()}${kind.slice(1)}`] = "true";
      document.head.appendChild(script);
    }
    return waitFor(probe);
  }

  async function solveHcaptcha() {
    const hcaptcha = await loadSdk("hcaptcha");
    const container = document.createElement("div");
    container.style.cssText = "position:fixed;top:-9999px;left:-9999px;pointer-events:none";
    document.body.appendChild(container);
    let widgetId;
    try {
      widgetId = hcaptcha.render(container, {
        sitekey: HCAPTCHA_SITEKEY,
        size: "invisible",
        sentry: false,
        endpoint: "https://hcaptcha-endpoint-prod.suno.com",
        assethost: "https://hcaptcha-assets-prod.suno.com",
        imghost: "https://hcaptcha-imgs-prod.suno.com",
        reportapi: "https://hcaptcha-reportapi-prod.suno.com"
      });
      const result = await hcaptcha.execute(widgetId, { async: true });
      return result?.response || "";
    } finally {
      if (widgetId !== undefined) {
        try { hcaptcha.remove(widgetId); } catch {}
      }
      container.remove();
    }
  }

  async function solveTurnstile() {
    const turnstile = await loadSdk("turnstile");
    const container = document.createElement("div");
    container.style.cssText = "position:fixed;top:-9999px;left:-9999px;pointer-events:none";
    document.body.appendChild(container);
    let widgetId;
    try {
      return await new Promise((resolve, reject) => {
        let settled = false;
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
        const timeout = setTimeout(() => fail("Turnstile produced no token"), 20000);
        widgetId = turnstile.render(container, {
          sitekey: TURNSTILE_SITEKEY,
          execution: "execute",
          callback: finish,
          "error-callback": (code) => fail(`Turnstile error ${code || "unknown"}`),
          "expired-callback": () => fail("Turnstile token expired"),
          "timeout-callback": () => fail("Turnstile challenge timed out"),
          "unsupported-callback": () => fail("Turnstile is unsupported in this browser")
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
    if (request.provider !== "hcaptcha" && request.provider !== "turnstile") return;

    activeRequest = request.requestId;
    try {
      const token = request.provider === "turnstile"
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
