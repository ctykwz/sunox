(() => {
  const retryableManagedNetworkReasons = new Set([
    "managed_network_connection",
    "managed_network_name_resolution",
    "managed_network_protocol",
    "managed_network_timeout"
  ]);
  globalThis.SUNOX_BRIDGE_SHARED = Object.freeze({
    errorMessage(error) {
      const value = error instanceof Error ? error.message : String(error);
      return value.slice(0, 900) || "Browser Bridge failed without an error message";
    },
    isRetryableManagedNetworkReason(reason) {
      return typeof reason === "string" && retryableManagedNetworkReasons.has(reason);
    }
  });
})();
