(() => {
  if (globalThis.__sunoxBridgeOffscreenLoaded) return;
  globalThis.__sunoxBridgeOffscreenLoaded = true;

  const bridgeConfig = globalThis.SUNOX_BRIDGE_CONFIG;
  const { errorMessage } = globalThis.SUNOX_BRIDGE_SHARED || {};
  const transport = globalThis.SUNOX_BRIDGE_TRANSPORTS?.[bridgeConfig?.transport];
  if (
    bridgeConfig?.schemaVersion !== 1
    || typeof errorMessage !== "function"
    || transport?.contractVersion !== 1
    || typeof transport.claimChallenge !== "function"
    || typeof transport.submitResult !== "function"
  ) {
    throw new Error("Unsupported Sunox Browser Bridge configuration");
  }

  const clientId = `offscreen-${crypto.randomUUID()}`;
  const pageUrl = "https://suno.com/create#sunox-browser-bridge";
  const frameReadyTimeoutMs = 20_000;
  const challengeTimeoutMs = 320_000;
  const frameIdleMs = 20 * 60 * 1000;
  const maxTokenLength = 16_384;
  let busy = false;
  let frame;
  let frameReady;
  let idleTimer;
  let nextScanAt = 0;
  let scanDelayMs = 500;

  chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
    if (
      message?.type !== "sunox-offscreen-ping-v1"
      || sender.id !== chrome.runtime.id
      || sender.tab
    ) return false;
    sendResponse({ type: "sunox-offscreen-pong-v1" });
    return false;
  });

  function destroyFrame() {
    clearTimeout(idleTimer);
    idleTimer = null;
    frame?.remove();
    frame = null;
    frameReady = null;
  }

  function scheduleFrameIdleCleanup() {
    clearTimeout(idleTimer);
    idleTimer = setTimeout(() => {
      if (busy) {
        scheduleFrameIdleCleanup();
      } else {
        destroyFrame();
      }
    }, frameIdleMs);
  }

  function createFrame() {
    const managedFrame = document.createElement("iframe");
    managedFrame.title = "Sunox managed challenge context";
    managedFrame.src = pageUrl;
    managedFrame.sandbox.add("allow-forms", "allow-same-origin", "allow-scripts");
    managedFrame.style.cssText = "width:1280px;height:900px;border:0";

    const ready = new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        chrome.runtime.onMessage.removeListener(onReady);
        reject(new Error("Managed Suno iframe did not become ready within 20 seconds"));
      }, frameReadyTimeoutMs);

      function onReady(message, sender) {
        if (
          message?.type !== "sunox-managed-frame-ready-v1"
          || sender.id !== chrome.runtime.id
          || sender.tab
        ) return false;
        clearTimeout(timeout);
        chrome.runtime.onMessage.removeListener(onReady);
        resolve(managedFrame);
        return false;
      }

      chrome.runtime.onMessage.addListener(onReady);
      document.body.appendChild(managedFrame);
    }).catch((error) => {
      managedFrame.remove();
      if (frame === managedFrame) {
        frame = null;
        frameReady = null;
      }
      throw error;
    });

    frame = managedFrame;
    frameReady = ready;
    return ready;
  }

  async function managedFrame() {
    const current = frameReady || createFrame();
    const ready = await current;
    scheduleFrameIdleCleanup();
    return ready;
  }

  async function executeInFrame(challenge) {
    await managedFrame();
    return await new Promise((resolve) => {
      let settled = false;
      const timeout = setTimeout(() => {
        finish({
          token: null,
          error: "Managed Suno iframe did not return a challenge result within 320 seconds"
        });
      }, challengeTimeoutMs);

      function finish(result) {
        if (settled) return;
        settled = true;
        clearTimeout(timeout);
        chrome.runtime.onMessage.removeListener(onResult);
        resolve(result);
      }

      function onResult(message, sender) {
        if (sender.id !== chrome.runtime.id || sender.tab) return false;
        if (message?.type === "sunox-managed-frame-disconnected-v1") {
          finish({
            token: null,
            error: "Managed Suno iframe messaging port disconnected"
          });
          return false;
        }
        if (
          message?.type !== "sunox-managed-frame-result-v1"
          || message.requestId !== challenge.requestId
        ) return false;
        const token = typeof message.token === "string"
          && message.token.length > 0
          && message.token.length <= maxTokenLength
          ? message.token
          : null;
        finish({
          token,
          error: token
            ? null
            : typeof message.error === "string" && message.error
              ? message.error.slice(0, 900)
              : "Managed Suno iframe returned an invalid challenge token"
        });
        return false;
      }

      chrome.runtime.onMessage.addListener(onResult);
      chrome.runtime.sendMessage({
        type: "sunox-managed-frame-execute-v1",
        requestId: challenge.requestId,
        provider: challenge.provider
      }).then((response) => {
        if (response?.accepted) return;
        finish({
          token: null,
          error: "Managed Suno iframe messaging port is unavailable"
        });
      }).catch((error) => {
        finish({
          token: null,
          error: errorMessage(error)
        });
      });
    });
  }

  async function submitFailure(challenge, error) {
    await transport.submitResult({
      transportReceipt: challenge.transportReceipt,
      requestId: challenge.requestId,
      token: null,
      error: errorMessage(error)
    });
  }

  async function poll() {
    if (busy || Date.now() < nextScanAt) return;
    busy = true;
    let challenge;
    try {
      challenge = await transport.claimChallenge({ clientId, pageUrl });
      scanDelayMs = challenge ? 500 : Math.min(Math.ceil(scanDelayMs * 1.6), 5000);
      nextScanAt = Date.now() + scanDelayMs;
      if (!challenge) return;

      const result = await executeInFrame(challenge);
      const token = result.token;
      if (token) {
        scheduleFrameIdleCleanup();
      } else {
        // Provider SDKs can retain a wedged widget after a timeout or error.
        // Never reuse that page for the next challenge.
        destroyFrame();
      }

      const submitted = await transport.submitResult({
        transportReceipt: challenge.transportReceipt,
        requestId: challenge.requestId,
        token,
        error: token
          ? null
          : result.error || "Managed Browser Bridge returned no challenge result"
      });
      if (!submitted?.accepted) {
        throw new Error("Sunox CLI rejected the Browser Bridge result");
      }
    } catch (error) {
      if (challenge) await submitFailure(challenge, error).catch(() => {});
    } finally {
      busy = false;
    }
  }

  const pollWorker = new Worker("poll-worker.js");
  pollWorker.addEventListener("message", (event) => {
    if (event.data?.type === "sunox-poll") poll();
  });
  poll();
})();
