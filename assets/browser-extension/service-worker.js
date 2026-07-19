importScripts("config.js");

const PORTS = Array.from({ length: 8 }, (_, index) => 29764 + index);
const PROTOCOL_VERSION = 1;

async function bridgeRequest(port, path, body) {
  const response = await fetch(`http://127.0.0.1:${port}${path}`, {
    method: "POST",
    headers: {
      "Authorization": `Bearer ${globalThis.SUNOX_BRIDGE_SECRET}`,
      "Content-Type": "application/json",
      "X-Sunox-Extension": "1"
    },
    body: JSON.stringify(body)
  });
  return response;
}

async function claimChallenge(message) {
  for (const port of PORTS) {
    try {
      const response = await bridgeRequest(port, "/v1/challenge/claim", {
        version: PROTOCOL_VERSION,
        client_id: message.clientId,
        page_url: message.pageUrl
      });
      if (response.status === 204 || response.status === 409) {
        return null;
      }
      if (response.status === 401 || response.status === 403) {
        return { bridgeError: "The installed Sunox extension is out of sync; reinstall it with --force." };
      }
      if (!response.ok) {
        continue;
      }
      const challenge = await response.json();
      return { ...challenge, bridgePort: port };
    } catch {
      // No Sunox listener on this port.
    }
  }
  return null;
}

async function submitResult(message) {
  try {
    const response = await bridgeRequest(message.bridgePort, "/v1/challenge/result", {
      version: PROTOCOL_VERSION,
      request_id: message.requestId,
      token: message.token || null,
      error: message.error || null
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
