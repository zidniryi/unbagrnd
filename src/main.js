// This project has no bundler (see README / tauri.conf.json `app.withGlobalTauri`),
// so the Tauri JS APIs are read off the `window.__TAURI__` global injected at
// runtime rather than imported as ES modules (bare specifiers like
// "@tauri-apps/api/core" don't resolve without a bundler).
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getCurrentWebview } = window.__TAURI__.webview;
const { ask, open, save } = window.__TAURI__.dialog;
const { revealItemInDir } = window.__TAURI__.opener;
const { getVersion } = window.__TAURI__.app;

const IMAGE_FILTERS = [
  { name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "bmp", "tiff", "tif", "gif"] },
];

// Models at or above this size get a confirmation dialog before downloading.
const LARGE_MODEL_THRESHOLD = 100 * 1024 * 1024;

// Android has no folder-picker: `tauri-plugin-dialog`'s `open({ directory:
// true })` always rejects there (`FolderPickerNotImplemented` - there's no
// Storage Access Framework tree-picker wired up), and scoped storage means
// an arbitrary chosen folder usually isn't even writable via a plain path
// anyway. Exports already auto-publish to the Gallery on Android
// regardless (see the Rust `publish_to_gallery` export path), so the
// custom-output-folder UI is simply hidden there instead of silently
// failing when tapped.
const IS_ANDROID = /Android/i.test(navigator.userAgent);

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
const singleMaskBtn = $("single-mask-btn");

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
const refineMaskBtn = $("refine-mask-btn");
const edgeHint = $("edge-hint");
const edgeShiftInput = $("edge-shift");
const edgeSmoothInput = $("edge-smooth");
const edgeFeatherInput = $("edge-feather");
const edgeShiftLabel = $("edge-shift-label");
const edgeSmoothLabel = $("edge-smooth-label");
const edgeFeatherLabel = $("edge-feather-label");
const edgeResetBtn = $("edge-reset-btn");
const edgeApplyBtn = $("edge-apply-btn");

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
const batchOptionsEl = $("batch-options");
const batchSwatchesEl = $("batch-swatches");
const batchShadowEnable = $("batch-shadow-enable");
const batchShadowControls = $("batch-shadow-controls");
const batchShadowPresetsEl = $("batch-shadow-presets");
const batchShadowOpacityInput = $("batch-shadow-opacity");
const batchShadowOpacityLabel = $("batch-shadow-opacity-label");
const batchNameTemplateInput = $("batch-name-template");
const batchDropzone = $("batch-dropzone");
const batchPickFilesBtn = $("batch-pick-files-btn");
const batchPickFolderBtn = $("batch-pick-folder-btn");
const batchResult = $("batch-result");
const batchProgressFill = $("batch-progress-fill");
const batchProgressLabel = $("batch-progress-label");
const batchFileList = $("batch-file-list");
const batchRevealBtn = $("batch-reveal-btn");
const batchResetBtn = $("batch-reset-btn");
const batchCancelBtn = $("batch-cancel-btn");
const batchRetryBtn = $("batch-retry-btn");
const batchZipBtn = $("batch-zip-btn");
const batchSummaryEl = $("batch-summary");
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
// Edge sliders, in the sliders' own integer units (each is 0.1% of the
// image's longer side - see `currentEdgeSpec`). Non-zero means an edge
// adjustment is previewed on the canvas but not yet applied.
let edgeShift = 0; // -20..20
let edgeSmooth = 0; // 0..10
let edgeFeather = 0; // 0..20
let edgePreviewDebounceTimer = null;
let edgePreviewRequestId = 0;

// Batch mode: what the results list currently on screen is showing.
// `entries[i]` is the i-th file of the batch (its position is also the
// `index` the backend reports progress with); `options` and `outputDir` are
// captured when the run starts so a retry produces consistent output.
let batch = null; // { entries, options, outputDir } | null
let batchBackgroundHex = null; // null = keep transparent
let batchShadowPreset = "natural";

