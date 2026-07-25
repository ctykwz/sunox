importScripts("shared.js");

const POLL_ALARM = "sunox-bridge-poll";
const IDLE_ALARM = "sunox-bridge-managed-context-idle";
const OFFSCREEN_PATH = "offscreen.html";
const MANAGED_PAGE_URL = "https://suno.com/create#sunox-browser-bridge";
const MANAGED_CONTEXT_KEY = "managedContext";
const MANAGED_CONTEXT_IDLE_MS = 20 * 60 * 1000;
const CONTENT_SCRIPT_TIMEOUT_MS = 20_000;
const CONTENT_SCRIPT_RETRY_MS = 250;
let creatingOffscreenDocument;
let solveInProgress;
let bootstrapPromise;
let contextOperation = Promise.resolve();
const { errorMessage } = globalThis.SUNOX_BRIDGE_SHARED;

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

function withContextLock(operation) {
  const result = contextOperation.then(operation, operation);
  contextOperation = result.catch(() => {});
  return result;
}

async function ensureOffscreenDocument() {
  const documentUrl = chrome.runtime.getURL(OFFSCREEN_PATH);
  const contexts = await chrome.runtime.getContexts({
    contextTypes: ["OFFSCREEN_DOCUMENT"],
    documentUrls: [documentUrl]
  });
  if (contexts.length > 0) return;

  if (!creatingOffscreenDocument) {
    creatingOffscreenDocument = chrome.offscreen.createDocument({
      url: OFFSCREEN_PATH,
      reasons: ["WORKERS"],
      justification: "Run the local Sunox challenge listener without opening a browser tab."
    }).finally(() => {
      creatingOffscreenDocument = null;
    });
  }
  await creatingOffscreenDocument;
}

async function readManagedContext() {
  const stored = (await chrome.storage.session.get(MANAGED_CONTEXT_KEY))[MANAGED_CONTEXT_KEY];
  if (
    !stored
    || !Number.isInteger(stored.windowId)
    || !Number.isInteger(stored.tabId)
    || !Number.isFinite(stored.lastUsedAt)
    || !Number.isInteger(stored.failureCount)
    || stored.failureCount < 0
  ) return null;

  try {
    const [window, tab] = await Promise.all([
      chrome.windows.get(stored.windowId),
      chrome.tabs.get(stored.tabId)
    ]);
    if (window.type !== "popup" || tab.windowId !== stored.windowId) {
      throw new Error("Managed context no longer matches its saved window");
    }
    return stored;
  } catch {
    await chrome.storage.session.remove(MANAGED_CONTEXT_KEY);
    return null;
  }
}

async function createManagedContext() {
  const window = await chrome.windows.create({
    url: MANAGED_PAGE_URL,
    type: "popup",
    focused: false,
    left: -32_000,
    top: -32_000,
    width: 1280,
    height: 900
  });
  const tabs = Number.isInteger(window?.id)
    ? await chrome.tabs.query({ windowId: window.id })
    : [];
  const tabId = window?.tabs?.[0]?.id ?? tabs[0]?.id;
  if (!Number.isInteger(window?.id) || !Number.isInteger(tabId)) {
    if (Number.isInteger(window?.id)) {
      await chrome.windows.remove(window.id).catch(() => {});
    }
    throw new Error("Chrome did not create the managed Browser Bridge context");
  }

  const context = {
    windowId: window.id,
    tabId,
    lastUsedAt: Date.now(),
    failureCount: 0
  };
  await chrome.storage.session.set({ [MANAGED_CONTEXT_KEY]: context });
  return context;
}

async function managedContext() {
  return await readManagedContext() || await createManagedContext();
}

async function touchManagedContext(context, failureCount) {
  const updated = { ...context, lastUsedAt: Date.now(), failureCount };
  await chrome.storage.session.set({ [MANAGED_CONTEXT_KEY]: updated });
  await chrome.alarms.create(IDLE_ALARM, {
    delayInMinutes: MANAGED_CONTEXT_IDLE_MS / 60_000
  });
}

async function closeManagedContext(context) {
  await chrome.storage.session.remove(MANAGED_CONTEXT_KEY);
  await chrome.windows.remove(context.windowId).catch(() => {});
}

