// Process Tab Manager — Tauri v2 frontend
// Communicates with Rust backend via __TAURI__.core.invoke()
// Receives sidebar updates via __TAURI__.event.listen()

const sidebar = document.getElementById("sidebar");
const contextMenu = document.getElementById("context-menu");

let items = [];
let selectedWid = null;
let selectedGid = null;
let renameTarget = null; // { type: "window", wid } or { type: "group", gid }

// ─── Tauri IPC ──────────────────────────────────────────────────

async function invoke(cmd, args) {
  if (window.__TAURI__ && window.__TAURI__.core) {
    return window.__TAURI__.core.invoke(cmd, args || {});
  }
}

async function listen(event, handler) {
  if (window.__TAURI__ && window.__TAURI__.event) {
    return window.__TAURI__.event.listen(event, handler);
  }
}

// E2E test instrumentation
async function logEvent(type, detail) {
  const ts = new Date().toISOString().slice(11, 23);
  invoke("log_event", { line: `${ts} ${type} ${detail}` });
}

async function writeTestState() {
  const state = {
    items: items.map(i => {
      if (i.kind === "GroupHeader") return { kind: "group", gid: i.gid, name: i.name };
      return { kind: "window", wid: i.wid, title: i.title };
    }),
    selectedWid,
    selectedGid,
    timestamp: new Date().toISOString(),
  };
  invoke("write_test_state", { json: JSON.stringify(state, null, 2) });
}

// ─── Render ─────────────────────────────────────────────────────

function render() {
  // Preserve rename input value across re-renders (sidebar re-renders every ~1s)
  const existingInput = sidebar.querySelector(".rename-input");
  const preservedRenameValue = existingInput ? existingInput.value : null;

  sidebar.innerHTML = "";

  items.forEach((item, index) => {
    if (item.kind === "GroupHeader") {
      sidebar.appendChild(renderGroupHeader(item, index, preservedRenameValue));
    } else {
      sidebar.appendChild(renderWindowRow(item, index, preservedRenameValue));
    }
  });
}

function renderWindowRow(item, index, preservedRenameValue) {
  const row = document.createElement("div");
  row.className = "row";
  row.dataset.wid = item.wid;
  row.dataset.index = index;
  row.draggable = true;

  if (item.is_active) row.classList.add("active");
  if (item.is_minimized) row.classList.add("minimized");
  if (item.is_urgent) row.classList.add("urgent");
  if (item.is_renamed) row.classList.add("renamed");
  if (item.wid === selectedWid) row.classList.add("selected");
  if (item.kind === "GroupedWindow") row.classList.add("grouped");

  // Icon
  const icon = document.createElement("img");
  icon.className = "icon";
  if (item.icon_path) {
    icon.src = convertFileSrc(item.icon_path);
    icon.onerror = () => { icon.style.display = "none"; };
  } else {
    icon.style.display = "none";
  }
  row.appendChild(icon);

  // Title (or rename input)
  if (renameTarget && renameTarget.type === "window" && renameTarget.wid === item.wid) {
    const displayValue = preservedRenameValue !== null ? preservedRenameValue : item.title;
    const input = createRenameInput(displayValue, (newName) => {
      if (newName && newName !== item.title) {
        invoke("rename_window", { wid: item.wid, name: newName });
        logEvent("rename", `wid=${item.wid} name=${newName}`);
      }
      renameTarget = null;
      refreshSidebar();
    });
    row.appendChild(input);
  } else {
    const title = document.createElement("span");
    title.className = "title";
    title.textContent = item.title;
    row.appendChild(title);
  }

  // Click: select + activate
  row.addEventListener("click", (e) => {
    if (e.button !== 0) return;
    selectedWid = item.wid;
    selectedGid = null;
    logEvent("click", `wid=${item.wid} isTrusted=${e.isTrusted}`);
    highlightSelected();
    writeTestState();
    invoke("activate_window", { wid: item.wid });
  });

  // Right-click: context menu
  row.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    selectedWid = item.wid;
    selectedGid = item.gid || null;
    logEvent("contextmenu", `wid=${item.wid} isTrusted=${e.isTrusted}`);
    showContextMenu(e.clientX, e.clientY, item);
    highlightSelected();
    writeTestState();
  });

  // DnD
  row.addEventListener("dragstart", (e) => {
    e.dataTransfer.effectAllowed = "move";
    e.dataTransfer.setData("text/plain", JSON.stringify({ type: "window", wid: item.wid, index }));
    row.classList.add("dragging");
    logEvent("dragstart", `wid=${item.wid}`);
  });

  row.addEventListener("dragend", () => {
    row.classList.remove("dragging");
    logEvent("dragend", `wid=${item.wid}`);
  });

  return row;
}

