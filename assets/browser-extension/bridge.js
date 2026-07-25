(() => {
  if (globalThis.__sunoxBridgeContentLoaded) return;
  globalThis.__sunoxBridgeContentLoaded = true;

  const maxTokenLength = 16_384;
  // Bound one recoverable error reset in both idle and interactive states.
  const challengePageTimeoutMs = 315_000;
  let busy = false;

  function executeInPage(challenge) {
    return new Promise((resolve) => {
      const timeout = setTimeout(() => {
        window.removeEventListener("message", onResult);
        resolve({ error: "Challenge page did not return a token within 315 seconds" });
      }, challengePageTimeoutMs);

      function onResult(event) {
        if (event.source !== window || event.origin !== location.origin) return;
        const result = event.data;
        if (result?.source !== "sunox-page-v1" || result.requestId !== challenge.requestId) return;
        clearTimeout(timeout);
        window.removeEventListener("message", onResult);
        const token = typeof result.token === "string"
          && result.token.length > 0
          && result.token.length <= maxTokenLength
          ? result.token
          : null;
        resolve({
          token,
          error: token
            ? null
            : typeof result.error === "string" && result.error
              ? result.error.slice(0, 900)
              : "Challenge page returned an invalid token"
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
    if (busy || location.hostname !== "suno.com") {
      return { token: null, error: "Managed Suno page is busy or unavailable" };
    }
    busy = true;
    try {
      return await executeInPage(challenge);
    } catch (error) {
      return {
        token: null,
        error: error instanceof Error ? error.message : String(error)
      };
    } finally {
      busy = false;
    }
  }

  if (window === window.top || window.parent !== window.top) return;

  let port;

  function connect() {
    const connected = chrome.runtime.connect({ name: "sunox-managed-frame-v1" });
    let reconnect = true;
    port = connected;
    connected.onMessage.addListener(async (message) => {
      if (message?.type === "sunox-managed-frame-rejected-v1") {
        reconnect = false;
        connected.disconnect();
        return;
      }
      if (
        message?.type !== "sunox-managed-frame-execute-v1"
        || !message.requestId
        || !["hcaptcha", "turnstile"].includes(message.provider)
      ) return;

      const result = await execute({
        requestId: message.requestId,
        provider: message.provider
      });
      if (port !== connected) return;
      connected.postMessage({
        type: "sunox-managed-frame-result-v1",
        requestId: message.requestId,
        token: result.token || null,
        error: result.error || null
      });
    });
    connected.onDisconnect.addListener(() => {
      if (port !== connected) return;
      port = null;
      if (reconnect) setTimeout(connect, 500);
    });
  }

  connect();
})();
