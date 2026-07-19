globalThis.SUNOX_BRIDGE_CONFIG = Object.freeze({
  schemaVersion: 1,
  transport: "loopback",
  loopback: Object.freeze({
    protocolVersion: 1,
    portStart: 29764,
    portCount: 8,
    sharedSecret: "__SUNOX_BRIDGE_SECRET__"
  })
});
