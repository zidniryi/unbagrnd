// This project has no bundler (see README / tauri.conf.json `app.withGlobalTauri`),
// so the Tauri JS APIs are read off the `window.__TAURI__` global injected at
// runtime rather than imported as ES modules (bare specifiers like
// "@tauri-apps/api/core" don't resolve without a bundler).
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getCurrentWebview } = window.__TAURI__.webview;
const { ask, open } = window.__TAURI__.dialog;
const { revealItemInDir } = window.__TAURI__.opener;

const IMAGE_FILTERS = [
  { name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "bmp", "tiff", "tif", "gif"] },
];

// Models at or above this size get a confirmation dialog before downloading.
const LARGE_MODEL_THRESHOLD = 100 * 1024 * 1024;

const $ = (id) => document.getElementById(id);

// ---- Shared elements ----
const statusLine = $("status-line");
const resourceUsageEl = $("resource-usage");
const outputDirDisplay = $("output-dir-display");
const clearOutputDirBtn = $("clear-output-dir-btn");

// ---- Model bar ----
const modelSelect = $("model-select");
const modelBadge = $("model-badge");
const modelDownloadBtn = $("model-download-btn");
const modelDownloadProgress = $("model-download-progress");
const modelProgressFill = $("model-progress-fill");
const modelProgressLabel = $("model-progress-label");
const modelDesc = $("model-desc");

// ---- Settings modal ----
const settingsBtn = $("settings-btn");
const settingsOverlay = $("settings-overlay");
const settingsCloseBtn = $("settings-close-btn");
const themeSelect = $("theme-select");
const settingsModelList = $("settings-model-list");
const clearAllModelsBtn = $("clear-all-models-btn");

// ---- Tabs ----
const tabSingle = $("tab-single");
const tabBatch = $("tab-batch");
const panelSingle = $("panel-single");
const panelBatch = $("panel-batch");

// ---- Single mode ----
const singleDropzone = $("single-dropzone");
const singlePickBtn = $("single-pick-btn");
const singleLoading = $("single-loading");
const singleLoadingText = $("single-loading-text");
const singleProgressFill = $("single-progress-fill");
const singleProgressLabel = $("single-progress-label");
const singleResult = $("single-result");
const previewBefore = $("preview-before");
const previewAfter = $("preview-after");
const singleOutputPathEl = $("single-output-path");
const singleRevealBtn = $("single-reveal-btn");
const singleResetBtn = $("single-reset-btn");

// ---- Batch mode ----
const batchDropzone = $("batch-dropzone");
const batchPickFilesBtn = $("batch-pick-files-btn");
const batchPickFolderBtn = $("batch-pick-folder-btn");
const batchResult = $("batch-result");
const batchProgressFill = $("batch-progress-fill");
const batchProgressLabel = $("batch-progress-label");
const batchFileList = $("batch-file-list");
const batchRevealBtn = $("batch-reveal-btn");
const batchResetBtn = $("batch-reset-btn");
const chooseOutputDirBtn = $("choose-output-dir-btn");

// ---- App state ----
let outputDir = null; // null = default: same folder as each source image
let models = []; // ModelInfo[] from the backend
let selectedModelKey = null;
const activeProgress = {}; // key -> { downloaded, total } for in-flight downloads
const downloadPromises = new Map(); // key -> in-flight download promise (dedupe)
let lastSingleOutputPath = null;
let lastBatchOutputDir = null;
let busy = false;

function setStatus(text) {
  statusLine.textContent = text;
}

function setBusy(isBusy) {
  busy = isBusy;
  for (const btn of document.querySelectorAll("button")) {
    btn.disabled = isBusy;
  }
}

function formatBytes(bytes) {
  if (!Number.isFinite(bytes)) return "";
  const mb = bytes / (1024 * 1024);
  return `${mb.toFixed(1)} MB`;
}

function findModel(key) {
  return models.find((m) => m.key === key);
}

// ---------------------------------------------------------------------
// Models: list, select, download, clear
// ---------------------------------------------------------------------

