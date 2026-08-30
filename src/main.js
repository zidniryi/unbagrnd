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
const exportFormatSelect = $("export-format-select");
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
const singleEditBgBtn = $("single-edit-bg-btn");
const singleRefineBtn = $("single-refine-btn");

// ---- Refine editor ----
const refineOverlay = $("refine-editor-overlay");
const refineCanvas = $("refine-canvas");
const refineLoading = $("refine-loading");
const refineDoneBtn = $("refine-done-btn");
const refineDownloadBtn = $("refine-download-btn");
const refineUndoBtn = $("refine-undo-btn");
const refineRedoBtn = $("refine-redo-btn");
const refineModeRow = $("refine-mode-row");
const refineModeHint = $("refine-mode-hint");
const refineBrushSizeInput = $("refine-brush-size");
const refineBrushSizeLabel = $("refine-brush-size-label");
const refineRestoreToGroup = $("refine-restore-to-group");
const refineRestoreRow = $("refine-restore-row");
const refineRestoreOriginalThumb = $("refine-restore-original-thumb");
const refineRestoreStartThumb = $("refine-restore-start-thumb");
const refineClearBtn = $("refine-clear-btn");
const refineApplyBtn = $("refine-apply-btn");

// ---- Background editor ----
const bgEditorOverlay = $("background-editor-overlay");
const bgEditorPreview = $("bg-editor-preview");
const bgEditorLoading = $("bg-editor-loading");
const bgEditorDoneBtn = $("bg-editor-done-btn");
const bgEditorDownloadBtn = $("bg-editor-download-btn");
const bgSwatchesBasic = $("bg-swatches-basic");
const bgSwatchesPastel = $("bg-swatches-pastel");
const bgSwatchesNeutral = $("bg-swatches-neutral");
const bgSwatchesImageGroup = $("bg-swatches-image-group");
const bgSwatchesImage = $("bg-swatches-image");
const bgShadowEnable = $("bg-shadow-enable");
const bgShadowControls = $("bg-shadow-controls");
const bgShadowPresetsEl = $("bg-shadow-presets");
const bgShadowCustomControls = $("bg-shadow-custom-controls");
const bgShadowAngleInput = $("bg-shadow-angle");
const bgShadowDistanceInput = $("bg-shadow-distance");
const bgShadowOpacityInput = $("bg-shadow-opacity");
const bgShadowOpacityLabel = $("bg-shadow-opacity-label");

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
let exportFormat = "png"; // "png" | "webp" | "svg"
const activeProgress = {}; // key -> { downloaded, total } for in-flight downloads
const downloadPromises = new Map(); // key -> in-flight download promise (dedupe)
let lastSingleOutputPath = null;
let lastBatchOutputDir = null;
let busy = false;

// Background editor state (single-image mode only).
let bgBackgroundHex = null; // null = transparent
let bgShadowEnabled = false;
let bgShadowPreset = "natural"; // "natural" | "overhead" | "left" | "right" | "custom"
let bgShadowOpacity = 50;
let bgShadowAngle = 0;
let bgShadowDistance = 8;
let bgPreviewDebounceTimer = null;
let bgPreviewRequestId = 0;

// Pristine snapshots of the current single-image session, captured once
// right after processing and never overwritten by later edits — these are
// the refine panel's "Original" (raw source photo) and "Start" (the
// model's own cutout) restore targets.
let sessionOriginalDataUrl = null;
let sessionStartDataUrl = null;

// Refine editor state (single-image mode only).
let refineMode = "erase"; // "erase" | "restore"
let refineRestoreTo = "original"; // "original" | "start"
let refineBrushPercent = 10; // 1-40, % of the image's longer side
let refineStrokes = []; // uncommitted strokes since the last apply/clear
let refineCurrentStroke = null; // in-progress stroke while the pointer is down
let refineUndoDepth = 0; // mirrors the backend's undo stack size
let refineRedoDepth = 0; // mirrors the backend's redo stack size
let refineBaseImg = null; // last committed/previewed result, backs the canvas
let refineOriginalImg = null; // preloaded "Original" source, for the local live-paint proxy
let refineStartImg = null; // preloaded "Start" source, for the local live-paint proxy
let refineSourceImg = null; // whichever of the above matches refineRestoreTo
let refinePointerDown = false;
let refineLastPoint = null;
let refinePreviewDebounceTimer = null;
let refinePreviewRequestId = 0;
let refineLocked = false; // true while an apply/undo/redo request is in flight

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

