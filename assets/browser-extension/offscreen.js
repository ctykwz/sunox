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
  let busy = false;
  let nextScanAt = 0;
  let scanDelayMs = 500;

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

      const result = await chrome.runtime.sendMessage({
        type: "sunox-managed-challenge",
        challenge
      });
      const token = typeof result?.token === "string" && result.token
        ? result.token
        : null;
      const submitted = await transport.submitResult({
        transportReceipt: challenge.transportReceipt,
        requestId: challenge.requestId,
        token,
        error: token
          ? null
          : typeof result?.error === "string" && result.error
            ? result.error.slice(0, 900)
            : "Managed Browser Bridge returned no challenge result"
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