function setStatus(text) {
  statusLine.textContent = text;
  // The status line sits at the bottom of the page - off-screen on a phone
  // and hidden under the editors - so a failure is also raised as a toast.
  if (text.startsWith("Failed:")) showToast(text, { kind: "error" });
}

// ---------------------------------------------------------------------
// Toasts
// ---------------------------------------------------------------------

const toastRegion = $("toast-region");
const TOAST_LIMIT = 3;
const TOAST_MS = { success: 4500, warning: 6500, error: 7000 };

function dismissToast(el) {
  clearTimeout(el._timer);
  if (el.classList.contains("is-leaving")) return;
  el.classList.add("is-leaving");
  setTimeout(() => el.remove(), 180);
}

function armToast(el, duration) {
  clearTimeout(el._timer);
  el._timer = setTimeout(() => dismissToast(el), duration);
}

/**
 * Shows a short-lived confirmation. `kind` is "success" | "warning" |
 * "error". With `actionLabel` + `onAction` a button is added (e.g. "Show").
 * An identical toast that's still up just has its timer refreshed, so a
 * burst of the same error doesn't stack up.
 */
function showToast(message, { kind = "success", actionLabel = null, onAction = null, duration = null } = {}) {
  if (!toastRegion) return;
  const ms = duration ?? TOAST_MS[kind] ?? TOAST_MS.success;

  for (const existing of toastRegion.children) {
    if (existing.dataset.message === message && existing.dataset.kind === kind) {
      existing.classList.remove("is-leaving");
      existing._ms = ms;
      armToast(existing, ms);
      return;
    }
  }

  const el = document.createElement("div");
  el.className = `toast toast--${kind}`;
  el.dataset.message = message;
  el.dataset.kind = kind;
  el.setAttribute("role", kind === "error" ? "alert" : "status");
  el._ms = ms;

  const text = document.createElement("span");
  text.className = "toast__text";
  text.textContent = message;
  el.appendChild(text);

  // `data-keep-enabled`: setBusy() disables every button while work runs,
  // but a toast's own controls should stay usable.
  if (actionLabel && onAction) {
    const action = document.createElement("button");
    action.type = "button";
    action.className = "toast__action";
    action.textContent = actionLabel;
    action.setAttribute("data-keep-enabled", "");
    action.addEventListener("click", () => {
      dismissToast(el);
      onAction();
    });
    el.appendChild(action);
  }

  const close = document.createElement("button");
  close.type = "button";
  close.className = "toast__close";
  close.textContent = "✕";
  close.setAttribute("aria-label", "Dismiss");
  close.setAttribute("data-keep-enabled", "");
  close.addEventListener("click", () => dismissToast(el));
  el.appendChild(close);

  // Hovering pauses the countdown so there's time to reach the button.
  el.addEventListener("mouseenter", () => clearTimeout(el._timer));
  el.addEventListener("mouseleave", () => armToast(el, el._ms));

  toastRegion.appendChild(el);
  while (toastRegion.children.length > TOAST_LIMIT) {
    toastRegion.firstElementChild.remove();
  }
  armToast(el, ms);
}

/** Last path segment, for showing a file name instead of a long path. */
function baseName(path) {
  return String(path).split(/[/\\]/).pop() || String(path);
}

/** Reveals `path` in the file manager (or opens it in the Gallery on Android, where that's unsupported). */
function revealSavedFile(path) {
  if (!path) return;
  revealItemInDir(path).catch(() => {
    invoke("reveal_last_export_in_gallery").catch((err) => setStatus(String(err)));
  });
}

/** "Saved" toast with a Show action for a file the app just wrote. */
function toastSaved(label, path) {
  showToast(`${label} - ${baseName(path)}`, {
    actionLabel: "Show",
    onAction: () => revealSavedFile(path),
  });
}

