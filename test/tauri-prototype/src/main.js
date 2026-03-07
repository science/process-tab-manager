// Interaction test prototype — logs every event for E2E verification
// Events are written to /tmp/ptm-events.log via Tauri command
// State snapshots written to /tmp/ptm-test-state.json

const items = [
  { id: 1, name: "Firefox", active: true },
  { id: 2, name: "Terminal", active: false },
  { id: 3, name: "VS Code", active: false },
  { id: 4, name: "Slack", active: false },
  { id: 5, name: "Files", active: false },
];

const log = document.getElementById("event-log");
const list = document.getElementById("window-list");
const menu = document.getElementById("context-menu");

let selectedId = null;
let dragSourceId = null;

// Tauri invoke — use __TAURI__ global (no bundler needed)
async function invoke(cmd, args) {
  if (window.__TAURI__ && window.__TAURI__.core) {
    return window.__TAURI__.core.invoke(cmd, args);
  }
}

function logEvent(type, detail) {
  const ts = new Date().toISOString().slice(11, 23);
  const line = `${ts} ${type} ${detail}`;

  // Write to DOM log
  const entry = document.createElement("div");
  entry.className = `entry ${type}`;
  entry.textContent = `[${line}]`;
  log.insertBefore(entry, log.firstChild);

  // Write to file via Tauri command
  invoke("log_event", { line });

  // Console for debugging
  console.log(`EVENT:${type}:${detail}`);
}

function writeState() {
  const state = {
    items: items.map((i) => ({ id: i.id, name: i.name })),
    selectedId,
    timestamp: new Date().toISOString(),
  };
  invoke("write_test_state", { json: JSON.stringify(state, null, 2) });
}

function render() {
  list.innerHTML = "";
  items.forEach((item) => {
    const li = document.createElement("li");
    li.textContent = item.name;
    li.dataset.id = item.id;
    li.draggable = true;
    if (item.active) li.classList.add("active");
    if (item.id === selectedId) li.classList.add("selected");

    // Single click
    li.addEventListener("click", (e) => {
      selectedId = item.id;
      logEvent("click", `id=${item.id} name=${item.name} button=${e.button} isTrusted=${e.isTrusted}`);
      writeState();
      render();
    });

    // Double click
    li.addEventListener("dblclick", (e) => {
      logEvent("dblclick", `id=${item.id} name=${item.name} isTrusted=${e.isTrusted}`);
    });

    // Right-click / context menu
    li.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      selectedId = item.id;
      logEvent("contextmenu", `id=${item.id} name=${item.name} x=${e.clientX} y=${e.clientY} isTrusted=${e.isTrusted}`);
      menu.style.left = e.clientX + "px";
      menu.style.top = e.clientY + "px";
      menu.classList.add("visible");
      menu.dataset.targetId = item.id;
      writeState();
      render();
    });

    // Drag start
    li.addEventListener("dragstart", (e) => {
      dragSourceId = item.id;
      li.classList.add("dragging");
      e.dataTransfer.effectAllowed = "move";
      e.dataTransfer.setData("text/plain", String(item.id));
      logEvent("dragstart", `id=${item.id} name=${item.name} isTrusted=${e.isTrusted}`);
    });

    // Drag over (allow drop)
    li.addEventListener("dragover", (e) => {
      e.preventDefault();
      e.dataTransfer.dropEffect = "move";
      li.classList.add("drag-over");
    });

    li.addEventListener("dragleave", () => {
      li.classList.remove("drag-over");
    });

    // Drop
    li.addEventListener("drop", (e) => {
      e.preventDefault();
      li.classList.remove("drag-over");
      const sourceId = parseInt(e.dataTransfer.getData("text/plain"));
      const targetId = item.id;
      logEvent("drop", `source=${sourceId} target=${targetId} isTrusted=${e.isTrusted}`);

      // Reorder items
      const sourceIdx = items.findIndex((i) => i.id === sourceId);
      const targetIdx = items.findIndex((i) => i.id === targetId);
      if (sourceIdx !== -1 && targetIdx !== -1 && sourceIdx !== targetIdx) {
        const [moved] = items.splice(sourceIdx, 1);
        items.splice(targetIdx, 0, moved);
        logEvent("reorder", `moved id=${sourceId} from=${sourceIdx} to=${targetIdx}`);
        writeState();
        render();
      }
    });

    // Drag end
    li.addEventListener("dragend", (e) => {
      li.classList.remove("dragging");
      dragSourceId = null;
      logEvent("dragend", `id=${item.id} isTrusted=${e.isTrusted}`);
    });

    list.appendChild(li);
  });
}

// Context menu actions
menu.querySelectorAll("div[data-action]").forEach((menuItem) => {
  menuItem.addEventListener("click", (e) => {
    const action = menuItem.dataset.action;
    const targetId = menu.dataset.targetId;
    logEvent("menu-action", `action=${action} targetId=${targetId}`);
    menu.classList.remove("visible");
  });
});

// Close context menu on click elsewhere
document.addEventListener("click", (e) => {
  if (!menu.contains(e.target)) {
    menu.classList.remove("visible");
  }
});

// Keyboard: F2 for rename, Delete for remove
document.addEventListener("keydown", (e) => {
  logEvent("keydown", `key=${e.key} code=${e.code} isTrusted=${e.isTrusted}`);
  if (e.key === "F2" && selectedId) {
    logEvent("f2-rename", `selectedId=${selectedId}`);
  }
  if (e.key === "Delete" && selectedId) {
    logEvent("delete", `selectedId=${selectedId}`);
  }
});

// Log mouse events at the document level for completeness
document.addEventListener("mousedown", (e) => {
  logEvent("mousedown", `button=${e.button} x=${e.clientX} y=${e.clientY} isTrusted=${e.isTrusted}`);
});

document.addEventListener("mouseup", (e) => {
  logEvent("mouseup", `button=${e.button} x=${e.clientX} y=${e.clientY} isTrusted=${e.isTrusted}`);
});

logEvent("init", "Prototype loaded successfully");
writeState();
render();