exportFormatSelect.addEventListener("change", async () => {
  const previous = exportFormat;
  exportFormat = exportFormatSelect.value;
  try {
    await invoke("set_export_format", { format: exportFormat });
  } catch (err) {
    exportFormat = previous;
    exportFormatSelect.value = previous;
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
      exportFormat,
    });

    progress.finish();
    previewBefore.src = result.beforeDataUrl;
    previewAfter.src = result.afterDataUrl;
    sessionOriginalDataUrl = result.beforeDataUrl;
    sessionStartDataUrl = result.afterDataUrl;
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
// Background editor (single-image mode only)
// ---------------------------------------------------------------------

const BASIC_COLORS = ["#ffffff", "#000000", "#ef4444", "#14b8a6"];
const PASTEL_COLORS = ["#fbe4ea", "#fbe8d3", "#fdf6e3", "#e3f7ea", "#dbeafe", "#e6e6fa"];
const NEUTRAL_COLORS = ["#d1d5db", "#9ca3af", "#6b7280", "#e7d9c9", "#c9a97e", "#7c5a3a"];

function markSwatchSelected(btn) {
  for (const el of document.querySelectorAll(".swatch")) el.classList.remove("selected");
  btn.classList.add("selected");
}

function selectBackgroundSwatch(hex, btn) {
  bgBackgroundHex = hex; // null for transparent
  markSwatchSelected(btn);
  scheduleBgPreviewUpdate();
}

function buildSwatchRow(container, colors) {
  container.innerHTML = "";
  for (const hex of colors) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "swatch";
    btn.style.background = hex;
    btn.title = hex;
    btn.addEventListener("click", () => selectBackgroundSwatch(hex, btn));
    container.appendChild(btn);
  }
}

/** Builds the basic-colors row (transparent + fixed swatches + a custom color picker) and returns the transparent swatch button, so the caller can mark it selected by default. */
function buildBasicSwatchRow() {
  bgSwatchesBasic.innerHTML = "";

  const transparentBtn = document.createElement("button");
  transparentBtn.type = "button";
  transparentBtn.className = "swatch transparent-swatch";
  transparentBtn.title = "Transparent";
  transparentBtn.addEventListener("click", () => selectBackgroundSwatch(null, transparentBtn));
  bgSwatchesBasic.appendChild(transparentBtn);

  for (const hex of BASIC_COLORS) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "swatch";
    btn.style.background = hex;
    btn.title = hex;
    btn.addEventListener("click", () => selectBackgroundSwatch(hex, btn));
    bgSwatchesBasic.appendChild(btn);
  }

  const customInput = document.createElement("input");
  customInput.type = "color";
  customInput.className = "swatch custom-swatch";
  customInput.title = "Custom color";
  customInput.value = "#8855ee";
  customInput.addEventListener("input", () => selectBackgroundSwatch(customInput.value, customInput));
  bgSwatchesBasic.appendChild(customInput);

  return transparentBtn;
}

/**
 * Samples the (already capped-size) "after" preview image client-side to
 * pick two representative colors for the "From your image" swatch row: the
 * average color of the subject's visible pixels, and its most saturated
 * color. Everything here runs locally in a throwaway canvas — no backend
 * round trip needed for a couple of swatch colors.
 */
