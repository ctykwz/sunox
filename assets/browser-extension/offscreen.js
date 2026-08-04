(() => {
  if (globalThis.__sunoxBridgeOffscreenLoaded) return;
  globalThis.__sunoxBridgeOffscreenLoaded = true;

  const bridgeConfig = globalThis.SUNOX_BRIDGE_CONFIG;
  const { errorMessage } = globalThis.SUNOX_BRIDGE_SHARED || {};
  const transport = globalThis.SUNOX_BRIDGE_TRANSPORTS?.[bridgeConfig?.transport];
  const runtimeBuild = bridgeConfig?.loopback?.runtimeBuild;
  if (
    bridgeConfig?.schemaVersion !== 1
    || typeof runtimeBuild !== "string"
    || !/^\d+\.\d+\.\d+$/.test(runtimeBuild)
    || typeof errorMessage !== "function"
    || transport?.contractVersion !== 1
    || typeof transport.claimChallenge !== "function"
    || typeof transport.submitResult !== "function"
  ) {
    throw new Error("Unsupported Sunox Browser Bridge configuration");
  }

  const clientId = `offscreen-${crypto.randomUUID()}`;
  const claimPageUrl = "https://suno.com/";
  const managedPageOrigin = "https://suno.com";
  // The fixed root carrier may need one fresh retry before its controlled
  // content-script handshake appears. Both attempts share one absolute
  // readiness deadline, and a retry is forbidden after provider execution.
  const managedFrameWarmupGraceMs = 3_000;
  const managedFramePrepareTimeoutMs = 9_000;
  const managedFrameReadyTimeoutMs = 45_000;
  const managedFrameNetworkDiagnosticGraceMs = 1_000;
  const managedFrameReleaseTimeoutMs = 500;
  const managedFrameResultTimeoutMs = 65_000;
  const managedFrameReadyAttempts = 2;
  const managedPageHashPrefix = "#sunox-browser-bridge=";
  const managedNoncePattern =
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
  const managedFrameResultAckType = "sunox-managed-frame-result-ack-v1";
  const retryableManagedNetworkReasons = new Set([
    "managed_network_connection",
    "managed_network_name_resolution",
    "managed_network_protocol",
    "managed_network_timeout"
  ]);
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

  async function runtimeMessageBeforeDeadline(message, deadline) {
    let timeout;
    const remainingMs = Math.max(0, deadline - Date.now());
    try {
      return await Promise.race([
        chrome.runtime.sendMessage(message)
          .then((value) => ({ failed: false, timedOut: false, value }))
          .catch(() => ({ failed: true, timedOut: false, value: null })),
        new Promise((resolve) => {
          timeout = setTimeout(
            () => resolve({ failed: false, timedOut: true, value: null }),
            remainingMs
          );
        })
      ]);
    } finally {
      clearTimeout(timeout);
    }
  }

  function cleanManagedPageUrl(value) {
    if (typeof value !== "string" || value.length > 2_048) return null;
    try {
      const url = new URL(value);
      if (
        url.origin !== managedPageOrigin
        || url.username
        || url.password
        || url.search
        || url.hash
      ) return null;
      return url.href === claimPageUrl ? url.href : null;
    } catch {
      return null;
    }
  }

  chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
    if (
      !["sunox-offscreen-start-v1", "sunox-offscreen-ping-v1"].includes(
        message?.type
      )
      || sender.id !== chrome.runtime.id
      || sender.tab
    ) return false;
    if (message.type === "sunox-offscreen-start-v1") {
      if (message.runtimeBuild !== runtimeBuild) {
        sendResponse({
          accepted: false,
          clientId,
          runtimeBuild
        });
        return false;
      }
      pollingReady = true;
      sendResponse({ accepted: true, clientId, runtimeBuild });
      poll();
      return false;
    }
    const pollWorkerAgeMs = lastPollWorkerTickAt > 0
      ? Math.max(0, Date.now() - lastPollWorkerTickAt)
      : null;
    sendResponse({
      busy: busySince > 0,
      busySince: busySince > 0 ? busySince : null,
      clientId,
      runtimeBuild,
      type: "sunox-offscreen-pong-v1",
      pollWorkerAgeMs,
      pollWorkerHealthy: pollWorkerAgeMs !== null
        && pollWorkerAgeMs <= pollWorkerStaleMs
    });
    return false;
  });

  async function executeInManagedFrame(challenge) {
    let previousNonce = null;
    const attemptedNonces = [];
    let readinessDeadline = null;
    try {
      for (
        let attempt = 1;
        attempt <= managedFrameReadyAttempts;
        attempt += 1
      ) {
        const nonce = crypto.randomUUID();
        if (!managedNoncePattern.test(nonce)) {
          return {
            token: null,
            error: "Managed Suno frame could not create a valid request nonce"
          };
        }
        // The service worker may install/rotate the environment even if the
        // response channel is lost. Record ownership before sending so every
        // possibly committed nonce is released in the terminal path.
        attemptedNonces.push(nonce);
        const prepareDeadline = readinessDeadline
          ?? Date.now() + managedFramePrepareTimeoutMs;
        const prepared = await runtimeMessageBeforeDeadline({
          type: "sunox-frame-environment-prepare-v1",
          clientId,
          nonce,
          previousNonce,
          provider: challenge.provider
        }, prepareDeadline);
        if (prepared.timedOut) {
          return {
            token: null,
            error: readinessDeadline
              ? "Managed Suno challenge environment rotation exceeded the shared readiness deadline"
              : "Managed Suno challenge environment preparation timed out"
          };
        }
        const environment = prepared.value;
        const pageUrl = cleanManagedPageUrl(environment?.pageUrl);
        if (environment?.accepted !== true || !pageUrl) {
          return {
            token: null,
            error:
              "Managed Suno challenge environment is unavailable; the current embedding policy could not be verified"
          };
        }
        readinessDeadline ??= Date.now() + managedFrameReadyTimeoutMs;
        const result = await executeInManagedFrameAttempt(
          challenge,
          attempt,
          readinessDeadline,
          pageUrl,
          nonce
        );
        if (result.retryReady && attempt < managedFrameReadyAttempts) {
          previousNonce = nonce;
          continue;
        }
        return {
          token: result.token || null,
          error: result.error || null
        };
      }
      return {
        token: null,
        error: "Managed Suno frame exhausted its readiness attempts"
      };
    } finally {
      for (const nonce of attemptedNonces.reverse()) {
        const released = await runtimeMessageBeforeDeadline({
          type: "sunox-frame-environment-release-v1",
          clientId,
          nonce
        }, Date.now() + managedFrameReleaseTimeoutMs);
        // The release message has already been dispatched. If Chrome's rule
        // update is still pending, its handler owns eventual fail-closed
        // cleanup; do not race it with a second release for an older nonce.
        if (
          released.timedOut
          || released.failed
          || released.value?.accepted !== true
        ) break;
      }
    }
  }

  async function executeInManagedFrameAttempt(
    challenge,
    attempt,
    readinessDeadline,
    pageUrl,
    nonce
  ) {
    const managedFrame = document.createElement("iframe");
    let frameErrorEvents = 0;
    let frameLoadEvents = 0;
    let executeRequested = false;
    let diagnosticGraceTimer = null;
    let domErrorPending = false;
    let lastStage = "none";
    let readinessGraceTimer = null;
    let retirementPending = false;
    let retirementTimeout = null;
    let settled = false;
    let timeout = null;

    managedFrame.title = "Sunox managed challenge context";
    // Use the current Chrome profile's Suno context so the provider observes
    // the same browser state as Suno Web. The extension still stops and
    // replaces the host response before its scripts run.
    managedFrame.referrerPolicy = "strict-origin-when-cross-origin";
    managedFrame.sandbox.add("allow-forms", "allow-same-origin", "allow-scripts");
    const managedFrameUrl = new URL(pageUrl);
    managedFrameUrl.hash = `${managedPageHashPrefix.slice(1)}${nonce}`;
    // The offscreen document itself has no browser surface. Keep a normal
    // layout viewport so visibility-sensitive provider code can measure the
    // widget without creating a tab or top-level window.
    managedFrame.style.cssText = "width:1280px;height:900px;border:0";
    managedFrame.src = managedFrameUrl.href;

    return await new Promise((resolve) => {
      const onFrameError = () => {
        frameErrorEvents += 1;
        if (settled || domErrorPending) return;
        domErrorPending = true;
        clearTimeout(timeout);
        clearTimeout(readinessGraceTimer);
        diagnosticGraceTimer = setTimeout(() => {
          finish({
            token: null,
            error: `Managed Suno frame failed to load without a classified network diagnostic (attempt=${attempt}/${managedFrameReadyAttempts}, stage=${lastStage})`
          });
        }, Math.min(
          managedFrameNetworkDiagnosticGraceMs,
          Math.max(0, readinessDeadline - Date.now())
        ));
      };
      const onFrameLoad = () => {
        frameLoadEvents += 1;
      };
      const remainingReadyMs = Math.max(0, readinessDeadline - Date.now());
      if (attempt === 1) {
        readinessGraceTimer = setTimeout(() => {
          if (settled || executeRequested) return;
          finishForRetry(
            `Managed Suno frame produced no ready port during warmup (attempt=${attempt}/${managedFrameReadyAttempts}, stage=${lastStage}, load_events=${frameLoadEvents}, error_events=${frameErrorEvents})`
          );
        }, Math.min(managedFrameWarmupGraceMs, remainingReadyMs));
      }
      timeout = setTimeout(() => {
        finish({
          token: null,
          error:
            `Managed Suno frame did not become ready within the shared ${Math.ceil(managedFrameReadyTimeoutMs / 1000)} second deadline (attempt=${attempt}/${managedFrameReadyAttempts}, stage=${lastStage}, load_events=${frameLoadEvents}, error_events=${frameErrorEvents})`
        });
      }, remainingReadyMs);

      function finish(result) {
        if (settled) return;
        settled = true;
        clearTimeout(timeout);
        clearTimeout(diagnosticGraceTimer);
        clearTimeout(readinessGraceTimer);
        clearTimeout(retirementTimeout);
        chrome.runtime.onMessage.removeListener(onFrameMessage);
        managedFrame.removeEventListener("error", onFrameError);
        managedFrame.removeEventListener("load", onFrameLoad);
        managedFrame.remove();
        resolve(result);
      }

      function finishForRetry(error) {
        if (
          settled
          || executeRequested
          || retirementPending
          || attempt !== 1
        ) return;
        retirementPending = true;
        clearTimeout(timeout);
        clearTimeout(diagnosticGraceTimer);
        clearTimeout(readinessGraceTimer);
        const retirementBudgetMs = Math.min(
          1_000,
          Math.max(0, readinessDeadline - Date.now())
        );
        retirementTimeout = setTimeout(() => {
          finish({
            token: null,
            error:
              "Managed Suno frame could not retire its first readiness attempt safely"
          });
        }, retirementBudgetMs);
        chrome.runtime.sendMessage({
          type: "sunox-frame-environment-retire-v1",
          clientId,
          nonce
        }).then((response) => {
          if (settled) return;
          if (response?.accepted !== true) {
            finish({
              token: null,
              error:
                "Managed Suno frame could not retire its first readiness attempt safely"
            });
            return;
          }
          finish({ token: null, retryReady: true, error });
        }).catch(() => {
          finish({
            token: null,
            error:
              "Managed Suno frame could not retire its first readiness attempt safely"
          });
        });
      }

      function onFrameMessage(message, sender, sendResponse) {
        if (sender.id !== chrome.runtime.id || sender.tab) return false;
        const isManagedNetworkDiagnostic =
          message?.type === "sunox-managed-frame-diagnostic-v1"
          && message.nonce === nonce
          && typeof message.reason === "string"
          && /^[a-z_]{1,64}$/.test(message.reason);
        if (domErrorPending && !isManagedNetworkDiagnostic) return false;
        if (
          message?.type === "sunox-managed-frame-stage-v1"
          && message.nonce === nonce
          && [
            "controlled_document",
            "controlled_document_install_failed",
            "content_report_pending_network",
            "network_request_bound",
            "network_request_headers_verified",
            "network_response_verified",
            "page_ready",
            "runner_error",
            "runner_injected",
            "runner_loaded"
          ].includes(message.stage)
        ) {
          lastStage = message.stage;
          if (retirementPending) return false;
          if (
            message.stage === "controlled_document_install_failed"
            || message.stage === "runner_error"
          ) {
            const error =
              `Managed Suno frame startup failed (attempt=${attempt}/${managedFrameReadyAttempts}, stage=${lastStage})`;
            if (attempt === 1 && !executeRequested) {
              finishForRetry(error);
            } else {
              finish({ token: null, error });
            }
          }
          return false;
        }
        if (retirementPending) return false;
        if (
          isManagedNetworkDiagnostic
        ) {
          const error =
            `Managed Suno frame network request failed (${message.reason})`;
          if (
            attempt === 1
            && !executeRequested
            && retryableManagedNetworkReasons.has(message.reason)
          ) {
            finishForRetry(error);
          } else {
            finish({ token: null, error });
          }
          return false;
        }
        if (
          message?.type === "sunox-managed-frame-ready-v2"
          && message.nonce === nonce
          && !executeRequested
        ) {
          executeRequested = true;
          clearTimeout(timeout);
          clearTimeout(readinessGraceTimer);
          timeout = setTimeout(() => {
            finish({
              token: null,
              error: "Managed Suno frame did not return a challenge result within 65 seconds"
            });
          }, managedFrameResultTimeoutMs);
          chrome.runtime.sendMessage({
            type: "sunox-managed-frame-execute-v2",
            clientId,
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
        sendResponse({
          accepted: true,
          type: managedFrameResultAckType,
          nonce,
          requestId: challenge.requestId
        });
        return false;
      }

      managedFrame.addEventListener("error", onFrameError);
      managedFrame.addEventListener("load", onFrameLoad);
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
    let terminalSubmissionStarted = false;
    try {
      challenge = await transport.claimChallenge({
        clientId,
        pageUrl: claimPageUrl
      });
      scanDelayMs = challenge ? 500 : Math.min(Math.ceil(scanDelayMs * 1.6), 5000);
      nextScanAt = Date.now() + scanDelayMs;
      if (!challenge) return;

      const result = await executeInManagedFrame(challenge);
      const token = result.token;
      const terminalResult = {
        transportReceipt: challenge.transportReceipt,
        requestId: challenge.requestId,
        token,
        error: token
          ? null
          : result.error || "Managed Browser Bridge returned no challenge result"
      };
      terminalSubmissionStarted = true;
      const submitted = await transport.submitResult(terminalResult);
      if (!submitted?.accepted) {
        return;
      }
    } catch (error) {
      if (challenge && !terminalSubmissionStarted) {
        await submitFailure(challenge, error).catch(() => {});
      }
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
