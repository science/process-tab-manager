use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;

#[derive(Serialize)]
struct WindowInfo {
    id: u32,
    name: String,
    active: bool,
}

#[tauri::command]
fn get_windows() -> Vec<WindowInfo> {
    vec![
        WindowInfo { id: 1, name: "Firefox".into(), active: true },
        WindowInfo { id: 2, name: "Terminal".into(), active: false },
        WindowInfo { id: 3, name: "VS Code".into(), active: false },
    ]
}

#[tauri::command]
fn activate_window(wid: u32) -> String {
    format!("Activated window {}", wid)
}

/// Append a line to /tmp/ptm-events.log for E2E test verification
#[tauri::command]
fn log_event(line: String) {
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/ptm-events.log")
    {
        let _ = writeln!(f, "{}", line);
    }
}

/// Write test state snapshot to /tmp/ptm-test-state.json
#[tauri::command]
fn write_test_state(json: String) {
    if let Ok(mut f) = std::fs::File::create("/tmp/ptm-test-state.json") {
        let _ = f.write_all(json.as_bytes());
    }
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_windows,
            activate_window,
            log_event,
            write_test_state,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