async function refreshModelsList() {
  models = await invoke("list_models");
  populateModelSelect();
  renderModelBar();
  if (!settingsOverlay.hidden) renderSettingsModelList();
}

function populateModelSelect() {
  modelSelect.innerHTML = "";
  for (const m of models) {
    const opt = document.createElement("option");
    opt.value = m.key;
    opt.textContent = m.downloaded ? m.displayName : `${m.displayName} (${formatBytes(m.sizeBytes)})`;
    modelSelect.appendChild(opt);
  }
  if (selectedModelKey) modelSelect.value = selectedModelKey;
}

function renderModelBar() {
  const info = findModel(selectedModelKey);
  if (!info) return;
  modelDesc.textContent = info.description;

  const progress = activeProgress[selectedModelKey];
  if (progress) {
    modelDownloadBtn.hidden = true;
    modelDownloadProgress.hidden = false;
    const pct = progress.total > 0 ? Math.min(100, (progress.downloaded / progress.total) * 100) : 0;
    modelProgressFill.style.width = `${pct}%`;
    modelProgressLabel.textContent = `${formatBytes(progress.downloaded)} / ${formatBytes(progress.total)}`;
    modelBadge.textContent = "Downloading…";
    modelBadge.className = "model-badge badge-warn";
    return;
  }

  modelDownloadProgress.hidden = true;
  if (info.downloaded) {
    modelDownloadBtn.hidden = true;
    modelBadge.textContent = "Ready";
    modelBadge.className = "model-badge badge-ready";
  } else {
    modelDownloadBtn.hidden = false;
    modelBadge.textContent = `Not downloaded (${formatBytes(info.sizeBytes)})`;
    modelBadge.className = "model-badge badge-warn";
  }
}

function renderSettingsModelList() {
  settingsModelList.innerHTML = "";
  for (const m of models) {
    const li = document.createElement("li");
    li.className = "settings-model-row";
    const isSelected = m.key === selectedModelKey;
    const progress = activeProgress[m.key];

    const info = document.createElement("div");
    info.className = "settings-model-info";

    const nameEl = document.createElement("div");
    nameEl.className = "settings-model-name";
    nameEl.textContent = m.displayName;
    if (isSelected) {
      const badge = document.createElement("span");
      badge.className = "model-current-badge";
      badge.textContent = "Current";
      nameEl.appendChild(badge);
    }
    info.appendChild(nameEl);

    const descEl = document.createElement("div");
    descEl.className = "settings-model-desc";
    descEl.textContent = m.description;
    info.appendChild(descEl);

    const metaEl = document.createElement("div");
    metaEl.className = "settings-model-meta";
    metaEl.textContent = `${formatBytes(m.sizeBytes)}${m.downloaded ? " · Downloaded" : ""}`;
    info.appendChild(metaEl);

    li.appendChild(info);

    const actions = document.createElement("div");
    actions.className = "settings-model-actions";

    if (progress) {
      const pct = progress.total > 0 ? Math.min(100, (progress.downloaded / progress.total) * 100) : 0;
      const bar = document.createElement("div");
      bar.className = "settings-model-progress";
      const track = document.createElement("div");
      track.className = "progress-track";
      const fill = document.createElement("div");
      fill.className = "progress-fill";
      fill.style.width = `${pct}%`;
      track.appendChild(fill);
      const label = document.createElement("span");
      label.textContent = `${formatBytes(progress.downloaded)} / ${formatBytes(progress.total)}`;
      bar.append(track, label);
      actions.appendChild(bar);
    } else {
      if (!isSelected) {
        const useBtn = document.createElement("button");
        useBtn.className = "btn btn-secondary btn-sm";
        useBtn.type = "button";
        useBtn.textContent = "Use";
        useBtn.addEventListener("click", () => selectModel(m.key));
        actions.appendChild(useBtn);
      }
      if (m.downloaded) {
        const clearBtn = document.createElement("button");
        clearBtn.className = "btn btn-link";
        clearBtn.type = "button";
        clearBtn.textContent = "Clear";
        if (isSelected) {
          clearBtn.disabled = true;
          clearBtn.title = "Can't clear the model currently in use";
        }
        clearBtn.addEventListener("click", () => clearOneModel(m.key));
        actions.appendChild(clearBtn);
      } else {
        const dlBtn = document.createElement("button");
        dlBtn.className = "btn btn-primary btn-sm";
        dlBtn.type = "button";
        dlBtn.textContent = "Download";
        dlBtn.addEventListener("click", () => downloadOneModel(m.key));
        actions.appendChild(dlBtn);
      }
    }

    li.appendChild(actions);
    settingsModelList.appendChild(li);
  }
}

