postMessage({ type: "sunox-poll" });
setInterval(() => postMessage({ type: "sunox-poll" }), 500);
