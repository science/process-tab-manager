// Process Tab Manager — Tauri v2 frontend
// Communicates with Rust backend via __TAURI__.core.invoke()
// Receives sidebar updates via __TAURI__.event.listen()

const sidebar = document.getElementById("sidebar");
const contextMenu = document.getElementById("context-menu");

let items = [];
let selectedWid = null;
let selectedGid = null;
let renameTarget = null; // { type: "window", wid } or { type: "group", gid }
let isDragging = false;
let pendingItems = null;

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

// ─── XI2 click bridge ────────────────────────────────────────────
// XI2 is the primary mouse input source for sidebar rows.
// Native click/pointermove/pointerup on sidebar serve as fallbacks
// for when WebKitGTK grabs the pointer (PTM focused) and XI2
// ButtonRelease/Motion events don't arrive.

let lastClickAction = 0; // Dedup timestamp: prevents double-fire from XI2 + native
let recentDragEnd = 0;   // Prevents click-after-drag from native click event

window.__ptm_xi2_click = function(viewportX, viewportY, button) {
  const el = document.elementFromPoint(viewportX, viewportY);

  // Dismiss context menu on any left-click outside the menu
  if (button === 1 && contextMenu.classList.contains("visible")) {
    if (!el || !contextMenu.contains(el)) {
      contextMenu.classList.remove("visible");
    }
  }

  if (!el) return;
  if (el.closest(".rename-input")) return; // Don't interfere with rename

  const row = el.closest(".row");
  const header = el.closest(".group-header");
  if (!row && !header) return;

  if (button === 1) {
    // Deduplicate: XI2 release and native click can both call this
    const now = Date.now();
    if (now - recentDragEnd < 300) return; // Skip click right after drag
    if (now - lastClickAction < 300) return; // Skip duplicate click
    lastClickAction = now;

    if (row) {
      const wid = parseInt(row.dataset.wid);
      selectedWid = wid;
      selectedGid = null;
      logEvent("click", `wid=${wid}`);
      highlightSelected();
      writeTestState();
      invoke("activate_window", { wid });
    } else if (header) {
      const gid = header.dataset.gid;
      selectedGid = gid;
      selectedWid = null;
      logEvent("click-group", `gid=${gid}`);
      invoke("toggle_group", { gid });
      refreshSidebar();
      writeTestState();
    }
  } else if (button === 3) {
    const index = parseInt((row || header).dataset.index);
    const item = items[index];
    if (!item) return;
    if (row) {
      selectedWid = item.wid;
      selectedGid = item.gid || null;
      logEvent("contextmenu", `wid=${item.wid}`);
    } else {
      selectedGid = item.gid;
      selectedWid = null;
      logEvent("contextmenu-group", `gid=${item.gid}`);
    }
    showContextMenu(viewportX, viewportY, item);
    highlightSelected();
    writeTestState();
  }
};

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

  return row;
}