function extractImageColors(dataUrl) {
  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => {
      const size = 40;
      const canvas = document.createElement("canvas");
      canvas.width = size;
      canvas.height = size;
      const ctx = canvas.getContext("2d");
      ctx.drawImage(img, 0, 0, size, size);

      let data;
      try {
        data = ctx.getImageData(0, 0, size, size).data;
      } catch {
        resolve([]);
        return;
      }

      let rSum = 0;
      let gSum = 0;
      let bSum = 0;
      let n = 0;
      let vivid = null;
      let vividSat = -1;
      for (let i = 0; i < data.length; i += 4) {
        const [r, g, b, a] = [data[i], data[i + 1], data[i + 2], data[i + 3]];
        if (a < 32) continue;
        rSum += r;
        gSum += g;
        bSum += b;
        n++;
        const max = Math.max(r, g, b);
        const min = Math.min(r, g, b);
        const sat = max === 0 ? 0 : (max - min) / max;
        if (sat > vividSat) {
          vividSat = sat;
          vivid = [r, g, b];
        }
      }
      if (n === 0) {
        resolve([]);
        return;
      }

      const toHex = ([r, g, b]) => `#${[r, g, b].map((c) => c.toString(16).padStart(2, "0")).join("")}`;
      const avgHex = toHex([Math.round(rSum / n), Math.round(gSum / n), Math.round(bSum / n)]);
      const colors = [avgHex];
      if (vivid && toHex(vivid) !== avgHex) colors.push(toHex(vivid));
      resolve(colors);
    };
    img.onerror = () => resolve([]);
    img.src = dataUrl;
  });
}

function currentShadowSpecOrNull() {
  if (!bgShadowEnabled) return null;
  return {
    preset: bgShadowPreset,
    opacity: bgShadowOpacity,
    angleDeg: bgShadowPreset === "custom" ? bgShadowAngle : null,
    distancePct: bgShadowPreset === "custom" ? bgShadowDistance : null,
  };
}

function scheduleBgPreviewUpdate() {
  // Shown immediately on interaction rather than after the debounce delay,
  // so picking a color or toggling the shadow always gives instant
  // feedback that something is happening, not just a frozen preview.
  bgEditorLoading.hidden = false;
  bgEditorPreview.classList.add("is-loading");
  clearTimeout(bgPreviewDebounceTimer);
  bgPreviewDebounceTimer = setTimeout(updateBgPreview, 90);
}

async function updateBgPreview() {
  const requestId = ++bgPreviewRequestId;
  try {
    const dataUrl = await invoke("preview_background", {
      backgroundHex: bgBackgroundHex,
      shadow: currentShadowSpecOrNull(),
    });
    if (requestId !== bgPreviewRequestId) return; // superseded by a newer request
    bgEditorPreview.src = dataUrl;
  } catch (err) {
    setStatus(`Failed: ${err}`);
  } finally {
    // Only the most recent request gets to clear the loading state — an
    // older, superseded request finishing later must not hide it while a
    // newer one is still in flight.
    if (requestId === bgPreviewRequestId) {
      bgEditorLoading.hidden = true;
      bgEditorPreview.classList.remove("is-loading");
    }
  }
}

singleEditBgBtn.addEventListener("click", async () => {
  bgBackgroundHex = null;
  bgShadowEnabled = false;
  bgShadowPreset = "natural";
  bgShadowOpacity = 50;
  bgShadowAngle = 0;
  bgShadowDistance = 8;

  bgShadowEnable.checked = false;
  bgShadowControls.hidden = true;
  bgShadowOpacityInput.value = "50";
  bgShadowOpacityLabel.textContent = "50%";
  bgShadowAngleInput.value = "0";
  bgShadowDistanceInput.value = "8";
  bgShadowCustomControls.hidden = true;
  for (const btn of bgShadowPresetsEl.querySelectorAll(".bg-shadow-preset")) {
    btn.classList.toggle("active", btn.dataset.preset === "natural");
  }

  const transparentBtn = buildBasicSwatchRow();
  buildSwatchRow(bgSwatchesPastel, PASTEL_COLORS);
  buildSwatchRow(bgSwatchesNeutral, NEUTRAL_COLORS);
  transparentBtn.classList.add("selected");

  bgEditorPreview.src = previewAfter.src;
  bgEditorPreview.classList.remove("is-loading");
  bgEditorLoading.hidden = true;
  bgEditorOverlay.hidden = false;

  const colors = await extractImageColors(previewAfter.src);
  if (colors.length > 0) {
    bgSwatchesImageGroup.hidden = false;
    buildSwatchRow(bgSwatchesImage, colors);
  } else {
    bgSwatchesImageGroup.hidden = true;
  }
});

