"use strict";

/**
 * Thin wrapper around the three Tauri commands exposed by src-tauri/src/main.rs.
 * Keeping this as its own object (rather than sprinkling `invoke(...)` calls
 * through the DOM code below) means the DOM layer never needs to know it's
 * talking to Tauri at all — swapping the transport later only touches this.
 */
const api = {
  invoke(cmd, args) {
    return window.__TAURI__.core.invoke(cmd, args);
  },
  listDrives() {
    return this.invoke("list_drives");
  },
  scanSummary(path, engine) {
    return this.invoke("scan_summary", { path, engine });
  },
  scanChildren(path, engine) {
    return this.invoke("scan_children", { path, engine });
  },
};

function formatBytes(bytes) {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / Math.pow(1024, exponent);
  return `${value.toFixed(exponent === 0 ? 0 : 1)} ${units[exponent]}`;
}

function currentEngine() {
  return document.getElementById("engine-select").value;
}

const screens = {
  drives: document.getElementById("drives-screen"),
  results: document.getElementById("results-screen"),
};

function showScreen(name) {
  for (const [key, el] of Object.entries(screens)) {
    el.classList.toggle("hidden", key !== name);
  }
}

const driveList = document.getElementById("drive-list");
const resultsPath = document.getElementById("results-path");
const resultsSummary = document.getElementById("results-summary");
const resultsStatus = document.getElementById("results-status");
const resultsBody = document.getElementById("results-body");

function setStatus(message, isError) {
  resultsStatus.textContent = message || "";
  resultsStatus.classList.toggle("error", Boolean(isError));
}

function renderDriveCard(drive) {
  const used = drive.total_bytes - drive.available_bytes;
  const usedPct = drive.total_bytes > 0 ? (used / drive.total_bytes) * 100 : 0;

  const card = document.createElement("button");
  card.className = "drive-card";
  card.innerHTML = `
    <div class="letter">${drive.mount_point}</div>
    <div class="fs">${drive.file_system}${drive.is_removable ? " · removable" : ""}</div>
    <div class="usage-bar"><i style="width:${usedPct.toFixed(1)}%"></i></div>
    <div class="sizes">
      <span>${formatBytes(used)} used</span>
      <span>${formatBytes(drive.total_bytes)} total</span>
    </div>
  `;
  card.addEventListener("click", () => openResults(drive.mount_point));
  return card;
}

async function loadDrives() {
  driveList.innerHTML = "<p>Loading drives…</p>";
  try {
    const drives = await api.listDrives();
    driveList.innerHTML = "";
    if (drives.length === 0) {
      driveList.innerHTML = "<p>No drives found.</p>";
      return;
    }
    for (const drive of drives) {
      driveList.appendChild(renderDriveCard(drive));
    }
  } catch (err) {
    driveList.innerHTML = `<p class="status error">Failed to list drives: ${err}</p>`;
  }
}

function renderChildRow(child, maxSize) {
  const pct = maxSize > 0 ? (child.allocated_size / maxSize) * 100 : 0;
  const tr = document.createElement("tr");
  tr.innerHTML = `
    <td>
      <div class="name-cell">
        <span class="kind-icon">${child.is_directory ? "📁" : "📄"}</span>
        <span>${child.name}</span>
      </div>
    </td>
    <td class="size-cell">
      <span class="size-bar" style="width:${pct.toFixed(1)}%"></span>
      <span>${formatBytes(child.allocated_size)}</span>
    </td>
    <td>${child.file_count.toLocaleString()}</td>
    <td>${child.dir_count.toLocaleString()}</td>
  `;
  return tr;
}

let currentPath = null;

async function openResults(path) {
  currentPath = path;
  resultsPath.textContent = path;
  resultsSummary.innerHTML = "";
  resultsBody.innerHTML = "";
  setStatus("Scanning…", false);
  showScreen("results");

  const engine = currentEngine();

  const [summaryResult, childrenResult] = await Promise.allSettled([
    api.scanSummary(path, engine),
    api.scanChildren(path, engine),
  ]);

  if (summaryResult.status === "fulfilled") {
    const s = summaryResult.value;
    resultsSummary.innerHTML = `
      <span><b>${formatBytes(s.allocated_size)}</b> allocated</span>
      <span><b>${formatBytes(s.logical_size)}</b> logical</span>
      <span><b>${s.file_count.toLocaleString()}</b> files</span>
      <span><b>${s.dir_count.toLocaleString()}</b> folders</span>
      <span><b>${s.elapsed_ms}</b> ms</span>
    `;
    if (s.note) setStatus(s.note, false);
  }

  if (childrenResult.status === "rejected") {
    setStatus(String(childrenResult.reason), true);
    return;
  }

  const { children, note } = childrenResult.value;
  if (summaryResult.status !== "fulfilled") {
    setStatus(String(summaryResult.reason), true);
  } else if (note) {
    setStatus(note, false);
  } else if (!children.length) {
    setStatus("No entries (empty, or nothing this engine could read).", false);
  } else {
    setStatus("", false);
  }

  const maxSize = children.reduce((max, c) => Math.max(max, c.allocated_size), 0);
  for (const child of children) {
    resultsBody.appendChild(renderChildRow(child, maxSize));
  }
}

document.getElementById("refresh-drives").addEventListener("click", loadDrives);
document.getElementById("back-to-drives").addEventListener("click", () => showScreen("drives"));
document.getElementById("rescan").addEventListener("click", () => {
  if (currentPath) openResults(currentPath);
});

showScreen("drives");
loadDrives();