async function executeInManagedContext(challenge) {
  const context = await managedContext();
  await touchManagedContext(context, context.failureCount);
  const deadline = Date.now() + CONTENT_SCRIPT_TIMEOUT_MS;
  try {
    while (Date.now() < deadline) {
      try {
        const result = await chrome.tabs.sendMessage(context.tabId, {
          type: "sunox-execute",
          challenge
        });
        if (typeof result?.token === "string" && result.token) {
          await touchManagedContext(context, 0);
          return { token: result.token, error: null };
        }
        if (typeof result?.error === "string" && result.error) {
          const failureCount = context.failureCount + 1;
          if (failureCount >= 3) {
            await closeManagedContext(context);
          } else {
            await touchManagedContext(context, failureCount);
          }
          return { token: null, error: result.error.slice(0, 900) };
        }
      } catch {
        // The managed Suno page may still be loading its content script.
      }
      await delay(CONTENT_SCRIPT_RETRY_MS);
    }
    await closeManagedContext(context);
    return {
      token: null,
      error: "Managed Suno page did not become ready within 20 seconds"
    };
  } catch (error) {
    await closeManagedContext(context);
    return { token: null, error: errorMessage(error) };
  }
}

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message?.type === "sunox-managed-challenge") {
    if (solveInProgress) {
      sendResponse({
        token: null,
        error: "Browser Bridge is already solving another challenge"
      });
      return false;
    }
    solveInProgress = ensureBootstrapped()
      .then(() => withContextLock(() => executeInManagedContext(message.challenge)))
      .catch((error) => ({ token: null, error: errorMessage(error) }))
      .finally(() => {
        solveInProgress = null;
      });
    solveInProgress.then(sendResponse);
    return true;
  }
  return false;
});

async function ensurePollAlarm() {
  if (await chrome.alarms.get(POLL_ALARM)) return;
  await chrome.alarms.create(POLL_ALARM, {
    delayInMinutes: 0.5,
    periodInMinutes: 0.5
  });
}

async function closeIdleManagedContext() {
  if (solveInProgress) {
    await chrome.alarms.create(IDLE_ALARM, {
      delayInMinutes: MANAGED_CONTEXT_IDLE_MS / 60_000
    });
    return;
  }
  const context = await readManagedContext();
  if (!context) return;
  if (solveInProgress) {
    await chrome.alarms.create(IDLE_ALARM, {
      delayInMinutes: MANAGED_CONTEXT_IDLE_MS / 60_000
    });
    return;
  }
  const idleFor = Date.now() - context.lastUsedAt;
  if (idleFor >= MANAGED_CONTEXT_IDLE_MS) {
    await closeManagedContext(context);
    return;
  }
  await chrome.alarms.create(IDLE_ALARM, {
    delayInMinutes: (MANAGED_CONTEXT_IDLE_MS - idleFor) / 60_000
  });
}

async function closeOrphanedManagedContexts() {
  if (await readManagedContext()) return;
  const tabs = await chrome.tabs.query({ url: "https://suno.com/*" });
  const windowIds = new Set(
    tabs
      .filter((tab) => tab.url === MANAGED_PAGE_URL && Number.isInteger(tab.windowId))
      .map((tab) => tab.windowId)
  );
  await Promise.allSettled(
    Array.from(windowIds, (windowId) => chrome.windows.remove(windowId))
  );
}

chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === POLL_ALARM) ensureOffscreenDocument().catch(() => {});
  if (alarm.name === IDLE_ALARM) {
    withContextLock(closeIdleManagedContext).catch(() => {});
  }
});
chrome.windows.onRemoved.addListener(async (windowId) => {
  const context = await readManagedContext();
  if (context?.windowId === windowId) {
    await chrome.storage.session.remove(MANAGED_CONTEXT_KEY);
  }
});

async function bootstrap() {
  await ensurePollAlarm();
  await closeOrphanedManagedContexts();
  await ensureOffscreenDocument();
}

function ensureBootstrapped() {
  if (!bootstrapPromise) {
    bootstrapPromise = bootstrap().catch((error) => {
      bootstrapPromise = null;
      throw error;
    });
  }
  return bootstrapPromise;
}

chrome.runtime.onInstalled.addListener(() => ensureBootstrapped().catch(() => {}));
chrome.runtime.onStartup.addListener(() => ensureBootstrapped().catch(() => {}));
ensureBootstrapped().catch(() => {});