bgEditorDoneBtn.addEventListener("click", () => {
  bgEditorOverlay.hidden = true;
});

bgShadowEnable.addEventListener("change", () => {
  bgShadowEnabled = bgShadowEnable.checked;
  bgShadowControls.hidden = !bgShadowEnabled;
  scheduleBgPreviewUpdate();
});

bgShadowPresetsEl.addEventListener("click", (event) => {
  const btn = event.target.closest(".bg-shadow-preset");
  if (!btn) return;
  bgShadowPreset = btn.dataset.preset;
  for (const el of bgShadowPresetsEl.querySelectorAll(".bg-shadow-preset")) {
    el.classList.toggle("active", el === btn);
  }
  bgShadowCustomControls.hidden = bgShadowPreset !== "custom";
  scheduleBgPreviewUpdate();
});

bgShadowAngleInput.addEventListener("input", () => {
  bgShadowAngle = Number(bgShadowAngleInput.value);
  scheduleBgPreviewUpdate();
});

bgShadowDistanceInput.addEventListener("input", () => {
  bgShadowDistance = Number(bgShadowDistanceInput.value);
  scheduleBgPreviewUpdate();
});

bgShadowOpacityInput.addEventListener("input", () => {
  bgShadowOpacity = Number(bgShadowOpacityInput.value);
  bgShadowOpacityLabel.textContent = `${bgShadowOpacity}%`;
  scheduleBgPreviewUpdate();
});

bgEditorDownloadBtn.addEventListener("click", async () => {
  if (busy) return;
  setBusy(true);
  try {
    setStatus("Saving…");
    const outputPath = await invoke("export_background", {
      outputDir,
      backgroundHex: bgBackgroundHex,
      shadow: currentShadowSpecOrNull(),
      exportFormat,
    });
    setStatus(`Saved to ${outputPath}`);
  } catch (err) {
    setStatus(`Failed: ${err}`);
  } finally {
    setBusy(false);
  }
});

// ---------------------------------------------------------------------
// Refine editor (single-image mode only)
// ---------------------------------------------------------------------

function loadImage(src) {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = reject;
    img.src = src;
  });
}

function redrawRefineCanvasBase() {
  const ctx = refineCanvas.getContext("2d");
  ctx.clearRect(0, 0, refineCanvas.width, refineCanvas.height);
  if (refineBaseImg) ctx.drawImage(refineBaseImg, 0, 0, refineCanvas.width, refineCanvas.height);
}

/**
 * Paints one brush dab directly onto the canvas for instant visual
 * feedback while dragging — erase clears pixels to transparent, restore
 * reveals the chosen source image. This is a rough local approximation
 * (hard-edged, no feathering); the debounced `preview_refine` round trip
 * started in `endRefineStroke` replaces it with the backend's accurate,
 * feathered result once it lands.
 */
function paintLocalDab(x, y, radiusPx) {
  const ctx = refineCanvas.getContext("2d");
  ctx.save();
  ctx.beginPath();
  ctx.arc(x, y, radiusPx, 0, Math.PI * 2);
  ctx.clip();
  if (refineMode === "restore" && refineSourceImg) {
    ctx.drawImage(refineSourceImg, 0, 0, refineCanvas.width, refineCanvas.height);
  } else {
    ctx.clearRect(x - radiusPx, y - radiusPx, radiusPx * 2, radiusPx * 2);
  }
  ctx.restore();
}

/** Paints dabs along a segment so a fast drag paints a continuous line instead of leaving gaps. */
function paintSegmentLocally(x0, y0, x1, y1, radiusPx) {
  const dist = Math.hypot(x1 - x0, y1 - y0);
  const step = Math.max(radiusPx / 3, 1);
  const steps = Math.max(1, Math.ceil(dist / step));
  for (let i = 0; i <= steps; i++) {
    const t = i / steps;
    paintLocalDab(x0 + (x1 - x0) * t, y0 + (y1 - y0) * t, radiusPx);
  }
}