/** Downloads `key`, deduping concurrent callers onto the same in-flight request. */
function downloadModelWithProgress(key) {
  if (downloadPromises.has(key)) return downloadPromises.get(key);
  const promise = (async () => {
    activeProgress[key] = { downloaded: 0, total: 0 };
    renderModelBar();
    renderSettingsModelList();
    try {
      const info = await invoke("download_model", { key });
      const idx = models.findIndex((m) => m.key === key);
      if (idx >= 0) models[idx] = info;
      return info;
    } finally {
      delete activeProgress[key];
      downloadPromises.delete(key);
      renderModelBar();
      renderSettingsModelList();
    }
  })();
  downloadPromises.set(key, promise);
  return promise;
}

/** Ensures `key` is downloaded, asking for confirmation first if it's large. */
async function ensureModelReady(key) {
  const info = findModel(key);
  if (info?.downloaded) return info;
  if (info && info.sizeBytes >= LARGE_MODEL_THRESHOLD) {
    const proceed = await ask(
      `"${info.displayName}" is ${formatBytes(info.sizeBytes)}. Download it now? ` +
        "This happens once — it's cached on your device afterward.",
      { title: "Download model", kind: "info" },
    );
    if (!proceed) throw new Error("Download cancelled.");
  }
  return downloadModelWithProgress(key);
}

async function selectModel(key) {
  const previous = selectedModelKey;
  selectedModelKey = key;
  populateModelSelect();
  renderModelBar();
  renderSettingsModelList();
  try {
    await ensureModelReady(key);
    await invoke("set_selected_model", { key });
  } catch (err) {
    selectedModelKey = previous;
    populateModelSelect();
    renderModelBar();
    renderSettingsModelList();
    setStatus(`Failed: ${err}`);
    return;
  }
  await refreshModelsList();
  setStatus("Ready.");
}

async function downloadOneModel(key) {
  try {
    await ensureModelReady(key);
    await refreshModelsList();
  } catch (err) {
    setStatus(`Failed: ${err}`);
  }
}

async function clearOneModel(key) {
  try {
    await invoke("clear_model", { key });
    await refreshModelsList();
  } catch (err) {
    setStatus(`Failed: ${err}`);
  }
}

modelSelect.addEventListener("change", () => {
  selectModel(modelSelect.value).catch(() => {});
});

modelDownloadBtn.addEventListener("click", async () => {
  try {
    await ensureModelReady(selectedModelKey);
    await refreshModelsList();
    setStatus("Ready.");
  } catch (err) {
    setStatus(`Failed: ${err}`);
  }
});

clearAllModelsBtn.addEventListener("click", async () => {
  const proceed = await ask(
    "Remove all downloaded models from disk? You'll need to re-download them next time you use them.",
    { title: "Clear all models", kind: "warning" },
  );
  if (!proceed) return;
  try {
    await invoke("clear_all_models");
    await refreshModelsList();
    setStatus("Cleared all cached models.");
  } catch (err) {
    setStatus(`Failed: ${err}`);
  }
});

// ---------------------------------------------------------------------
// Settings modal
// ---------------------------------------------------------------------

function applyTheme(theme) {
  if (theme === "system") {
    delete document.documentElement.dataset.theme;
  } else {
    document.documentElement.dataset.theme = theme;
  }
}

