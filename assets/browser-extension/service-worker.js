importScripts("config.js", "transport-loopback.js");

const bridgeConfig = globalThis.SUNOX_BRIDGE_CONFIG;
const transport = globalThis.SUNOX_BRIDGE_TRANSPORTS?.[bridgeConfig?.transport];
if (
  bridgeConfig?.schemaVersion !== 1
  || transport?.contractVersion !== 1
  || typeof transport.claimChallenge !== "function"
  || typeof transport.submitResult !== "function"
) {
  throw new Error("Unsupported Sunox Browser Bridge configuration");
}

const POLL_ALARM = "sunox-bridge-poll";
let scanInProgress = false;
let nextScanAt = 0;
let scanDelayMs = 500;

async function claimChallenge(message) {
  if (scanInProgress || Date.now() < nextScanAt) return null;
  scanInProgress = true;
  try {
    const challenge = await transport.claimChallenge(message);
    scanDelayMs = challenge ? 500 : Math.min(Math.ceil(scanDelayMs * 1.6), 5000);
    nextScanAt = Date.now() + scanDelayMs;
    return challenge;
  } finally {
    scanInProgress = false;
  }
}

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message?.type === "sunox-claim") {
    claimChallenge(message).then(sendResponse);
    return true;
  }
  if (message?.type === "sunox-result") {
    transport.submitResult(message).then(sendResponse);
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