function refineBrushRadiusPx() {
  return (refineBrushPercent / 100) * Math.max(refineCanvas.width, refineCanvas.height);
}

function canvasPointFromEvent(event) {
  const rect = refineCanvas.getBoundingClientRect();
  const scaleX = refineCanvas.width / rect.width;
  const scaleY = refineCanvas.height / rect.height;
  return {
    x: (event.clientX - rect.left) * scaleX,
    y: (event.clientY - rect.top) * scaleY,
  };
}

function updateRefineStrokeButtons() {
  const hasPending = refineStrokes.length > 0;
  refineApplyBtn.disabled = !hasPending;
  refineClearBtn.disabled = !hasPending;
  for (const el of refineModeRow.querySelectorAll(".bg-shadow-preset")) el.disabled = hasPending;
  for (const el of refineRestoreRow.querySelectorAll(".refine-restore-btn")) el.disabled = hasPending;
}

function updateRefineUndoRedoButtons() {
  refineUndoBtn.disabled = refineUndoDepth <= 0;
  refineRedoBtn.disabled = refineRedoDepth <= 0;
}

refineCanvas.addEventListener("pointerdown", (event) => {
  if (refineLocked) return;
  refinePointerDown = true;
  refineCanvas.setPointerCapture(event.pointerId);
  const p = canvasPointFromEvent(event);
  refineCurrentStroke = {
    points: [{ x: p.x / refineCanvas.width, y: p.y / refineCanvas.height }],
    radius: refineBrushPercent / 100,
  };
  paintLocalDab(p.x, p.y, refineBrushRadiusPx());
  refineLastPoint = p;
});

refineCanvas.addEventListener("pointermove", (event) => {
  if (!refinePointerDown || !refineCurrentStroke) return;
  const p = canvasPointFromEvent(event);
  paintSegmentLocally(refineLastPoint.x, refineLastPoint.y, p.x, p.y, refineBrushRadiusPx());
  refineCurrentStroke.points.push({ x: p.x / refineCanvas.width, y: p.y / refineCanvas.height });
  refineLastPoint = p;
});

function endRefineStroke() {
  if (!refinePointerDown) return;
  refinePointerDown = false;
  if (refineCurrentStroke) {
    refineStrokes.push(refineCurrentStroke);
    refineCurrentStroke = null;
    updateRefineStrokeButtons();
    scheduleRefinePreviewUpdate();
  }
}

refineCanvas.addEventListener("pointerup", endRefineStroke);
refineCanvas.addEventListener("pointercancel", endRefineStroke);

function scheduleRefinePreviewUpdate() {
  refineLoading.hidden = false;
  clearTimeout(refinePreviewDebounceTimer);
  refinePreviewDebounceTimer = setTimeout(updateRefinePreview, 150);
}

async function updateRefinePreview() {
  const requestId = ++refinePreviewRequestId;
  try {
    const dataUrl = await invoke("preview_refine", {
      strokes: refineStrokes,
      mode: refineMode,
      restoreTo: refineRestoreTo,
    });
    if (requestId !== refinePreviewRequestId) return; // superseded by a newer request
    refineBaseImg = await loadImage(dataUrl);
    redrawRefineCanvasBase();
  } catch (err) {
    setStatus(`Failed: ${err}`);
  } finally {
    if (requestId === refinePreviewRequestId) refineLoading.hidden = true;
  }
}

refineModeRow.addEventListener("click", (event) => {
  const btn = event.target.closest(".bg-shadow-preset");
  if (!btn || btn.disabled) return;
  refineMode = btn.dataset.mode;
  for (const el of refineModeRow.querySelectorAll(".bg-shadow-preset")) {
    el.classList.toggle("active", el === btn);
  }
  refineModeHint.textContent =
    refineMode === "erase"
      ? "Paint over parts of the subject you want to remove."
      : "Paint over parts of the subject you want to bring back.";
  refineRestoreToGroup.hidden = refineMode !== "restore";
});