settingsBtn.addEventListener("click", () => {
  renderSettingsModelList();
  settingsOverlay.hidden = false;
});

settingsCloseBtn.addEventListener("click", () => {
  settingsOverlay.hidden = true;
});

settingsOverlay.addEventListener("click", (event) => {
  if (event.target === settingsOverlay) settingsOverlay.hidden = true;
});

themeSelect.addEventListener("change", async () => {
  const theme = themeSelect.value;
  applyTheme(theme);
  try {
    await invoke("set_theme", { theme });
  } catch (err) {
    setStatus(`Failed: ${err}`);
  }
});

// ---------------------------------------------------------------------
// Tabs
// ---------------------------------------------------------------------

function activateTab(which) {
  const single = which === "single";
  tabSingle.classList.toggle("active", single);
  tabBatch.classList.toggle("active", !single);
  tabSingle.setAttribute("aria-selected", String(single));
  tabBatch.setAttribute("aria-selected", String(!single));
  panelSingle.hidden = !single;
  panelBatch.hidden = single;
}

tabSingle.addEventListener("click", () => activateTab("single"));
tabBatch.addEventListener("click", () => activateTab("batch"));

// ---------------------------------------------------------------------
// Output directory
// ---------------------------------------------------------------------

function refreshOutputDirDisplay() {
  outputDirDisplay.textContent = outputDir ?? "Same folder as the original image";
  clearOutputDirBtn.hidden = outputDir === null;
}

chooseOutputDirBtn.addEventListener("click", async () => {
  const picked = await open({ multiple: false, directory: true });
  if (picked) {
    outputDir = picked;
    refreshOutputDirDisplay();
  }
});

clearOutputDirBtn.addEventListener("click", () => {
  outputDir = null;
  refreshOutputDirDisplay();
});

// ---------------------------------------------------------------------
// Single mode
// ---------------------------------------------------------------------

singlePickBtn.addEventListener("click", async () => {
  const picked = await open({ multiple: false, directory: false, filters: IMAGE_FILTERS });
  if (picked) {
    await processSingle(picked);
  }
});

singleResetBtn.addEventListener("click", () => {
  singleResult.hidden = true;
  singleDropzone.hidden = false;
});

singleRevealBtn.addEventListener("click", () => {
  if (lastSingleOutputPath) {
    revealItemInDir(lastSingleOutputPath).catch((err) => setStatus(String(err)));
  }
});

/**
 * There's no real per-percent progress signal for a single inference call
 * (ONNX Runtime doesn't expose one), so this eases a fill toward 90% while
 * the request is in flight and snaps to 100% the moment it resolves — an
 * honest "still working" indicator rather than a claim of exact progress.
 */
function startFakeProgress(fillEl, labelEl) {
  let pct = 0;
  fillEl.style.width = "0%";
  labelEl.textContent = "0%";
  const timer = setInterval(() => {
    pct += (90 - pct) * 0.1 + 0.4;
    if (pct > 90) pct = 90;
    fillEl.style.width = `${pct}%`;
    labelEl.textContent = `${Math.round(pct)}%`;
  }, 150);
  return {
    finish() {
      clearInterval(timer);
      fillEl.style.width = "100%";
      labelEl.textContent = "100%";
    },
    stop() {
      clearInterval(timer);
    },
  };
}

async function processSingle(path) {
  if (busy) return;
  setBusy(true);
  singleDropzone.hidden = true;
  singleResult.hidden = true;
  singleLoadingText.textContent = "Preparing…";
  singleLoading.hidden = false;
  const progress = startFakeProgress(singleProgressFill, singleProgressLabel);
  try {
    setStatus("Preparing…");
    await ensureModelReady(selectedModelKey);

    setStatus("Removing background…");
    singleLoadingText.textContent = "Removing background…";
    const result = await invoke("remove_background_single", {
      inputPath: path,
      outputDir,
      modelKey: selectedModelKey,
    });

    progress.finish();
    previewBefore.src = result.beforeDataUrl;
    previewAfter.src = result.afterDataUrl;
    singleOutputPathEl.textContent = result.outputPath;
    lastSingleOutputPath = result.outputPath;

    singleLoading.hidden = true;
    singleDropzone.hidden = true;
    singleResult.hidden = false;
    setStatus("Done.");
  } catch (err) {
    progress.stop();
    singleLoading.hidden = true;
    singleDropzone.hidden = false;
    setStatus(`Failed: ${err}`);
  } finally {
    setBusy(false);
  }
}

