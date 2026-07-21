use tauri::Emitter;

mod discovery;
mod io_util;
mod launcher;
mod model;
mod parser_claude;
mod parser_codex;
mod procs;
mod snapshot;
mod status;

#[tauri::command]
fn get_snapshot() -> Vec<model::Project> {
    snapshot::build()
}

#[tauri::command]
fn resume_session(
    cwd: String,
    agent: model::Agent,
    session_id: String,
    fresh: bool,
    terminal: String,
) -> Result<(), String> {
    let line = launcher::build_shell_line(&cwd, agent, &session_id, fresh);
    launcher::launch(&line, &terminal)
}

#[tauri::command]
fn open_folder(path: String) -> Result<(), String> {
    std::process::Command::new("open")
        .arg(&path)
        .status()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            resume_session,
            open_folder
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                let projects = snapshot::build();
                let _ = handle.emit("snapshot", projects);
                std::thread::sleep(std::time::Duration::from_secs(2));
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
