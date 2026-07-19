(() => {
  if (globalThis.__sunoxBridgeContentLoaded) return;
  globalThis.__sunoxBridgeContentLoaded = true;

  const clientId = crypto.randomUUID();
  let busy = false;

  function executeInPage(challenge) {
    return new Promise((resolve) => {
      const timeout = setTimeout(() => {
        window.removeEventListener("message", onResult);
        resolve({ error: "Challenge page did not return a token within 30 seconds" });
      }, 30000);

      function onResult(event) {
        if (event.source !== window || event.origin !== location.origin) return;
        const result = event.data;
        if (result?.source !== "sunox-page-v1" || result.requestId !== challenge.request_id) return;
        clearTimeout(timeout);
        window.removeEventListener("message", onResult);
        resolve({ token: result.token || null, error: result.error || null });
      }

      window.addEventListener("message", onResult);
      window.postMessage({
        source: "sunox-extension-v1",
        requestId: challenge.request_id,
        provider: challenge.provider
      }, location.origin);
    });
  }

  async function poll() {
    if (busy || location.hostname !== "suno.com") return false;
    busy = true;
    try {
      const challenge = await chrome.runtime.sendMessage({
        type: "sunox-claim",
        clientId,
        pageUrl: location.href
      });
      if (!challenge) return false;

      const result = await executeInPage(challenge);
      await chrome.runtime.sendMessage({
        type: "sunox-result",
        bridgePort: challenge.bridgePort,
        clientNonce: challenge.clientNonce,
        serverNonce: challenge.serverNonce,
        requestId: challenge.request_id,
        token: result.token,
        error: result.error
      });
      return true;
    } catch {
      // The CLI listener is normally absent. Polling failures are expected.
      return false;
    } finally {
      busy = false;
    }
  }

  poll();
  setInterval(poll, 750);
  chrome.runtime.onMessage.addListener((message) => {
    if (message?.type !== "sunox-wake") return false;
    poll();
    return false;
  });
})();