refineRestoreRow.addEventListener("click", (event) => {
  const btn = event.target.closest(".refine-restore-btn");
  if (!btn || btn.disabled) return;
  refineRestoreTo = btn.dataset.restoreTo;
  for (const el of refineRestoreRow.querySelectorAll(".refine-restore-btn")) {
    el.classList.toggle("active", el === btn);
  }
  refineSourceImg = refineRestoreTo === "original" ? refineOriginalImg : refineStartImg;
});

refineBrushSizeInput.addEventListener("input", () => {
  refineBrushPercent = Number(refineBrushSizeInput.value);
  refineBrushSizeLabel.textContent = `${refineBrushPercent}%`;
});

refineClearBtn.addEventListener("click", () => {
  clearTimeout(refinePreviewDebounceTimer);
  refinePreviewRequestId++; // invalidate any in-flight preview response
  refineStrokes = [];
  refineCurrentStroke = null;
  updateRefineStrokeButtons();
  redrawRefineCanvasBase();
});

refineApplyBtn.addEventListener("click", async () => {
  if (refineStrokes.length === 0 || busy) return;
  clearTimeout(refinePreviewDebounceTimer);
  refinePreviewRequestId++; // invalidate any in-flight preview response
  setBusy(true);
  refineLocked = true;
  refineLoading.hidden = false;
  try {
    const dataUrl = await invoke("apply_refine", {
      strokes: refineStrokes,
      mode: refineMode,
      restoreTo: refineRestoreTo,
    });
    refineStrokes = [];
    refineUndoDepth = Math.min(refineUndoDepth + 1, 15);
    refineRedoDepth = 0;
    refineBaseImg = await loadImage(dataUrl);
    redrawRefineCanvasBase();
    previewAfter.src = dataUrl;
    updateRefineStrokeButtons();
    updateRefineUndoRedoButtons();
    setStatus("Applied.");
  } catch (err) {
    setStatus(`Failed: ${err}`);
  } finally {
    refineLoading.hidden = true;
    refineLocked = false;
    setBusy(false);
  }
});

refineUndoBtn.addEventListener("click", async () => {
  if (refineUndoDepth <= 0 || busy) return;
  setBusy(true);
  refineLocked = true;
  refineLoading.hidden = false;
  try {
    const dataUrl = await invoke("undo_refine");
    refineUndoDepth -= 1;
    refineRedoDepth = Math.min(refineRedoDepth + 1, 15);
    refineBaseImg = await loadImage(dataUrl);
    redrawRefineCanvasBase();
    previewAfter.src = dataUrl;
    updateRefineUndoRedoButtons();
  } catch (err) {
    setStatus(`Failed: ${err}`);
  } finally {
    refineLoading.hidden = true;
    refineLocked = false;
    setBusy(false);
  }
});

refineRedoBtn.addEventListener("click", async () => {
  if (refineRedoDepth <= 0 || busy) return;
  setBusy(true);
  refineLocked = true;
  refineLoading.hidden = false;
  try {
    const dataUrl = await invoke("redo_refine");
    refineRedoDepth -= 1;
    refineUndoDepth = Math.min(refineUndoDepth + 1, 15);
    refineBaseImg = await loadImage(dataUrl);
    redrawRefineCanvasBase();
    previewAfter.src = dataUrl;
    updateRefineUndoRedoButtons();
  } catch (err) {
    setStatus(`Failed: ${err}`);
  } finally {
    refineLoading.hidden = true;
    refineLocked = false;
    setBusy(false);
  }
});

refineDoneBtn.addEventListener("click", () => {
  refineOverlay.hidden = true;
});

refineDownloadBtn.addEventListener("click", async () => {
  if (busy) return;
  setBusy(true);
  try {
    setStatus("Saving…");
    const outputPath = await invoke("export_refine", { outputDir, exportFormat });
    setStatus(`Saved to ${outputPath}`);
  } catch (err) {
    setStatus(`Failed: ${err}`);
  } finally {
    setBusy(false);
  }
});