function setBusy(isBusy) {
  busy = isBusy;
  for (const btn of document.querySelectorAll("button")) {
    // Controls that must stay usable while work is running (batch Cancel).
    if (btn.hasAttribute("data-keep-enabled")) continue;
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
      showToast(`${info.displayName} downloaded`);
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
  outputDirDisplay.textContent = IS_ANDROID
    ? "Saved next to the source photo, and to your Gallery"
    : (outputDir ?? "Same folder as the original image");
  clearOutputDirBtn.hidden = outputDir === null;
}

if (IS_ANDROID) {
  chooseOutputDirBtn.hidden = true;
} else {
  chooseOutputDirBtn.addEventListener("click", async () => {
    try {
      const picked = await open({ multiple: false, directory: true });
      if (picked) {
        outputDir = picked;
        refreshOutputDirDisplay();
      }
    } catch (err) {
      setStatus(String(err));
    }
  });
}

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

/** Writes the working image's alpha channel as a black-and-white mask file. */
async function exportMask() {
  if (busy) return;
  setBusy(true);
  try {
    setStatus("Saving mask…");
    const outputPath = await invoke("export_mask", { outputDir, exportFormat });
    setStatus(`Saved mask to ${outputPath}`);
    toastSaved("Mask saved", outputPath);
  } catch (err) {
    setStatus(`Failed: ${err}`);
  } finally {
    setBusy(false);
    if (!refineOverlay.hidden) updateRefineControls();
  }
}

singleMaskBtn.addEventListener("click", exportMask);
refineMaskBtn.addEventListener("click", exportMask);

singleRevealBtn.addEventListener("click", () => {
  // On Android this falls back to opening the exported photo in the
  // gallery app, since there's no "reveal in folder" there.
  revealSavedFile(lastSingleOutputPath);
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
    toastSaved("Image saved", outputPath);
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

function redrawRefineCanvasBase(img = refineBaseImg) {
  const ctx = refineCanvas.getContext("2d");
  ctx.clearRect(0, 0, refineCanvas.width, refineCanvas.height);
  if (img) ctx.drawImage(img, 0, 0, refineCanvas.width, refineCanvas.height);
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
  updateEdgeControls();
}

/** Re-derives every refine control's enabled state; call after `setBusy(false)`, which enables all buttons. */
function updateRefineControls() {
  updateRefineStrokeButtons();
  updateRefineUndoRedoButtons();
}

function updateRefineUndoRedoButtons() {
  refineUndoBtn.disabled = refineUndoDepth <= 0;
  refineRedoBtn.disabled = refineRedoDepth <= 0;
}

refineCanvas.addEventListener("pointerdown", (event) => {
  // An un-applied edge adjustment owns the canvas until it's applied or reset.
  if (refineLocked || edgePending()) return;
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

refineModeRow.addEventListener("click", async (event) => {
  const btn = event.target.closest(".bg-shadow-preset");
  if (!btn || btn.disabled || btn.dataset.mode === refineMode) return;
  // Pending strokes were painted in the current mode and are applied with
  // it, so commit them before switching - otherwise they'd be re-interpreted
  // in the new mode.
  if (!(await applyPendingStrokes())) return;
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

refineRestoreRow.addEventListener("click", async (event) => {
  const btn = event.target.closest(".refine-restore-btn");
  if (!btn || btn.disabled || btn.dataset.restoreTo === refineRestoreTo) return;
  if (!(await applyPendingStrokes())) return;
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

// ---- Edge adjustments (shift / smooth / feather) ----

const EDGE_HINT_DEFAULT = edgeHint.textContent.replace(/\s+/g, " ").trim();

/** Slider units are 0.1% of the image's longer side; the backend takes percentages. */
function currentEdgeSpec() {
  return { shiftPct: edgeShift / 10, smoothPct: edgeSmooth / 10, featherPct: edgeFeather / 10 };
}

function edgePending() {
  return edgeShift !== 0 || edgeSmooth !== 0 || edgeFeather !== 0;
}

function formatEdgeAmount(units, signed) {
  const sign = signed && units > 0 ? "+" : "";
  return `${sign}${(units / 10).toFixed(1)}%`;
}

function renderEdgeLabels() {
  edgeShiftLabel.textContent = formatEdgeAmount(edgeShift, true);
  edgeSmoothLabel.textContent = formatEdgeAmount(edgeSmooth, false);
  edgeFeatherLabel.textContent = formatEdgeAmount(edgeFeather, false);
}

/**
 * Edge sliders are unavailable while brush strokes are pending (both would
 * be competing previews of the same canvas) or a request is in flight, and
 * painting is in turn blocked while an edge adjustment is pending.
 */
function updateEdgeControls() {
  const hasStrokes = refineStrokes.length > 0;
  const disabled = hasStrokes || refineLocked;
  for (const el of [edgeShiftInput, edgeSmoothInput, edgeFeatherInput]) el.disabled = disabled;
  const pending = edgePending();
  edgeApplyBtn.disabled = disabled || !pending;
  edgeResetBtn.disabled = disabled || !pending;
  if (hasStrokes) {
    edgeHint.textContent = "Apply or clear your brush strokes first to adjust the edge.";
  } else if (pending) {
    edgeHint.textContent = "Apply or reset this adjustment before painting again.";
  } else {
    edgeHint.textContent = EDGE_HINT_DEFAULT;
  }
}

/** Zeroes the sliders and drops any in-flight preview, without touching the canvas. */
function resetEdgeValues() {
  clearTimeout(edgePreviewDebounceTimer);
  edgePreviewRequestId++; // invalidate any in-flight preview response
  edgeShift = 0;
  edgeSmooth = 0;
  edgeFeather = 0;
  edgeShiftInput.value = "0";
  edgeSmoothInput.value = "0";
  edgeFeatherInput.value = "0";
  renderEdgeLabels();
}

function scheduleEdgePreviewUpdate() {
  clearTimeout(edgePreviewDebounceTimer);
  if (!edgePending()) {
    edgePreviewRequestId++;
    refineLoading.hidden = true;
    redrawRefineCanvasBase();
    return;
  }
  refineLoading.hidden = false;
  edgePreviewDebounceTimer = setTimeout(updateEdgePreview, 120);
}

async function updateEdgePreview() {
  const requestId = ++edgePreviewRequestId;
  try {
    const dataUrl = await invoke("preview_edges", { edges: currentEdgeSpec() });
    if (requestId !== edgePreviewRequestId) return; // superseded by a newer request
    const img = await loadImage(dataUrl);
    if (requestId !== edgePreviewRequestId) return;
    redrawRefineCanvasBase(img);
  } catch (err) {
    setStatus(`Failed: ${err}`);
  } finally {
    if (requestId === edgePreviewRequestId) refineLoading.hidden = true;
  }
}

function onEdgeSliderInput() {
  edgeShift = Number(edgeShiftInput.value);
  edgeSmooth = Number(edgeSmoothInput.value);
  edgeFeather = Number(edgeFeatherInput.value);
  renderEdgeLabels();
  updateEdgeControls();
  scheduleEdgePreviewUpdate();
}

edgeShiftInput.addEventListener("input", onEdgeSliderInput);
edgeSmoothInput.addEventListener("input", onEdgeSliderInput);
edgeFeatherInput.addEventListener("input", onEdgeSliderInput);

edgeResetBtn.addEventListener("click", () => {
  resetEdgeValues();
  refineLoading.hidden = true;
  redrawRefineCanvasBase();
  updateEdgeControls();
});

edgeApplyBtn.addEventListener("click", async () => {
  if (!edgePending() || busy) return;
  clearTimeout(edgePreviewDebounceTimer);
  edgePreviewRequestId++; // invalidate any in-flight preview response
  setBusy(true);
  refineLocked = true;
  updateEdgeControls();
  refineLoading.hidden = false;
  try {
    const dataUrl = await invoke("apply_edges", { edges: currentEdgeSpec() });
    refineUndoDepth = Math.min(refineUndoDepth + 1, 15);
    refineRedoDepth = 0;
    refineBaseImg = await loadImage(dataUrl);
    resetEdgeValues();
    redrawRefineCanvasBase();
    previewAfter.src = dataUrl;
    setStatus("Applied edge adjustment.");
    showToast("Edge adjustment applied", { duration: 2200 });
  } catch (err) {
    setStatus(`Failed: ${err}`);
  } finally {
    refineLoading.hidden = true;
    refineLocked = false;
    setBusy(false);
    updateRefineControls();
  }
});

refineClearBtn.addEventListener("click", () => {
  clearTimeout(refinePreviewDebounceTimer);
  refinePreviewRequestId++; // invalidate any in-flight preview response
  // The invalidated request's `finally` only hides the spinner for the
  // *current* request id, and a Clear inside the debounce window never
  // starts one at all - so hide it here, or the overlay keeps covering the
  // canvas and the editor looks stuck.
  refineLoading.hidden = true;
  refineStrokes = [];
  refineCurrentStroke = null;
  updateRefineStrokeButtons();
  redrawRefineCanvasBase();
});

/**
 * Commits the pending brush strokes to the working image. Resolves `true`
 * when there was nothing to apply or the apply succeeded, `false` if it
 * failed or the app was busy - callers that need the strokes committed
 * before doing something else (switching mode) must check this.
 */
async function applyPendingStrokes() {
  if (refineStrokes.length === 0) return true;
  if (busy) return false;
  clearTimeout(refinePreviewDebounceTimer);
  refinePreviewRequestId++; // invalidate any in-flight preview response
  setBusy(true);
  refineLocked = true;
  refineLoading.hidden = false;
  let ok = false;
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
    showToast("Brush edits applied", { duration: 2200 });
    ok = true;
  } catch (err) {
    setStatus(`Failed: ${err}`);
  } finally {
    refineLoading.hidden = true;
    refineLocked = false;
    setBusy(false);
    // `setBusy(false)` enables every button; put the refine panel's own
    // enabled/disabled state back.
    updateRefineControls();
  }
  return ok;
}

refineApplyBtn.addEventListener("click", () => {
  applyPendingStrokes();
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
    resetEdgeValues();
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
    // `setBusy(false)` enables every button; put the refine panel's own
    // enabled/disabled state back.
    updateRefineControls();
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
    resetEdgeValues();
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
    // `setBusy(false)` enables every button; put the refine panel's own
    // enabled/disabled state back.
    updateRefineControls();
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
    toastSaved("Cutout saved", outputPath);
  } catch (err) {
    setStatus(`Failed: ${err}`);
  } finally {
    setBusy(false);
    updateRefineControls();
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
  resetEdgeValues();

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
  batch = null;
  batchResult.hidden = true;
  setBatchSetupVisible(true);
  batchFileList.innerHTML = "";
});

batchRevealBtn.addEventListener("click", () => {
  // On Android this falls back to opening the last exported photo in the
  // gallery app instead of its (inaccessible) folder.
  revealSavedFile(lastBatchOutputDir);
});

// ---- Batch options (background, shadow, file name) ----

/** The options card and drop zone are shown together, and hidden once a run starts. */
function setBatchSetupVisible(visible) {
  batchDropzone.hidden = !visible;
  batchOptionsEl.hidden = !visible;
}

/** The default file name changes from `-nobg` to `-bg` once a background or shadow is applied. */
function updateBatchNamePlaceholder() {
  const compositing = batchBackgroundHex !== null || batchShadowEnable.checked;
  batchNameTemplateInput.placeholder = compositing ? "{name}-bg" : "{name}-nobg";
}

function buildBatchSwatches() {
  batchSwatchesEl.innerHTML = "";
  const select = (hex, el) => {
    batchBackgroundHex = hex;
    for (const swatch of batchSwatchesEl.querySelectorAll(".swatch")) swatch.classList.remove("selected");
    el.classList.add("selected");
    updateBatchNamePlaceholder();
  };

  const transparentBtn = document.createElement("button");
  transparentBtn.type = "button";
  transparentBtn.className = "swatch transparent-swatch selected";
  transparentBtn.title = "Transparent";
  transparentBtn.addEventListener("click", () => select(null, transparentBtn));
  batchSwatchesEl.appendChild(transparentBtn);

  for (const hex of [...BASIC_COLORS, ...PASTEL_COLORS]) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "swatch";
    btn.style.background = hex;
    btn.title = hex;
    btn.addEventListener("click", () => select(hex, btn));
    batchSwatchesEl.appendChild(btn);
  }

  const customInput = document.createElement("input");
  customInput.type = "color";
  customInput.className = "swatch custom-swatch";
  customInput.title = "Custom color";
  customInput.value = "#8855ee";
  customInput.addEventListener("input", () => select(customInput.value, customInput));
  batchSwatchesEl.appendChild(customInput);
}

batchShadowEnable.addEventListener("change", () => {
  batchShadowControls.hidden = !batchShadowEnable.checked;
  updateBatchNamePlaceholder();
});

batchShadowPresetsEl.addEventListener("click", (event) => {
  const btn = event.target.closest(".bg-shadow-preset");
  if (!btn) return;
  batchShadowPreset = btn.dataset.preset;
  for (const el of batchShadowPresetsEl.querySelectorAll(".bg-shadow-preset")) {
    el.classList.toggle("active", el === btn);
  }
});

batchShadowOpacityInput.addEventListener("input", () => {
  batchShadowOpacityLabel.textContent = `${batchShadowOpacityInput.value}%`;
});

function readBatchOptions() {
  return {
    backgroundHex: batchBackgroundHex,
    shadow: batchShadowEnable.checked
      ? {
          preset: batchShadowPreset,
          opacity: Number(batchShadowOpacityInput.value),
          angleDeg: null,
          distancePct: null,
        }
      : null,
    nameTemplate: batchNameTemplateInput.value.trim() || null,
  };
}

// ---- Batch run ----

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

/** Entry states: "pending" (waiting/running), "done", "error", "cancelled" (never ran). */
function batchCounts() {
  const counts = { pending: 0, done: 0, error: 0, cancelled: 0, total: 0 };
  for (const entry of batch?.entries ?? []) {
    counts[entry.status] += 1;
    counts.total += 1;
  }
  return counts;
}

function refreshBatchProgress() {
  const { done, error, total } = batchCounts();
  const finished = done + error;
  batchProgressFill.style.width = `${total > 0 ? (finished / total) * 100 : 0}%`;
  batchProgressLabel.textContent = `${finished} / ${total}`;
}

/** Updates the summary line and the Retry / ZIP buttons from the entries' states. */
function updateBatchButtons() {
  const counts = batchCounts();
  const retryable = counts.error + counts.cancelled;

  const parts = [];
  if (counts.done > 0) parts.push(`${counts.done} done`);
  if (counts.error > 0) parts.push(`${counts.error} failed`);
  if (counts.cancelled > 0) parts.push(`${counts.cancelled} not processed`);
  batchSummaryEl.textContent = parts.join(" · ");

  batchRetryBtn.hidden = busy || retryable === 0;
  batchRetryBtn.textContent =
    counts.cancelled === 0 ? `Retry failed (${retryable})` : `Process remaining (${retryable})`;
  // Android has no save-file dialog (see IS_ANDROID); outputs are already
  // published to the Gallery there.
  batchZipBtn.hidden = IS_ANDROID || busy || counts.done === 0;
}

function showEntryDone(entry, outputPath, afterDataUrl) {
  const { thumbEl, statusEl } = entry.row;
  entry.status = "done";
  entry.outputPath = outputPath;
  entry.message = null;
  lastBatchOutputDir = parentDir(outputPath);
  thumbEl.dataset.settled = "1";
  statusEl.textContent = "Done";
  statusEl.className = "file-status done";
  statusEl.title = "";
  thumbEl.innerHTML = "";
  thumbEl.classList.add("checkerboard");
  const img = document.createElement("img");
  img.src = afterDataUrl;
  img.alt = "";
  thumbEl.appendChild(img);
}

function showEntryError(entry, message) {
  const { thumbEl, statusEl } = entry.row;
  entry.status = "error";
  entry.outputPath = null;
  entry.message = message ?? "Failed";
  thumbEl.dataset.settled = "1";
  thumbEl.classList.remove("checkerboard");
  statusEl.textContent = entry.message;
  statusEl.className = "file-status error";
  statusEl.title = entry.message;
  thumbEl.innerHTML = '<span class="file-thumb-error">!</span>';
}

function showEntryCancelled(entry, text) {
  const { thumbEl, statusEl } = entry.row;
  entry.status = "cancelled";
  thumbEl.dataset.settled = "1";
  statusEl.textContent = text;
  statusEl.className = "file-status pending";
  // Keep the "before" picture, but stop implying it's still being worked on.
  for (const el of thumbEl.querySelectorAll(".file-thumb-overlay, .file-thumb-spinner")) el.remove();
}

/** Puts a row back to "Waiting…" with its original image, ready to be re-run. */
function resetEntryForRetry(entry) {
  const { thumbEl, statusEl } = entry.row;
  entry.status = "pending";
  entry.message = null;
  delete thumbEl.dataset.settled;
  thumbEl.classList.remove("checkerboard");
  thumbEl.innerHTML = '<span class="file-thumb-spinner"></span>';
  statusEl.textContent = "Waiting…";
  statusEl.className = "file-status pending";
  statusEl.title = "";
  showBeforeThumb(thumbEl, entry.path);
}

/**
 * Runs the backend batch for the entries at `indices` (positions in
 * `batch.entries`), updating rows as `batch-progress` events arrive.
 * Entries that never got a result - the user cancelled, or the run failed
 * outright - end up "cancelled" so they can be retried.
 */
async function runBatch(indices) {
  const { entries, options, outputDir: runOutputDir } = batch;

  const unlisten = await listen("batch-progress", (event) => {
    const { index, status, outputPath, afterDataUrl, message } = event.payload;
    const entry = entries[index];
    if (!entry) return;
    if (status === "done") {
      showEntryDone(entry, outputPath, afterDataUrl);
    } else {
      showEntryError(entry, message);
    }
    refreshBatchProgress();
  });

  let notRunText = "Not processed";
  batchCancelBtn.disabled = false;
  batchCancelBtn.hidden = false;
  try {
    setStatus("Removing backgrounds…");
    const summary = await invoke("remove_background_batch", {
      inputPaths: indices.map((i) => entries[i].path),
      positions: indices,
      total: entries.length,
      reservedOutputs: entries.filter((e) => e.status === "done").map((e) => e.outputPath),
      outputDir: runOutputDir,
      modelKey: selectedModelKey,
      exportFormat,
      options,
    });
    if (summary.cancelled) notRunText = "Cancelled";
    setStatus(summary.cancelled ? "Batch cancelled." : "Batch complete.");
  } finally {
    unlisten();
    batchCancelBtn.hidden = true;
    for (const i of indices) {
      if (entries[i].status === "pending") showEntryCancelled(entries[i], notRunText);
    }
  }
}

async function processBatch(paths) {
  if (busy) return;
  setBusy(true);
  let completed = false;
  batch = null;
  batchFileList.innerHTML = "";
  batchProgressFill.style.width = "0%";
  batchProgressLabel.textContent = "";
  batchSummaryEl.textContent = "";
  batchRetryBtn.hidden = true;
  batchZipBtn.hidden = true;

  setBatchSetupVisible(false);
  batchResult.hidden = false;

  try {
    setStatus("Preparing…");
    const expandedPaths = await invoke("expand_batch_paths", { paths });
    const entries = expandedPaths.map((path) => {
      const name = path.split(/[/\\]/).pop() ?? path;
      const row = renderBatchRow(name);
      showBeforeThumb(row.thumbEl, path);
      return { path, name, row, status: "pending", outputPath: null, message: null };
    });
    batch = { entries, options: readBatchOptions(), outputDir };
    batchProgressLabel.textContent = `0 / ${entries.length}`;

    await ensureModelReady(selectedModelKey);
    await runBatch(entries.map((_, i) => i));
    completed = true;
  } catch (err) {
    setStatus(`Failed: ${err}`);
  } finally {
    setBusy(false);
    updateBatchButtons();
    if (completed) toastBatchOutcome();
  }
}

/** One summary toast once a batch run (or a retry) has settled. */
function toastBatchOutcome() {
  const { done, error, cancelled } = batchCounts();
  const parts = [];
  if (done > 0) parts.push(`${done} done`);
  if (error > 0) parts.push(`${error} failed`);
  if (cancelled > 0) parts.push(`${cancelled} not processed`);
  if (parts.length === 0) return;

  const clean = error === 0 && cancelled === 0;
  const title = clean ? "Batch complete" : cancelled > 0 ? "Batch stopped" : "Batch finished with errors";
  showToast(`${title} - ${parts.join(", ")}`, {
    kind: clean ? "success" : "warning",
    actionLabel: done > 0 && lastBatchOutputDir ? "Show" : null,
    onAction: () => revealSavedFile(lastBatchOutputDir),
  });
}

batchCancelBtn.addEventListener("click", async () => {
  batchCancelBtn.disabled = true;
  setStatus("Cancelling after the current image…");
  try {
    await invoke("cancel_batch");
  } catch (err) {
    setStatus(`Failed: ${err}`);
  }
});

batchRetryBtn.addEventListener("click", async () => {
  if (busy || !batch) return;
  const indices = batch.entries
    .map((entry, i) => (entry.status === "error" || entry.status === "cancelled" ? i : -1))
    .filter((i) => i >= 0);
  if (indices.length === 0) return;

  setBusy(true);
  batchRetryBtn.hidden = true;
  batchZipBtn.hidden = true;
  for (const i of indices) resetEntryForRetry(batch.entries[i]);
  refreshBatchProgress();
  let completed = false;
  try {
    await ensureModelReady(selectedModelKey);
    await runBatch(indices);
    completed = true;
  } catch (err) {
    setStatus(`Failed: ${err}`);
  } finally {
    setBusy(false);
    updateBatchButtons();
    if (completed) toastBatchOutcome();
  }
});

batchZipBtn.addEventListener("click", async () => {
  if (busy || !batch) return;
  const files = batch.entries.filter((e) => e.status === "done").map((e) => e.outputPath);
  if (files.length === 0) return;

  try {
    const dir = lastBatchOutputDir ?? batch.outputDir;
    const sep = dir && dir.includes("\\") && !dir.includes("/") ? "\\" : "/";
    const picked = await save({
      defaultPath: dir ? `${dir}${sep}unbagrnd-batch.zip` : "unbagrnd-batch.zip",
      filters: [{ name: "ZIP archive", extensions: ["zip"] }],
    });
    if (!picked) return;

    setBusy(true);
    setStatus("Creating ZIP…");
    const zipPath = await invoke("export_batch_zip", { files, zipPath: picked });
    setStatus(`Saved ZIP to ${zipPath}`);
    toastSaved("ZIP saved", zipPath);
  } catch (err) {
    setStatus(`Failed: ${err}`);
  } finally {
    setBusy(false);
    updateBatchButtons();
  }
});

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

function activeDropzone() {
  // Only one mode is visible at a time, so route the native file drop to that
  // mode directly. Native backends do not report positions in consistent
  // coordinate units (notably Cocoa points on macOS), making hit-testing here
  // unreliable on Retina displays.
  // Modes are switched by hiding the whole tab panel, not the dropzone itself,
  // so both the zone and its panel have to be visible.
  return (
    dropzones.find(
      (zone) => !zone.el.hidden && !zone.el.closest('[role="tabpanel"]').hidden,
    ) ?? null
  );
}

async function setupDragAndDrop() {
  const webview = getCurrentWebview();
  await webview.onDragDropEvent((event) => {
    const payload = event.payload;
    if (payload.type === "enter" || payload.type === "over") {
      const zone = activeDropzone();
      for (const z of dropzones) z.highlight(z === zone);
    } else if (payload.type === "drop") {
      for (const z of dropzones) z.highlight(false);
      const zone = activeDropzone();
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
  // Read straight from the running build (tauri.conf.json's `version` at
  // build time) rather than a hardcoded string in index.html, which was
  // left stale through several releases before this.
  getVersion()
    .then((version) => {
      $("app-version").textContent = version;
    })
    .catch(() => {});

  activateTab("single");
  buildBatchSwatches();
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
