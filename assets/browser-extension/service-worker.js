importScripts("config.js");

const PORTS = Array.from({ length: 8 }, (_, index) => 29764 + index);
const PROTOCOL_VERSION = 1;
const REQUEST_TIMEOUT_MS = 350;
const POLL_ALARM = "sunox-bridge-poll";
const textEncoder = new TextEncoder();
let signingKeyPromise;
let scanInProgress = false;
let nextScanAt = 0;
let scanDelayMs = 500;

function authenticationPayload(label, fields) {
  const encodedLabel = textEncoder.encode(label);
  const encodedFields = fields.map((field) => textEncoder.encode(String(field)));
  const size = encodedLabel.length + 1 + encodedFields.reduce((total, field) => total + 4 + field.length, 0);
  const payload = new Uint8Array(size);
  let offset = 0;
  payload.set(encodedLabel, offset);
  offset += encodedLabel.length;
  payload[offset++] = 0;
  const view = new DataView(payload.buffer);
  for (const field of encodedFields) {
    view.setUint32(offset, field.length, false);
    offset += 4;
    payload.set(field, offset);
    offset += field.length;
  }
  return payload;
}

function signingKey() {
  signingKeyPromise ||= crypto.subtle.importKey(
    "raw",
    textEncoder.encode(globalThis.SUNOX_BRIDGE_SECRET),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign", "verify"]
  );
  return signingKeyPromise;
}

async function sign(label, fields) {
  const signature = await crypto.subtle.sign(
    "HMAC",
    await signingKey(),
    authenticationPayload(label, fields)
  );
  return Array.from(new Uint8Array(signature), (byte) => byte.toString(16).padStart(2, "0")).join("");
}

async function verify(proof, label, fields) {
  if (!/^[0-9a-f]{64}$/.test(proof || "")) return false;
  const bytes = Uint8Array.from(proof.match(/../g), (pair) => Number.parseInt(pair, 16));
  return crypto.subtle.verify(
    "HMAC",
    await signingKey(),
    bytes,
    authenticationPayload(label, fields)
  );
}

async function bridgeRequest(port, path, body) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS);
  try {
    return await fetch(`http://127.0.0.1:${port}${path}`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "X-Sunox-Extension": "1"
      },
      body: JSON.stringify(body),
      signal: controller.signal
    });
  } finally {
    clearTimeout(timeout);
  }
}

async function authenticateBridge(port) {
  const clientNonce = crypto.randomUUID();
  try {
    const response = await bridgeRequest(port, "/v1/challenge/hello", {
      version: PROTOCOL_VERSION,
      client_nonce: clientNonce
    });
    if (!response.ok) return null;
    const hello = await response.json();
    const valid = hello.version === PROTOCOL_VERSION
      && typeof hello.server_nonce === "string"
      && await verify(
        hello.proof,
        "sunox-bridge-server-v1",
        [port, clientNonce, hello.server_nonce]
      );
    return valid ? { port, clientNonce, serverNonce: hello.server_nonce } : null;
  } catch {
    return null;
  }
}

async function scanAndClaim(message) {
  const authenticated = (await Promise.all(PORTS.map(authenticateBridge)))
    .filter(Boolean)
    .sort((left, right) => left.port - right.port);

  for (const bridge of authenticated) {
    const fields = [
      bridge.port,
      bridge.clientNonce,
      bridge.serverNonce,
      message.clientId,
      message.pageUrl
    ];
    try {
      const response = await bridgeRequest(bridge.port, "/v1/challenge/claim", {
        version: PROTOCOL_VERSION,
        client_id: message.clientId,
        page_url: message.pageUrl,
        client_nonce: bridge.clientNonce,
        server_nonce: bridge.serverNonce,
        proof: await sign("sunox-bridge-client-v1", fields)
      });
      if (!response.ok) continue;
      const challenge = await response.json();
      return {
        ...challenge,
        bridgePort: bridge.port,
        clientNonce: bridge.clientNonce,
        serverNonce: bridge.serverNonce
      };
    } catch {
      // Try another authenticated Sunox listener.
    }
  }
  return null;
}

async function claimChallenge(message) {
  if (scanInProgress || Date.now() < nextScanAt) return null;
  scanInProgress = true;
  try {
    const challenge = await scanAndClaim(message);
    scanDelayMs = challenge ? 500 : Math.min(Math.ceil(scanDelayMs * 1.6), 5000);
    nextScanAt = Date.now() + scanDelayMs;
    return challenge;
  } finally {
    scanInProgress = false;
  }
}

async function submitResult(message) {
  const kind = message.token ? "token" : "error";
  const value = message.token || message.error || "Challenge returned no result";
  const fields = [
    message.bridgePort,
    message.clientNonce,
    message.serverNonce,
    message.requestId,
    kind,
    value
  ];
  try {
    const response = await bridgeRequest(message.bridgePort, "/v1/challenge/result", {
      version: PROTOCOL_VERSION,
      request_id: message.requestId,
      client_nonce: message.clientNonce,
      server_nonce: message.serverNonce,
      token: kind === "token" ? value : null,
      error: kind === "error" ? value : null,
      proof: await sign("sunox-bridge-result-v1", fields)
    });
    return { accepted: response.ok };
  } catch {
    return { accepted: false };
  }
}

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message?.type === "sunox-claim") {
    claimChallenge(message).then(sendResponse);
    return true;
  }
  if (message?.type === "sunox-result") {
    submitResult(message).then(sendResponse);
    return true;
  }
  return false;
});

async function ensurePollAlarm() {
  if (await chrome.alarms.get(POLL_ALARM)) return;
  await chrome.alarms.create(POLL_ALARM, {
    delayInMinutes: 0.1,
    periodInMinutes: 0.1
  });
}

async function wakeSunoTabs() {
  const tabs = await chrome.tabs.query({ url: "https://suno.com/*" });
  await Promise.allSettled(
    tabs
      .filter((tab) => Number.isInteger(tab.id))
      .map((tab) => chrome.tabs.sendMessage(tab.id, { type: "sunox-wake" }))
  );
}

chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === POLL_ALARM) wakeSunoTabs().catch(() => {});
});
chrome.runtime.onInstalled.addListener(() => ensurePollAlarm().catch(() => {}));
chrome.runtime.onStartup.addListener(() => ensurePollAlarm().catch(() => {}));
ensurePollAlarm().catch(() => {});