function renderGroupHeader(item, index, preservedRenameValue) {
  const header = document.createElement("div");
  header.className = "group-header";
  header.dataset.gid = item.gid;
  header.dataset.index = index;
  header.draggable = true;

  if (item.gid === selectedGid) header.classList.add("selected");

  // Collapse arrow
  const arrow = document.createElement("span");
  arrow.className = "group-arrow";
  arrow.textContent = item.collapsed ? "\u25B6" : "\u25BC";
  header.appendChild(arrow);

  // Name (or rename input)
  if (renameTarget && renameTarget.type === "group" && renameTarget.gid === item.gid) {
    const displayValue = preservedRenameValue !== null ? preservedRenameValue : item.name;
    const input = createRenameInput(displayValue, (newName) => {
      if (newName && newName !== item.name) {
        invoke("rename_group", { gid: item.gid, name: newName });
        logEvent("rename-group", `gid=${item.gid} name=${newName}`);
      }
      renameTarget = null;
      refreshSidebar();
    });
    header.appendChild(input);
  } else {
    const name = document.createElement("span");
    name.className = "group-name";
    name.textContent = item.name;
    header.appendChild(name);
  }

  // Member count
  const count = document.createElement("span");
  count.className = "group-count";
  count.textContent = `(${item.member_count})`;
  header.appendChild(count);

  // Click: toggle collapse
  header.addEventListener("click", (e) => {
    selectedGid = item.gid;
    selectedWid = null;
    logEvent("click-group", `gid=${item.gid} isTrusted=${e.isTrusted}`);
    invoke("toggle_group", { gid: item.gid });
    refreshSidebar();
    writeTestState();
  });

  // Right-click: context menu
  header.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    selectedGid = item.gid;
    selectedWid = null;
    logEvent("contextmenu-group", `gid=${item.gid} isTrusted=${e.isTrusted}`);
    showContextMenu(e.clientX, e.clientY, item);
    highlightSelected();
    writeTestState();
  });

  // DnD
  header.addEventListener("dragstart", (e) => {
    e.dataTransfer.effectAllowed = "move";
    e.dataTransfer.setData("text/plain", JSON.stringify({ type: "group", gid: item.gid, index }));
    header.classList.add("dragging");
    logEvent("dragstart-group", `gid=${item.gid}`);
  });

  header.addEventListener("dragend", () => {
    header.classList.remove("dragging");
    logEvent("dragend-group", `gid=${item.gid}`);
  });

  return header;
}

// ─── Inline rename ──────────────────────────────────────────────

function createRenameInput(currentValue, onCommit) {
  const input = document.createElement("input");
  input.type = "text";
  input.className = "rename-input";
  input.value = currentValue;

  const commit = () => {
    const val = input.value.trim();
    onCommit(val);
  };

  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") { e.preventDefault(); commit(); }
    if (e.key === "Escape") { renameTarget = null; refreshSidebar(); }
    e.stopPropagation(); // Don't trigger sidebar keyboard shortcuts
  });

  input.addEventListener("blur", commit);

  // Auto-focus after append
  requestAnimationFrame(() => {
    input.focus();
    input.select();
  });

  return input;
}

// ─── Selection highlight ────────────────────────────────────────

function highlightSelected() {
  document.querySelectorAll(".row.selected, .group-header.selected").forEach(el => {
    el.classList.remove("selected");
  });

  if (selectedWid !== null) {
    const el = sidebar.querySelector(`[data-wid="${selectedWid}"]`);
    if (el) el.classList.add("selected");
  }
  if (selectedGid !== null) {
    const el = sidebar.querySelector(`[data-gid="${selectedGid}"]`);
    if (el) el.classList.add("selected");
  }
}

// ─── Context menu ───────────────────────────────────────────────

function showContextMenu(x, y, item) {
  contextMenu.innerHTML = "";

  if (item.kind === "GroupHeader") {
    addMenuItem("Rename Group", async () => {
      renameTarget = { type: "group", gid: item.gid };
      await refreshSidebar();
    });
    addMenuItem("Delete Group", async () => {
      await invoke("delete_group", { gid: item.gid });
      logEvent("delete-group", `gid=${item.gid}`);
      await refreshSidebar();
    });
  } else {
    addMenuItem("Rename", async () => {
      renameTarget = { type: "window", wid: item.wid };
      await refreshSidebar();
    });
    if (item.is_renamed) {
      addMenuItem("Clear Rename", async () => {
        await invoke("clear_rename", { wid: item.wid });
        logEvent("clear-rename", `wid=${item.wid}`);
        await refreshSidebar();
      });
    }
    addMenuSeparator();
    addMenuItem("Close Window", async () => {
      await invoke("close_window", { wid: item.wid });
      logEvent("close-window", `wid=${item.wid}`);
    });
    addMenuItem("Remove from List", async () => {
      await invoke("hide_window", { wid: item.wid });
      logEvent("hide-window", `wid=${item.wid}`);
      await refreshSidebar();
    });
    addMenuSeparator();
    addMenuItem("Create Group", async () => {
      await invoke("create_group", { name: "New Group", wid: item.wid });
      logEvent("create-group", `wid=${item.wid}`);
      await refreshSidebar();
    });
    if (item.kind === "GroupedWindow") {
      addMenuItem("Remove from Group", async () => {
        await invoke("remove_from_group", { wid: item.wid });
        logEvent("remove-from-group", `wid=${item.wid}`);
        await refreshSidebar();
      });
    }
  }

  // Render off-screen to measure, then clamp within viewport
  contextMenu.style.left = "-9999px";
  contextMenu.style.top = "-9999px";
  contextMenu.classList.add("visible");

  const rect = contextMenu.getBoundingClientRect();
  const clampedX = Math.min(x, window.innerWidth - rect.width - 4);
  const clampedY = Math.min(y, window.innerHeight - rect.height - 4);
  contextMenu.style.left = Math.max(0, clampedX) + "px";
  contextMenu.style.top = Math.max(0, clampedY) + "px";
}