// ---------------------------------------------------------------------
// Batch mode
// ---------------------------------------------------------------------

batchPickFilesBtn.addEventListener("click", async () => {
  const picked = await open({ multiple: true, directory: false, filters: IMAGE_FILTERS });
  if (picked && picked.length > 0) {
    await processBatch(picked);
  }
});

batchPickFolderBtn.addEventListener("click", async () => {
  const picked = await open({ multiple: false, directory: true });
  if (picked) {
    await processBatch([picked]);
  }
});

batchResetBtn.addEventListener("click", () => {
  batchResult.hidden = true;
  batchDropzone.hidden = false;
  batchFileList.innerHTML = "";
});

batchRevealBtn.addEventListener("click", () => {
  console.log("[reveal] lastBatchOutputDir =", lastBatchOutputDir);
  if (lastBatchOutputDir) {
    revealItemInDir(lastBatchOutputDir)
      .then(() => console.log("[reveal] succeeded"))
      .catch((err) => {
        console.error("[reveal] failed", err);
        setStatus(String(err));
      });
  }
});

function renderBatchRow(fileName) {
  const li = document.createElement("li");
  li.dataset.file = fileName;
  const thumbEl = document.createElement("div");
  thumbEl.className = "file-thumb";
  thumbEl.innerHTML = '<span class="file-thumb-spinner"></span>';
  const nameEl = document.createElement("span");
  nameEl.className = "file-name";
  nameEl.textContent = fileName;
  const statusEl = document.createElement("span");
  statusEl.className = "file-status pending";
  statusEl.textContent = "Waiting…";
  li.append(thumbEl, nameEl, statusEl);
  batchFileList.appendChild(li);
  return { thumbEl, statusEl };
}

async function processBatch(paths) {
  if (busy) return;
  setBusy(true);
  batchFileList.innerHTML = "";
  batchProgressFill.style.width = "0%";
  batchProgressLabel.textContent = "";

  batchDropzone.hidden = true;
  batchResult.hidden = false;

  const rowsByIndex = [];

  const unlisten = await listen("batch-progress", (event) => {
    const { index, total, fileName, status, outputPath, afterDataUrl, message } = event.payload;
    console.log("[batch-progress]", {
      index,
      status,
      outputPath,
      afterDataUrlLength: afterDataUrl ? afterDataUrl.length : null,
      afterDataUrlPrefix: afterDataUrl ? afterDataUrl.slice(0, 40) : null,
      message,
    });

    let row = rowsByIndex[index];
    if (!row) {
      row = renderBatchRow(fileName);
      rowsByIndex[index] = row;
    }
    const { thumbEl, statusEl } = row;

    if (status === "done") {
      statusEl.textContent = "Done";
      statusEl.className = "file-status done";
      lastBatchOutputDir = parentDir(outputPath);
      thumbEl.innerHTML = "";
      thumbEl.classList.add("checkerboard");
      const img = document.createElement("img");
      img.src = afterDataUrl;
      img.alt = "";
      thumbEl.appendChild(img);
    } else {
      statusEl.textContent = message ?? "Failed";
      statusEl.className = "file-status error";
      statusEl.title = message ?? "";
      thumbEl.innerHTML = '<span class="file-thumb-error">!</span>';
    }

    const done = index + 1;
    const pct = total > 0 ? (done / total) * 100 : 0;
    batchProgressFill.style.width = `${pct}%`;
    batchProgressLabel.textContent = `${done} / ${total}`;
  });

  try {
    setStatus("Preparing…");
    await ensureModelReady(selectedModelKey);

    setStatus("Removing backgrounds…");
    await invoke("remove_background_batch", { inputPaths: paths, outputDir, modelKey: selectedModelKey });
    setStatus("Batch complete.");
  } catch (err) {
    setStatus(`Failed: ${err}`);
  } finally {
    unlisten();
    setBusy(false);
  }
}

