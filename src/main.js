import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

const IMAGE_FILTERS = [
  { name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "bmp", "tiff", "tif", "gif"] },
];

const $ = (id) => document.getElementById(id);

// ---- Shared elements ----
const statusLine = $("status-line");
const outputDirDisplay = $("output-dir-display");
const clearOutputDirBtn = $("clear-output-dir-btn");

// ---- Model banner ----
const modelBanner = $("model-banner");
const modelBannerTitle = $("model-banner-title");
const modelBannerDetail = $("model-banner-detail");
const modelBannerProgress = $("model-banner-progress");
const modelProgressFill = $("model-progress-fill");
const modelProgressLabel = $("model-progress-label");
const modelDownloadBtn = $("model-download-btn");

// ---- Tabs ----
const tabSingle = $("tab-single");
const tabBatch = $("tab-batch");
const panelSingle = $("panel-single");
const panelBatch = $("panel-batch");

// ---- Single mode ----
const singleDropzone = $("single-dropzone");
const singlePickBtn = $("single-pick-btn");
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
let modelReadyPromise = null;
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

// ---------------------------------------------------------------------
// Model download
// ---------------------------------------------------------------------

async function refreshModelBanner(status) {
  if (status.downloaded) {
    modelBanner.hidden = true;
    return;
  }
  modelBanner.hidden = false;
  modelBannerTitle.textContent = "One-time setup";
  modelBannerDetail.textContent =
    "Downloading the background-removal model (~170 MB), one time only. " +
    "It's cached on your device — no further downloads after this.";
}

/**
 * Ensures the model is downloaded, starting the download (and showing
 * progress) if it isn't. Safe to call repeatedly; concurrent callers share
 * the same in-flight download.
 */
function ensureModelReady() {
  if (!modelReadyPromise) {
    modelReadyPromise = downloadModel();
  }
  return modelReadyPromise;
}

async function downloadModel() {
  modelBanner.hidden = false;
  modelDownloadBtn.hidden = true;
  modelBannerProgress.dataset.active = "true";
  modelBannerTitle.textContent = "Downloading model…";

  const unlisten = await listen("model-download-progress", (event) => {
    const { downloadedBytes, totalBytes } = event.payload;
    const pct = totalBytes > 0 ? Math.min(100, (downloadedBytes / totalBytes) * 100) : 0;
    modelProgressFill.style.width = `${pct}%`;
    modelProgressLabel.textContent = `${formatBytes(downloadedBytes)} / ${formatBytes(totalBytes)}`;
  });

  try {
    const status = await invoke("download_model");
    modelBanner.hidden = true;
    return status;
  } catch (err) {
    modelBannerTitle.textContent = "Model download failed";
    modelBannerDetail.textContent = String(err);
    modelDownloadBtn.hidden = false;
    modelReadyPromise = null; // allow retrying
    throw err;
  } finally {
    unlisten();
    modelBannerProgress.dataset.active = "false";
  }
}

modelDownloadBtn.addEventListener("click", () => {
  ensureModelReady().catch(() => {});
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

async function processSingle(path) {
  if (busy) return;
  setBusy(true);
  try {
    setStatus("Preparing…");
    await ensureModelReady();

    setStatus("Removing background…");
    const result = await invoke("remove_background_single", {
      inputPath: path,
      outputDir,
    });

    previewBefore.src = result.beforeDataUrl;
    previewAfter.src = result.afterDataUrl;
    singleOutputPathEl.textContent = result.outputPath;
    lastSingleOutputPath = result.outputPath;

    singleDropzone.hidden = true;
    singleResult.hidden = false;
    setStatus("Done.");
  } catch (err) {
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
  if (lastBatchOutputDir) {
    revealItemInDir(lastBatchOutputDir).catch((err) => setStatus(String(err)));
  }
});

function renderBatchRow(fileName) {
  const li = document.createElement("li");
  li.dataset.file = fileName;
  const nameEl = document.createElement("span");
  nameEl.className = "file-name";
  nameEl.textContent = fileName;
  const statusEl = document.createElement("span");
  statusEl.className = "file-status pending";
  statusEl.textContent = "Waiting…";
  li.append(nameEl, statusEl);
  batchFileList.appendChild(li);
  return statusEl;
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
    const { index, total, fileName, status, outputPath, message } = event.payload;

    let statusEl = rowsByIndex[index];
    if (!statusEl) {
      statusEl = renderBatchRow(fileName);
      rowsByIndex[index] = statusEl;
    }

    if (status === "done") {
      statusEl.textContent = "Done";
      statusEl.className = "file-status done";
      lastBatchOutputDir = parentDir(outputPath);
    } else {
      statusEl.textContent = message ?? "Failed";
      statusEl.className = "file-status error";
      statusEl.title = message ?? "";
    }

    const done = index + 1;
    const pct = total > 0 ? (done / total) * 100 : 0;
    batchProgressFill.style.width = `${pct}%`;
    batchProgressLabel.textContent = `${done} / ${total}`;
  });

  try {
    setStatus("Preparing…");
    await ensureModelReady();

    setStatus("Removing backgrounds…");
    await invoke("remove_background_batch", { inputPaths: paths, outputDir });
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
// Startup
// ---------------------------------------------------------------------

async function init() {
  activateTab("single");
  refreshOutputDirDisplay();

  try {
    const status = await invoke("model_status");
    await refreshModelBanner(status);
    if (!status.downloaded) {
      ensureModelReady().catch(() => {});
    }
  } catch (err) {
    setStatus(`Could not check model status: ${err}`);
  }

  await setupDragAndDrop();
  setStatus("Ready.");
}

init();