function addMenuItem(label, onClick) {
  const item = document.createElement("div");
  item.className = "menu-item";
  item.textContent = label;
  item.addEventListener("click", async (e) => {
    e.stopPropagation();
    contextMenu.classList.remove("visible");
    await onClick();
  });
  contextMenu.appendChild(item);
}

function addMenuSeparator() {
  const sep = document.createElement("div");
  sep.className = "menu-separator";
  contextMenu.appendChild(sep);
}

// Close context menu on click elsewhere
document.addEventListener("click", (e) => {
  if (!contextMenu.contains(e.target)) {
    contextMenu.classList.remove("visible");
  }
});

// ─── Drag and drop ──────────────────────────────────────────────

let dropIndicator = null;

function getDropTargetIndex(clientY) {
  const rows = sidebar.querySelectorAll(".row, .group-header");
  for (let i = 0; i < rows.length; i++) {
    const rect = rows[i].getBoundingClientRect();
    const midpoint = rect.top + rect.height / 2;
    if (clientY < midpoint) return i;
  }
  return rows.length;
}

function showDropIndicator(index) {
  removeDropIndicator();
  dropIndicator = document.createElement("div");
  dropIndicator.className = "drop-indicator";
  const rows = sidebar.querySelectorAll(".row, .group-header");
  if (index < rows.length) {
    sidebar.insertBefore(dropIndicator, rows[index]);
  } else {
    sidebar.appendChild(dropIndicator);
  }
}

function removeDropIndicator() {
  if (dropIndicator && dropIndicator.parentNode) {
    dropIndicator.parentNode.removeChild(dropIndicator);
  }
  dropIndicator = null;
}

sidebar.addEventListener("dragover", (e) => {
  e.preventDefault();
  e.dataTransfer.dropEffect = "move";
  const targetIndex = getDropTargetIndex(e.clientY);
  showDropIndicator(targetIndex);
});

sidebar.addEventListener("dragleave", (e) => {
  // Only remove if leaving the sidebar entirely
  if (!sidebar.contains(e.relatedTarget)) {
    removeDropIndicator();
  }
});

sidebar.addEventListener("drop", (e) => {
  e.preventDefault();
  removeDropIndicator();

  let source;
  try {
    source = JSON.parse(e.dataTransfer.getData("text/plain"));
  } catch { return; }

  const targetIndex = getDropTargetIndex(e.clientY);

  // Check if dropping onto a group header
  const rows = sidebar.querySelectorAll(".row, .group-header");
  let targetItem = null;
  if (targetIndex < rows.length) {
    const targetRow = rows[targetIndex];
    if (targetRow.classList.contains("group-header") && targetRow.dataset.gid) {
      targetItem = items[parseInt(targetRow.dataset.index)];
    }
  }

  logEvent("drop", `source=${JSON.stringify(source)} targetIndex=${targetIndex}`);

  if (source.type === "window" && targetItem && targetItem.kind === "GroupHeader") {
    invoke("add_to_group", { wid: source.wid, gid: targetItem.gid });
    logEvent("add-to-group", `wid=${source.wid} gid=${targetItem.gid}`);
  } else {
    invoke("reorder", { from: source.index, to: targetIndex });
    logEvent("reorder", `from=${source.index} to=${targetIndex}`);
  }

  refreshSidebar();
});

document.addEventListener("dragend", () => {
  removeDropIndicator();
});

// ─── Keyboard shortcuts ─────────────────────────────────────────

