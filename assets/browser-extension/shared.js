globalThis.SUNOX_BRIDGE_SHARED = Object.freeze({
  errorMessage(error) {
    const value = error instanceof Error ? error.message : String(error);
    return value.slice(0, 900) || "Browser Bridge failed without an error message";
  }
});
