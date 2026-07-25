(async () => {
  let container;
  let widgetId;
  try {
    container = document.createElement("div");
    container.id = "sunox-generation-turnstile";
    document.body.appendChild(container);
    return await new Promise((resolve) => {
      let settled = false;
      let interactive = false;
      let timeout;
      const finish = (value) => {
        if (settled) return;
        settled = true;
        clearTimeout(timeout);
        resolve(value || "");
      };
      const scheduleDeadline = (timeoutMs, message) => {
        clearTimeout(timeout);
        timeout = setTimeout(() => finish(`ERR:${message}`), timeoutMs);
      };
      scheduleDeadline(
        15000,
        "Turnstile produced no callback within 15 seconds"
      );
      widgetId = turnstile.render(container, {
        sitekey: "0x4AAAAAADI7xDNyj-3LcIbi",
        execution: "execute",
        appearance: "interaction-only",
        callback: finish,
        "error-callback": (code) => scheduleDeadline(
          interactive ? 120000 : 15000,
          `Turnstile did not recover after error ${code || "unknown"}`
        ),
        "expired-callback": () => finish("ERR:Turnstile token expired"),
        "timeout-callback": () => {},
        "unsupported-callback": () => finish("ERR:Turnstile unsupported in this browser"),
        "before-interactive-callback": () => {
          interactive = true;
          scheduleDeadline(
            120000,
            "Turnstile interactive challenge was abandoned"
          );
        },
        "after-interactive-callback": () => {
          interactive = false;
          scheduleDeadline(
            15000,
            "Turnstile produced no token after interactive challenge completed"
          );
        }
      });
      turnstile.execute(widgetId);
    });
  } catch (error) {
    return `ERR:${String(error)}`;
  } finally {
    if (widgetId !== undefined) {
      try {
        turnstile.remove(widgetId);
      } catch {}
    }
    container?.remove();
  }
})()
