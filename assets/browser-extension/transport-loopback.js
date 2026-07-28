(() => {
  const settings = globalThis.SUNOX_BRIDGE_CONFIG?.loopback;
  if (!settings) {
    throw new Error("Missing Sunox loopback transport configuration");
  }
  if (
    !Number.isInteger(settings.protocolVersion)
    || typeof settings.runtimeBuild !== "string"
    || !/^\d+\.\d+\.\d+$/.test(settings.runtimeBuild)
  ) {
    throw new Error("Invalid Sunox loopback runtime identity");
  }

  const ports = Array.from(
    { length: settings.portCount },
    (_, index) => settings.portStart + index
  );
  const requestTimeoutMs = 350;
  const maxJsonResponseBytes = 4 * 1024;
  const textEncoder = new TextEncoder();
  const textDecoder = new TextDecoder();
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

  function encodeBase64Url(bytes) {
    let binary = "";
    for (const byte of bytes) binary += String.fromCharCode(byte);
    return btoa(binary).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "");
  }

  function decodeBase64Url(value) {
    const padding = "=".repeat((4 - value.length % 4) % 4);
    const binary = atob(value.replaceAll("-", "+").replaceAll("_", "/") + padding);
    return Uint8Array.from(binary, (character) => character.charCodeAt(0));
  }

  async function createReceipt(bridge) {
    const payload = encodeBase64Url(textEncoder.encode(JSON.stringify([
      bridge.port,
      bridge.clientNonce,
      bridge.serverNonce
    ])));
    const proof = await sign("sunox-bridge-receipt-v3", [payload]);
    return `${payload}.${proof}`;
  }

  async function openReceipt(receipt) {
    if (!/^[A-Za-z0-9_-]+\.[0-9a-f]{64}$/.test(receipt || "")) return null;
    const [payload, proof] = receipt.split(".");
    if (!await verify(proof, "sunox-bridge-receipt-v3", [payload])) return null;
    try {
      const [port, clientNonce, serverNonce] = JSON.parse(
        textDecoder.decode(decodeBase64Url(payload))
      );
      if (
        !ports.includes(port)
        || typeof clientNonce !== "string"
        || typeof serverNonce !== "string"
      ) return null;
      return { port, clientNonce, serverNonce };
    } catch {
      return null;
    }
  }

  async function readBoundedJson(response) {
    const contentType = response.headers.get("content-type");
    if (
      typeof contentType !== "string"
      || contentType.split(";", 1)[0].trim().toLowerCase() !== "application/json"
    ) {
      throw new Error("Sunox loopback response was not JSON");
    }
    const declaredLength = response.headers.get("content-length");
    if (declaredLength !== null) {
      const normalizedLength = declaredLength.trim();
      if (
        !/^(?:0|[1-9]\d*)$/.test(normalizedLength)
        || Number(normalizedLength) > maxJsonResponseBytes
      ) {
        throw new Error("Sunox loopback JSON response exceeded its size limit");
      }
    }

    const reader = response.body?.getReader();
    if (!reader) {
      throw new Error("Sunox loopback JSON response had no readable body");
    }
    const chunks = [];
    let total = 0;
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      if (!(value instanceof Uint8Array)) {
        throw new Error("Sunox loopback JSON response returned an invalid body chunk");
      }
      total += value.byteLength;
      if (total > maxJsonResponseBytes) {
        reader.cancel().catch(() => {});
        throw new Error("Sunox loopback JSON response exceeded its size limit");
      }
      chunks.push(value);
    }
    const bytes = new Uint8Array(total);
    let offset = 0;
    for (const chunk of chunks) {
      bytes.set(chunk, offset);
      offset += chunk.byteLength;
    }
    return JSON.parse(textDecoder.decode(bytes));
  }

  async function bridgeRequest(port, path, body, expectJson = false) {
    const controller = new AbortController();
    let timeout;
    const deadline = new Promise((_, reject) => {
      timeout = setTimeout(() => {
        controller.abort();
        reject(new Error("Sunox loopback request timed out"));
      }, requestTimeoutMs);
    });
    const request = (async () => {
      const response = await fetch(`http://127.0.0.1:${port}${path}`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "X-Sunox-Extension": "1"
        },
        body: JSON.stringify(body),
        redirect: "error",
        signal: controller.signal
      });
      return {
        ok: response.ok,
        json: response.ok && expectJson
          ? await readBoundedJson(response)
          : null
      };
    })();
    try {
      return await Promise.race([request, deadline]);
    } finally {
      clearTimeout(timeout);
      controller.abort();
    }
  }

  async function authenticateBridge(port) {
    const clientNonce = crypto.randomUUID();
    try {
      const response = await bridgeRequest(
        port,
        "/v3/challenge/hello",
        {
          version: settings.protocolVersion,
          client_nonce: clientNonce
        },
        true
      );
      if (!response.ok) return null;
      const hello = response.json;
      const valid = hello.version === settings.protocolVersion
        && typeof hello.server_nonce === "string"
        && await verify(
          hello.proof,
          "sunox-bridge-server-v3",
          [port, clientNonce, hello.server_nonce]
        );
      return valid ? { port, clientNonce, serverNonce: hello.server_nonce } : null;
    } catch {
      return null;
    }
  }

  async function acknowledgeProbe(bridge, requestId) {
    const fields = [
      bridge.port,
      bridge.clientNonce,
      bridge.serverNonce,
      requestId,
      settings.runtimeBuild
    ];
    const response = await bridgeRequest(
      bridge.port,
      "/v3/challenge/probe-ack",
      {
        version: settings.protocolVersion,
        runtime_build: settings.runtimeBuild,
        request_id: requestId,
        client_nonce: bridge.clientNonce,
        server_nonce: bridge.serverNonce,
        proof: await sign("sunox-bridge-probe-ack-v3", fields)
      }
    );
    return response.ok;
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
        settings.runtimeBuild,
        message.clientId,
        message.pageUrl
      ];
      try {
        const response = await bridgeRequest(
          bridge.port,
          "/v3/challenge/claim",
          {
            version: settings.protocolVersion,
            runtime_build: settings.runtimeBuild,
            client_id: message.clientId,
            page_url: message.pageUrl,
            client_nonce: bridge.clientNonce,
            server_nonce: bridge.serverNonce,
            proof: await sign("sunox-bridge-client-v3", fields)
          },
          true
        );
        if (!response.ok) continue;
        const challenge = response.json;
        if (
          challenge.version === settings.protocolVersion
          && challenge.probe === true
          && typeof challenge.request_id === "string"
        ) {
          await acknowledgeProbe(bridge, challenge.request_id);
          continue;
        }
        if (
          challenge.version !== settings.protocolVersion
          || typeof challenge.request_id !== "string"
          || !["hcaptcha", "turnstile"].includes(challenge.provider)
        ) continue;
        return {
          requestId: challenge.request_id,
          provider: challenge.provider,
          transportReceipt: await createReceipt(bridge)
        };
      } catch {
        // Try another authenticated Sunox listener.
      }
    }
    return null;
  }

  async function submitResult(message) {
    const bridge = await openReceipt(message.transportReceipt);
    if (!bridge) return { accepted: false };
    const kind = message.token ? "token" : "error";
    const value = message.token || message.error || "Challenge returned no result";
    const fields = [
      bridge.port,
      bridge.clientNonce,
      bridge.serverNonce,
      message.requestId,
      kind,
      value
    ];
    try {
      const response = await bridgeRequest(bridge.port, "/v3/challenge/result", {
        version: settings.protocolVersion,
        request_id: message.requestId,
        client_nonce: bridge.clientNonce,
        server_nonce: bridge.serverNonce,
        token: kind === "token" ? value : null,
        error: kind === "error" ? value : null,
        proof: await sign("sunox-bridge-result-v3", fields)
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