document.addEventListener("keydown", (e) => {
  // Don't handle keys when renaming
  if (renameTarget) return;

  logEvent("keydown", `key=${e.key} code=${e.code} ctrl=${e.ctrlKey} shift=${e.shiftKey} alt=${e.altKey} isTrusted=${e.isTrusted}`);

  if (e.key === "F2") {
    e.preventDefault();
    if (selectedWid) {
      renameTarget = { type: "window", wid: selectedWid };
      logEvent("f2-rename", `wid=${selectedWid}`);
      refreshSidebar();
    } else if (selectedGid) {
      renameTarget = { type: "group", gid: selectedGid };
      logEvent("f2-rename-group", `gid=${selectedGid}`);
      refreshSidebar();
    }
  }

  if (e.key === "Delete") {
    if (selectedWid) {
      invoke("hide_window", { wid: selectedWid });
      logEvent("delete-hide", `wid=${selectedWid}`);
      refreshSidebar();
    } else if (selectedGid) {
      invoke("delete_group", { gid: selectedGid });
      logEvent("delete-group", `gid=${selectedGid}`);
      refreshSidebar();
    }
  }

  if (e.key === "Enter") {
    if (selectedWid) {
      invoke("activate_window", { wid: selectedWid });
      logEvent("enter-activate", `wid=${selectedWid}`);
    }
  }

  // Ctrl+Shift+Up / Ctrl+Shift+Down: reorder
  if (e.ctrlKey && e.shiftKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
    e.preventDefault();
    const currentIndex = items.findIndex(i =>
      (selectedWid && i.wid === selectedWid) || (selectedGid && i.gid === selectedGid)
    );
    if (currentIndex === -1) return;
    const newIndex = e.key === "ArrowUp" ? currentIndex - 1 : currentIndex + 1;
    if (newIndex >= 0 && newIndex < items.length) {
      invoke("reorder", { from: currentIndex, to: newIndex });
      logEvent("keyboard-reorder", `from=${currentIndex} to=${newIndex}`);
      refreshSidebar();
    }
  }

  // Arrow Up/Down: navigate
  if (!e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
    e.preventDefault();
    const currentIndex = items.findIndex(i =>
      (selectedWid && i.wid === selectedWid) || (selectedGid && i.gid === selectedGid)
    );
    let newIndex;
    if (currentIndex === -1) {
      newIndex = 0;
    } else {
      newIndex = e.key === "ArrowUp" ? currentIndex - 1 : currentIndex + 1;
    }
    if (newIndex >= 0 && newIndex < items.length) {
      const newItem = items[newIndex];
      if (newItem.kind === "GroupHeader") {
        selectedGid = newItem.gid;
        selectedWid = null;
      } else {
        selectedWid = newItem.wid;
        selectedGid = null;
      }
      highlightSelected();
      writeTestState();
    }
  }
});

// ─── Tauri asset helper ─────────────────────────────────────────

function convertFileSrc(path) {
  if (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.convertFileSrc) {
    return window.__TAURI__.core.convertFileSrc(path);
  }
  // Fallback for Tauri v2 asset protocol
  return "asset://localhost/" + encodeURI(path);
}

// ─── Data fetching ──────────────────────────────────────────────

async function refreshSidebar() {
  const newItems = await invoke("get_sidebar_items");
  if (newItems) {
    items = newItems;
    render();
    writeTestState();
  }
}

// ─── Focus-click workaround ─────────────────────────────────────
// On X11/GTK, when another window has focus and the user clicks on PTM,
// GTK absorbs the first click to regain window focus without delivering
// it as a DOM event. This workaround replays the click when the window
// regains focus, so every click on a sidebar row is actionable.

{
  let windowBlurred = false;

  window.addEventListener("blur", () => {
    windowBlurred = true;
  });

  window.addEventListener("focus", () => {
    setTimeout(() => { windowBlurred = false; }, 300);
  });

  // On mousedown while blurred: GTK delivered the mousedown but won't follow
  // through with a click event. Find the row at the click position and click it.
  document.addEventListener("mousedown", (e) => {
    if (e.button !== 0 || !windowBlurred) return;
    windowBlurred = false;
    const clickY = e.clientY;
    setTimeout(() => {
      const rows = document.querySelectorAll(".row, .group-header");
      let closest = null;
      let closestDist = Infinity;
      rows.forEach(row => {
        const rect = row.getBoundingClientRect();
        const rowMidY = rect.top + rect.height / 2;
        const dist = Math.abs(clickY - rowMidY);
        if (dist < closestDist) {
          closestDist = dist;
          closest = row;
        }
      });
      if (closest) {
        closest.click();
      }
    }, 50);
  }, true);
}

// ─── Initialization ─────────────────────────────────────────────

async function init() {
  // Listen for backend updates (X11 window changes) — must await to ensure registration
  await listen("sidebar-update", (event) => {
    items = event.payload;
    render();
    writeTestState();
  });

  // Initial load
  await refreshSidebar();

  logEvent("init", "PTM frontend loaded");
}

init();
