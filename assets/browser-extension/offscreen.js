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
  const pageUrl = "https://suno.com/create";
  const managedFrameReadyTimeoutMs = 45_000;
  const managedFrameResultTimeoutMs = 65_000;
  const managedPageHashPrefix = "#sunox-browser-bridge=";
  const managedNoncePattern =
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
  const pollWorkerStaleMs = 5_000;
  const maxTokenLength = 16_384;
  let busy = false;
  let busySince = 0;
  let pollingReady = false;
  let lastPollWorkerTickAt = 0;
  let nextScanAt = 0;
  let pollWorker;
  let pollWorkerGeneration = 0;
  let pollWorkerRestartDelayMs = 250;
  let pollWorkerRestartTimer;
  let scanDelayMs = 500;

  chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
    if (
      !["sunox-offscreen-start-v1", "sunox-offscreen-ping-v1"].includes(
        message?.type
      )
      || sender.id !== chrome.runtime.id
      || sender.tab
    ) return false;
    if (message.type === "sunox-offscreen-start-v1") {
      pollingReady = true;
      sendResponse({ accepted: true });
      poll();
      return false;
    }
    const pollWorkerAgeMs = lastPollWorkerTickAt > 0
      ? Math.max(0, Date.now() - lastPollWorkerTickAt)
      : null;
    sendResponse({
      busy: busySince > 0,
      busySince: busySince > 0 ? busySince : null,
      type: "sunox-offscreen-pong-v1",
      pollWorkerAgeMs,
      pollWorkerHealthy: pollWorkerAgeMs !== null
        && pollWorkerAgeMs <= pollWorkerStaleMs
    });
    return false;
  });

  async function executeInManagedFrame(challenge) {
    const environment = await chrome.runtime.sendMessage({
      type: "sunox-frame-environment-prepare-v1"
    }).catch(() => null);
    if (environment?.accepted !== true) {
      return {
        token: null,
        error:
          "Managed Suno challenge environment is unavailable; the current embedding policy could not be verified"
      };
    }

    const nonce = crypto.randomUUID();
    if (!managedNoncePattern.test(nonce)) {
      return {
        token: null,
        error: "Managed Suno frame could not create a valid request nonce"
      };
    }

    const managedFrame = document.createElement("iframe");
    let frameErrorEvents = 0;
    let frameLoadEvents = 0;
    managedFrame.addEventListener("error", () => {
      frameErrorEvents += 1;
    });
    managedFrame.addEventListener("load", () => {
      frameLoadEvents += 1;
    });
    managedFrame.title = "Sunox managed challenge context";
    managedFrame.src = `${pageUrl}${managedPageHashPrefix}${nonce}`;
    managedFrame.sandbox.add("allow-forms", "allow-same-origin", "allow-scripts");
    // The offscreen document itself has no browser surface. Keep a normal
    // layout viewport so visibility-sensitive provider code can measure the
    // widget without creating a tab or top-level window.
    managedFrame.style.cssText = "width:1280px;height:900px;border:0";

    return await new Promise((resolve) => {
      let executeRequested = false;
      let settled = false;
      let timeout = setTimeout(() => {
        finish({
          token: null,
          error:
            `Managed Suno frame did not become ready within 45 seconds (load_events=${frameLoadEvents}, error_events=${frameErrorEvents})`
        });
      }, managedFrameReadyTimeoutMs);

      function finish(result) {
        if (settled) return;
        settled = true;
        clearTimeout(timeout);
        chrome.runtime.onMessage.removeListener(onFrameMessage);
        managedFrame.remove();
        resolve(result);
      }

      function onFrameMessage(message, sender) {
        if (sender.id !== chrome.runtime.id || sender.tab) return false;
        if (
          message?.type === "sunox-managed-frame-diagnostic-v1"
          && message.nonce === nonce
          && typeof message.reason === "string"
          && /^[a-z_]{1,64}$/.test(message.reason)
        ) {
          finish({
            token: null,
            error: `Managed Suno frame port was rejected (${message.reason})`
          });
          return false;
        }
        if (
          message?.type === "sunox-managed-frame-ready-v2"
          && message.nonce === nonce
          && !executeRequested
        ) {
          executeRequested = true;
          clearTimeout(timeout);
          timeout = setTimeout(() => {
            finish({
              token: null,
              error: "Managed Suno frame did not return a challenge result within 65 seconds"
            });
          }, managedFrameResultTimeoutMs);
          chrome.runtime.sendMessage({
            type: "sunox-managed-frame-execute-v2",
            requestId: challenge.requestId,
            provider: challenge.provider,
            nonce
          }).then((response) => {
            if (response?.accepted) return;
            finish({
              token: null,
              error: typeof response?.error === "string" && response.error
                ? response.error.slice(0, 900)
                : "Managed Suno frame messaging port is unavailable"
            });
          }).catch((error) => {
            finish({
              token: null,
              error: errorMessage(error)
            });
          });
          return false;
        }
        if (
          message?.type === "sunox-managed-frame-disconnected-v2"
          && message.nonce === nonce
          && executeRequested
        ) {
          finish({
            token: null,
            error: "Managed Suno frame disconnected after challenge execution began"
          });
          return false;
        }
        if (
          message?.type !== "sunox-managed-frame-result-v2"
          || message.nonce !== nonce
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
              : "Managed Suno frame returned an invalid challenge token"
        });
        return false;
      }

      chrome.runtime.onMessage.addListener(onFrameMessage);
      document.body.appendChild(managedFrame);
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
    if (!pollingReady || busy || Date.now() < nextScanAt) return;
    busy = true;
    // The CLI marks a request claimed before this promise resolves. Preserve
    // the complete claim/window/result operation across service-worker wakes.
    busySince = Date.now();
    let challenge;
    try {
      challenge = await transport.claimChallenge({ clientId, pageUrl });
      scanDelayMs = challenge ? 500 : Math.min(Math.ceil(scanDelayMs * 1.6), 5000);
      nextScanAt = Date.now() + scanDelayMs;
      if (!challenge) return;

      const result = await executeInManagedFrame(challenge);
      const token = result.token;
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
      busySince = 0;
    }
  }

  function schedulePollWorkerRestart() {
    if (pollWorkerRestartTimer) return;
    lastPollWorkerTickAt = 0;
    const delayMs = pollWorkerRestartDelayMs;
    pollWorkerRestartDelayMs = Math.min(
      pollWorkerRestartDelayMs * 2,
      pollWorkerStaleMs
    );
    pollWorkerRestartTimer = setTimeout(() => {
      pollWorkerRestartTimer = null;
      startPollWorker();
    }, delayMs);
  }

  function startPollWorker() {
    const generation = ++pollWorkerGeneration;
    let worker;
    try {
      worker = new Worker("poll-worker.js");
    } catch {
      schedulePollWorkerRestart();
      return;
    }
    pollWorker = worker;

    const recover = () => {
      if (pollWorker !== worker || generation !== pollWorkerGeneration) return;
      pollWorker = null;
      worker.terminate?.();
      schedulePollWorkerRestart();
    };
    worker.addEventListener("message", (event) => {
      if (
        pollWorker !== worker
        || generation !== pollWorkerGeneration
        || event.data?.type !== "sunox-poll"
      ) return;
      lastPollWorkerTickAt = Date.now();
      pollWorkerRestartDelayMs = 250;
      poll();
    });
    worker.addEventListener("error", recover);
    worker.addEventListener("messageerror", recover);
  }

  startPollWorker();
})();
