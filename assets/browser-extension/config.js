globalThis.SUNOX_BRIDGE_CONFIG = Object.freeze({
  schemaVersion: 1,
  transport: "loopback",
  loopback: Object.freeze({
    protocolVersion: __SUNOX_BRIDGE_PROTOCOL_VERSION__,
    portStart: __SUNOX_BRIDGE_PORT_START__,
    portCount: __SUNOX_BRIDGE_PORT_COUNT__,
    sharedSecret: "__SUNOX_BRIDGE_SECRET__"
  })
});