singleRefineBtn.addEventListener("click", async () => {
  refineMode = "erase";
  refineRestoreTo = "original";
  refineBrushPercent = 10;
  refineStrokes = [];
  refineCurrentStroke = null;
  refineUndoDepth = 0;
  refineRedoDepth = 0;

  refineBrushSizeInput.value = "10";
  refineBrushSizeLabel.textContent = "10%";
  refineModeHint.textContent = "Paint over parts of the subject you want to remove.";
  refineRestoreToGroup.hidden = true;
  for (const el of refineModeRow.querySelectorAll(".bg-shadow-preset")) {
    el.classList.toggle("active", el.dataset.mode === "erase");
    el.disabled = false;
  }
  for (const el of refineRestoreRow.querySelectorAll(".refine-restore-btn")) {
    el.classList.toggle("active", el.dataset.restoreTo === "original");
    el.disabled = false;
  }
  updateRefineStrokeButtons();
  updateRefineUndoRedoButtons();

  refineRestoreOriginalThumb.src = sessionOriginalDataUrl;
  refineRestoreStartThumb.src = sessionStartDataUrl;

  refineLoading.hidden = false;
  refineOverlay.hidden = false;
  try {
    const [baseImg, originalImg, startImg] = await Promise.all([
      loadImage(previewAfter.src),
      loadImage(sessionOriginalDataUrl),
      loadImage(sessionStartDataUrl),
    ]);
    refineBaseImg = baseImg;
    refineOriginalImg = originalImg;
    refineStartImg = startImg;
    refineSourceImg = refineOriginalImg;

    refineCanvas.width = baseImg.naturalWidth;
    refineCanvas.height = baseImg.naturalHeight;
    redrawRefineCanvasBase();
  } catch (err) {
    setStatus(`Failed: ${err}`);
    refineOverlay.hidden = true;
  } finally {
    refineLoading.hidden = true;
  }
});

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
  if (lastBatchOutputDir) {
    revealItemInDir(lastBatchOutputDir).catch((err) => setStatus(String(err)));
  }
});

function renderBatchRow(fileName) {
  const li = document.createElement("li");
  li.className = "file-card";
  li.dataset.file = fileName;
  li.title = fileName;
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

/**
 * Shows the original image in `thumbEl` right away (with a spinner overlaid
 * on top, since it's still just the "before" picture), so the row isn't
 * blank while the real background-removal pass — which is much slower — is
 * still running. Swapped out for the real result once that pass finishes.
 */
async function showBeforeThumb(thumbEl, path) {
  try {
    const dataUrl = await invoke("preview_image", { path });
    if (thumbEl.dataset.settled) return; // already got the real result while this was loading
    thumbEl.innerHTML = '<div class="file-thumb-overlay"><span class="file-thumb-spinner"></span></div>';
    const img = document.createElement("img");
    img.src = dataUrl;
    img.alt = "";
    thumbEl.prepend(img);
  } catch {
    // Non-critical: the row just keeps showing a spinner until the real result arrives.
  }
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

    let row = rowsByIndex[index];
    if (!row) {
      row = renderBatchRow(fileName);
      rowsByIndex[index] = row;
    }
    const { thumbEl, statusEl } = row;

    thumbEl.dataset.settled = "1";
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
    const expandedPaths = await invoke("expand_batch_paths", { paths });
    for (const [index, filePath] of expandedPaths.entries()) {
      const fileName = filePath.split(/[/\\]/).pop() ?? filePath;
      const row = renderBatchRow(fileName);
      rowsByIndex[index] = row;
      showBeforeThumb(row.thumbEl, filePath);
    }
    batchProgressLabel.textContent = `0 / ${expandedPaths.length}`;

    await ensureModelReady(selectedModelKey);

    setStatus("Removing backgrounds…");
    await invoke("remove_background_batch", {
      inputPaths: expandedPaths,
      outputDir,
      modelKey: selectedModelKey,
      exportFormat,
    });
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
    exportFormat = settings.exportFormat;
    exportFormatSelect.value = settings.exportFormat;
    populateModelSelect();
    renderModelBar();
    setStatus("Ready.");
  } catch (err) {
    setStatus(`Could not load models: ${err}`);
  }
}

init();
