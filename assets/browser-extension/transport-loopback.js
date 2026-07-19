(() => {
  const settings = globalThis.SUNOX_BRIDGE_CONFIG?.loopback;
  if (!settings) {
    throw new Error("Missing Sunox loopback transport configuration");
  }

  const ports = Array.from(
    { length: settings.portCount },
    (_, index) => settings.portStart + index
  );
  const requestTimeoutMs = 350;
  const textEncoder = new TextEncoder();
  let signingKeyPromise;

  function authenticationPayload(label, fields) {
    const encodedLabel = textEncoder.encode(label);
    const encodedFields = fields.map((field) => textEncoder.encode(String(field)));
    const size = encodedLabel.length + 1
      + encodedFields.reduce((total, field) => total + 4 + field.length, 0);
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
      textEncoder.encode(settings.sharedSecret),
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
    return Array.from(
      new Uint8Array(signature),
      (byte) => byte.toString(16).padStart(2, "0")
    ).join("");
  }

  async function verify(proof, label, fields) {
    if (!/^[0-9a-f]{64}$/.test(proof || "")) return false;
    const bytes = Uint8Array.from(
      proof.match(/../g),
      (pair) => Number.parseInt(pair, 16)
    );
    return crypto.subtle.verify(
      "HMAC",
      await signingKey(),
      bytes,
      authenticationPayload(label, fields)
    );
  }

  async function bridgeRequest(port, path, body) {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), requestTimeoutMs);
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
        version: settings.protocolVersion,
        client_nonce: clientNonce
      });
      if (!response.ok) return null;
      const hello = await response.json();
      const valid = hello.version === settings.protocolVersion
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

  async function claimChallenge(message) {
    const authenticated = (await Promise.all(ports.map(authenticateBridge)))
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
          version: settings.protocolVersion,
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
        version: settings.protocolVersion,
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

  globalThis.SUNOX_BRIDGE_TRANSPORTS ||= Object.create(null);
  globalThis.SUNOX_BRIDGE_TRANSPORTS.loopback = Object.freeze({
    contractVersion: 1,
    claimChallenge,
    submitResult
  });
})();