function renderGroupHeader(item, index, preservedRenameValue) {
  const header = document.createElement("div");
  header.className = "group-header";
  header.dataset.gid = item.gid;
  header.dataset.index = index;

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

// Suppress native browser context menu (XI2 handles right-click)
document.addEventListener("contextmenu", (e) => {
  e.preventDefault();
});

// Native click fallback — handles clicks when PTM has focus and
// XI2 ButtonRelease doesn't arrive (WebKitGTK grabs the pointer).
// Dedup in __ptm_xi2_click prevents double-fire when XI2 also works.
sidebar.addEventListener("click", (e) => {
  const row = e.target.closest(".row");
  const header = e.target.closest(".group-header");
  if (!row && !header) return;
  window.__ptm_xi2_click(e.clientX, e.clientY, 1);
});

// ─── Drag and drop (XI2 primary, native fallback) ────────────────

let dragState = null; // { sourceIndex, startY, started }
const DRAG_THRESHOLD = 5;

function findDropTarget(x, y) {
  const el = document.elementFromPoint(x, y);
  const row = el?.closest(".row, .group-header");
  if (row) return row;
  // If pointer is below all rows, target the last row
  const rows = sidebar.querySelectorAll(".row, .group-header");
  if (rows.length > 0) {
    const lastRow = rows[rows.length - 1];
    const lastRect = lastRow.getBoundingClientRect();
    if (y > lastRect.bottom) return lastRow;
  }
  return null;
}

function clearDropHighlight() {
  sidebar.querySelectorAll(".drop-before, .drop-after").forEach(el =>
    el.classList.remove("drop-before", "drop-after"));
}

function updateDragVisual(x, y) {
  if (!dragState?.started) return;
  clearDropHighlight();
  const row = findDropTarget(x, y);
  if (row) {
    const idx = parseInt(row.dataset.index);
    if (idx !== dragState.sourceIndex) {
      // Show insertion line based on pointer position relative to row midpoint
      const rect = row.getBoundingClientRect();
      const inTopHalf = y < rect.top + rect.height / 2;
      row.classList.add(inTopHalf ? "drop-before" : "drop-after");
    }
  }
}

async function completeDrop(x, y) {
  clearDropHighlight();
  // Remove .dragging from source
  const allRows = sidebar.querySelectorAll(".row, .group-header");
  const srcRow = allRows[dragState.sourceIndex];
  if (srcRow) srcRow.classList.remove("dragging");

  const row = findDropTarget(x, y);
  if (row) {
    const targetIndex = parseInt(row.dataset.index);
    if (targetIndex !== dragState.sourceIndex) {
      // Group header drop: add to group
      if (row.classList.contains("group-header") && row.dataset.gid
          && srcRow?.classList.contains("row") && srcRow.dataset.wid) {
        await invoke("add_to_group", {
          wid: parseInt(srcRow.dataset.wid), gid: row.dataset.gid });
        logEvent("add-to-group", `wid=${srcRow.dataset.wid} gid=${row.dataset.gid}`);
      } else {
        await invoke("reorder", { from: dragState.sourceIndex, to: targetIndex });
        logEvent("reorder", `from=${dragState.sourceIndex} to=${targetIndex}`);
      }
      pendingItems = null;
      await refreshSidebar();
    }
  }
  isDragging = false;
  dragState = null;
  recentDragEnd = Date.now();
}

// ─── XI2 handler functions ───────────────────────────────────────

function handleXi2Press(x, y, button) {
  if (button === 1) {
    const target = document.elementFromPoint(x, y);
    const row = target?.closest(".row, .group-header");
    if (row) {
      dragState = { sourceIndex: parseInt(row.dataset.index), startY: y, started: false };
    }
  } else if (button === 3) {
    window.__ptm_xi2_click(x, y, button);
  }
}

function handleXi2Move(x, y) {
  if (!dragState) return;
  if (!dragState.started) {
    if (Math.abs(y - dragState.startY) < DRAG_THRESHOLD) return;
    dragState.started = true;
    isDragging = true;
    const rows = sidebar.querySelectorAll(".row, .group-header");
    if (rows[dragState.sourceIndex]) rows[dragState.sourceIndex].classList.add("dragging");
    logEvent("xi2-drag-start", `index=${dragState.sourceIndex}`);
  }
  updateDragVisual(x, y);
}

async function handleXi2Release(x, y) {
  if (!dragState) return;
  if (!dragState.started) {
    dragState = null;
    window.__ptm_xi2_click(x, y, 1);
    return;
  }
  await completeDrop(x, y);
}

function handleXi2Cancel() {
  if (dragState?.started) {
    clearDropHighlight();
    const rows = sidebar.querySelectorAll(".row, .group-header");
    if (rows[dragState.sourceIndex]) rows[dragState.sourceIndex].classList.remove("dragging");
    isDragging = false;
  }
  dragState = null;
}

// Test bridges
window.__ptm_xi2_press = handleXi2Press;
window.__ptm_xi2_move = handleXi2Move;
window.__ptm_xi2_release = handleXi2Release;
window.__ptm_xi2_cancel = handleXi2Cancel;

// Native pointer event fallbacks for drag — handles the case where
// WebKitGTK grabs the pointer and XI2 Motion/ButtonRelease don't arrive.
// XI2 press already set dragState; these track motion and completion.
sidebar.addEventListener("pointermove", (e) => {
  if (!dragState) return;
  if (!dragState.started) {
    if (Math.abs(e.clientY - dragState.startY) < DRAG_THRESHOLD) return;
    dragState.started = true;
    isDragging = true;
    const rows = sidebar.querySelectorAll(".row, .group-header");
    if (rows[dragState.sourceIndex]) rows[dragState.sourceIndex].classList.add("dragging");
    logEvent("drag-start", `index=${dragState.sourceIndex}`);
  }
  updateDragVisual(e.clientX, e.clientY);
});

sidebar.addEventListener("pointerup", async (e) => {
  if (!dragState) return;
  if (!dragState.started) { dragState = null; return; }
  await completeDrop(e.clientX, e.clientY);
});

sidebar.addEventListener("pointercancel", () => {
  handleXi2Cancel();
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

// ─── Initialization ─────────────────────────────────────────────

async function init() {
  // Listen for backend updates (X11 window changes) — must await to ensure registration
  await listen("sidebar-update", (event) => {
    if (isDragging) {
      pendingItems = event.payload;
      return;
    }
    items = event.payload;
    render();
    writeTestState();
  });

  // Listen for XI2 click events from Rust backend
  await listen("x11-click", (event) => {
    const { x, y, button, root_x, root_y, event_wid, registered_wid } = event.payload;
    logEvent("x11-click", `x=${x} y=${y} root=(${root_x},${root_y}) event_wid=0x${event_wid.toString(16)} registered=0x${registered_wid.toString(16)} btn=${button}`);
    handleXi2Press(x, y, button);
  });

  // Listen for XI2 drag motion events
  await listen("x11-drag-move", (event) => {
    handleXi2Move(event.payload.x, event.payload.y);
  });

  // Listen for XI2 drag end events (ButtonRelease)
  await listen("x11-drag-end", async (event) => {
    await handleXi2Release(event.payload.x, event.payload.y);
  });

  // Initial load
  await refreshSidebar();

  logEvent("init", "PTM frontend loaded");
}

init();
