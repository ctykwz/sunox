(() => {
  if (globalThis.__sunoxBridgeContentLoaded) return;
  globalThis.__sunoxBridgeContentLoaded = true;

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
        if (result?.source !== "sunox-page-v1" || result.requestId !== challenge.requestId) return;
        clearTimeout(timeout);
        window.removeEventListener("message", onResult);
        resolve({ token: result.token || null, error: result.error || null });
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

  chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
    if (message?.type !== "sunox-execute") return false;
    execute(message.challenge).then(sendResponse);
    return true;
  });
})();