function parentDir(path) {
  if (!path) return null;
  const sep = path.lastIndexOf("/") >= 0 ? "/" : "\\";
  const idx = path.lastIndexOf(sep);
  return idx >= 0 ? path.slice(0, idx) : path;
}

// ---------------------------------------------------------------------
// Drag & drop (real filesystem paths, via the Tauri webview)
// ---------------------------------------------------------------------

function setupDropzone(el, onPaths) {
  const highlight = (on) => el.classList.toggle("drag-over", on);
  return { el, onPaths, highlight };
}

const dropzones = [
  setupDropzone(singleDropzone, async (paths) => {
    if (paths.length > 0) await processSingle(paths[0]);
  }),
  setupDropzone(batchDropzone, async (paths) => {
    if (paths.length > 0) await processBatch(paths);
  }),
];

// `position` on drag-drop events is in physical pixels; `getBoundingClientRect()`
// is in logical/CSS pixels, so it has to be converted before comparing the two
// (otherwise zone detection is wrong on any HiDPI/Retina display).
function zoneUnderPoint(physicalPosition) {
  const { x, y } = physicalPosition.toLogical(window.devicePixelRatio);
  for (const zone of dropzones) {
    if (zone.el.hidden) continue;
    const rect = zone.el.getBoundingClientRect();
    if (x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom) {
      return zone;
    }
  }
  return null;
}

async function setupDragAndDrop() {
  const webview = getCurrentWebview();
  await webview.onDragDropEvent((event) => {
    const payload = event.payload;
    if (payload.type === "enter" || payload.type === "over") {
      const zone = zoneUnderPoint(payload.position);
      for (const z of dropzones) z.highlight(z === zone);
    } else if (payload.type === "drop") {
      for (const z of dropzones) z.highlight(false);
      const zone = zoneUnderPoint(payload.position);
      if (zone && !busy) {
        zone.onPaths(payload.paths);
      }
    } else {
      for (const z of dropzones) z.highlight(false);
    }
  });
}

// ---------------------------------------------------------------------
// Resource usage (footer)
// ---------------------------------------------------------------------

async function pollResourceUsage() {
  try {
    const { cpuPercent, ramPercent } = await invoke("get_system_usage");
    const pad = "    ";
    resourceUsageEl.textContent = `CPU: ${Math.round(cpuPercent)}%${pad}|${pad}RAM: ${Math.round(ramPercent)}%`;
  } catch {
    // Non-critical; leave the last known reading in place.
  }
}

// ---------------------------------------------------------------------
// Startup
// ---------------------------------------------------------------------

async function init() {
  activateTab("single");
  refreshOutputDirDisplay();
  await setupDragAndDrop();

  pollResourceUsage();
  setInterval(pollResourceUsage, 2000);

  await listen("model-download-progress", (event) => {
    const { key, downloadedBytes, totalBytes } = event.payload;
    activeProgress[key] = { downloaded: downloadedBytes, total: totalBytes };
    renderModelBar();
    if (!settingsOverlay.hidden) renderSettingsModelList();
  });

  try {
    const [modelList, settings] = await Promise.all([invoke("list_models"), invoke("get_settings")]);
    models = modelList;
    selectedModelKey = settings.selectedModel;
    applyTheme(settings.theme);
    themeSelect.value = settings.theme;
    populateModelSelect();
    renderModelBar();
    setStatus("Ready.");
  } catch (err) {
    setStatus(`Could not load models: ${err}`);
  }
}

init();
