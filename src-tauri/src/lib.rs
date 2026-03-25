use gilrs::{Button, EventType, Gilrs};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command as StdCommand;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use sysinfo::System;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use tokio::sync::Mutex;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Program {
    pub id: String,
    pub name: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hours_played: Option<f64>,
}

fn get_data_dir(app: &tauri::AppHandle) -> PathBuf {
    let data_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    fs::create_dir_all(&data_dir).ok();
    data_dir
}

fn get_store_path(app: &tauri::AppHandle) -> PathBuf {
    get_data_dir(app).join("programs.json")
}

fn get_settings_path(app: &tauri::AppHandle) -> PathBuf {
    get_data_dir(app).join("settings.json")
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct AppSettings {
    #[serde(default)]
    pub igdb_client_id: String,
    #[serde(default)]
    pub igdb_client_secret: String,
}

fn load_settings(app: &tauri::AppHandle) -> AppSettings {
    let path = get_settings_path(app);
    if path.exists() {
        let data = fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        AppSettings::default()
    }
}

fn save_settings(app: &tauri::AppHandle, settings: &AppSettings) {
    let path = get_settings_path(app);
    if let Ok(data) = serde_json::to_string_pretty(settings) {
        fs::write(path, data).ok();
    }
}

fn load_programs(app: &tauri::AppHandle) -> Vec<Program> {
    let path = get_store_path(app);
    if path.exists() {
        let data = fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        vec![]
    }
}

fn save_programs(app: &tauri::AppHandle, programs: &[Program]) {
    let path = get_store_path(app);
    if let Ok(data) = serde_json::to_string_pretty(programs) {
        fs::write(path, data).ok();
    }
}

#[tauri::command]
fn get_programs(app: tauri::AppHandle) -> Vec<Program> {
    load_programs(&app)
}

#[tauri::command]
fn add_program(app: tauri::AppHandle, name: String, path: String) -> Vec<Program> {
    let mut programs = load_programs(&app);
    let id = format!(
        "{}_{}",
        name.to_lowercase().replace(' ', "_"),
        programs.len()
    );
    programs.push(Program {
        id,
        name,
        path,
        cover_url: None,
        favorite: false,
        hours_played: None,
    });
    save_programs(&app, &programs);
    programs
}

#[tauri::command]
fn remove_program(app: tauri::AppHandle, id: String) -> Vec<Program> {
    let mut programs = load_programs(&app);
    programs.retain(|p| p.id != id);
    save_programs(&app, &programs);
    programs
}

#[tauri::command]
fn update_program(
    app: tauri::AppHandle,
    id: String,
    name: Option<String>,
    cover_url: Option<String>,
    favorite: Option<bool>,
) -> Vec<Program> {
    let mut programs = load_programs(&app);
    if let Some(p) = programs.iter_mut().find(|p| p.id == id) {
        if let Some(n) = name {
            p.name = n;
        }
        if let Some(url) = cover_url {
            p.cover_url = if url.is_empty() { None } else { Some(url) };
        }
        if let Some(fav) = favorite {
            p.favorite = fav;
        }
    }
    save_programs(&app, &programs);
    programs
}

#[tauri::command]
fn launch_program(app: tauri::AppHandle, path: String, program_id: Option<String>, program_name: Option<String>) -> Result<String, String> {
    let program_path = std::path::Path::new(&path);
    let ext = program_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    if ext != "url" && ext != "lnk" && !program_path.exists() {
        return Err(format!("Program not found: {}", path));
    }

    // Snapshot PIDs before launch for detection
    let pids_before = if program_id.is_some() { snapshot_pids() } else { std::collections::HashSet::new() };

    StdCommand::new("cmd")
        .args(["/C", "start", "", &path])
        .spawn()
        .map_err(|e| format!("Failed to launch: {}", e))?;

    // Track the running game if we have an ID
    if let Some(pid_str) = &program_id {
        let p_name = program_name.clone().unwrap_or_default();
        let exe_path = path.clone();

        let app_clone = app.clone();
        let pid_str_clone = pid_str.clone();
        std::thread::spawn(move || {
            let mut found_pid: Option<u32> = None;

            // Try multiple times over 10 seconds
            for attempt in 0..5 {
                std::thread::sleep(std::time::Duration::from_secs(2));

                // First try exact exe name match
                if let Some(pid) = find_process_pid(&exe_path) {
                    found_pid = Some(pid);
                    println!("[running] Found by exe name match (attempt {}): PID {}", attempt, pid);
                    break;
                }

                // Then try before/after snapshot
                if let Some((pid, name)) = find_new_game_process(&pids_before) {
                    found_pid = Some(pid);
                    println!("[running] Found by snapshot diff (attempt {}): PID {} ({})", attempt, pid, name);
                    break;
                }
            }

            if let Some(pid) = found_pid {
                let state = app_clone.state::<RunningGamesState>();
                let mut games = state.games.lock().unwrap();

                // Mute all other running games
                for entry in games.values_mut() {
                    entry.active = false;
                    entry.muted = true;
                    set_process_mute(entry.pid, true);
                }

                games.insert(pid_str_clone.clone(), RunningGameEntry {
                    program_id: pid_str_clone,
                    program_name: p_name,
                    pid,
                    exe_path,
                    active: true,
                    muted: false,
                });
                println!("[running] Tracking game PID {}", pid);
                app_clone.emit("running-games-changed", ()).ok();
            } else {
                println!("[running] Could not find process for {}", exe_path);
            }
        });
    }

    if let Some(window) = app.get_webview_window("main") {
        println!("[launch] Hiding main window");
        window.set_fullscreen(false).ok();
        window.minimize().ok();
        if let Err(e) = window.hide() {
            println!("[launch] ERROR hiding main: {}", e);
        }
    } else {
        println!("[launch] ERROR: main window not found");
    }
    if let Some(fab) = app.get_webview_window("fab") {
        println!("[launch] Showing fab window");
        if let Err(e) = fab.show() {
            println!("[launch] ERROR showing fab: {}", e);
        }
        if let Err(e) = fab.set_focus() {
            println!("[launch] ERROR focusing fab: {}", e);
        }
        if let Ok(pos) = fab.outer_position() {
            println!("[launch] fab position: {:?}", pos);
        }
        if let Ok(size) = fab.outer_size() {
            println!("[launch] fab size: {:?}", size);
        }
    } else {
        println!("[launch] ERROR: fab window not found");
    }

    Ok("Program launched".to_string())
}

#[tauri::command]
fn show_launcher(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(overlay) = app.get_webview_window("overlay") {
        overlay.hide().ok();
    }
    if let Some(fab) = app.get_webview_window("fab") {
        fab.hide().ok();
    }
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|e| e.to_string())?;
        window.unminimize().ok();
        window.set_fullscreen(true).ok();
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn suspend_game_and_show_launcher(app: tauri::AppHandle) -> Result<(), String> {
    // Mute all running games (suspend audio)
    let state = app.state::<RunningGamesState>();
    {
        let mut games = state.games.lock().unwrap();
        for entry in games.values_mut() {
            entry.active = false;
            entry.muted = true;
            set_process_mute(entry.pid, true);
        }
    }
    app.emit("running-games-changed", ()).ok();

    // Hide overlay and FAB, show main launcher
    if let Some(overlay) = app.get_webview_window("overlay") {
        overlay.hide().ok();
    }
    if let Some(fab) = app.get_webview_window("fab") {
        fab.hide().ok();
    }
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|e| e.to_string())?;
        window.unminimize().ok();
        window.set_fullscreen(true).ok();
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn hide_launcher(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.set_fullscreen(false).ok();
        window.minimize().ok();
        window.hide().map_err(|e| e.to_string())?;
    }
    if let Some(fab) = app.get_webview_window("fab") {
        fab.show().ok();
        fab.set_focus().ok();
    }
    Ok(())
}

#[tauri::command]
fn show_overlay(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(fab) = app.get_webview_window("fab") {
        fab.hide().ok();
    }
    if let Some(overlay) = app.get_webview_window("overlay") {
        overlay.show().map_err(|e| e.to_string())?;
        overlay.set_focus().ok();
    }
    if let Some(state) = app.try_state::<OverlayState>() {
        state.visible.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
fn hide_overlay(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(overlay) = app.get_webview_window("overlay") {
        overlay.hide().map_err(|e| e.to_string())?;
    }
    if let Some(fab) = app.get_webview_window("fab") {
        fab.show().ok();
    }
    if let Some(state) = app.try_state::<OverlayState>() {
        state.visible.store(false, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn toggle_overlay_cmd(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(state) = app.try_state::<OverlayState>() {
        toggle_overlay(&app, &state.visible);
    }
    Ok(())
}

#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> AppSettings {
    load_settings(&app)
}

/* ── Downloads folder listing ── */

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DownloadEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: u64,
}

#[tauri::command]
fn list_downloads() -> Result<Vec<DownloadEntry>, String> {
    let downloads = dirs::download_dir()
        .ok_or_else(|| "Could not find Downloads folder".to_string())?;
    if !downloads.exists() {
        return Ok(vec![]);
    }
    let mut entries: Vec<DownloadEntry> = Vec::new();
    let read = fs::read_dir(&downloads).map_err(|e| format!("Failed to read downloads: {}", e))?;
    for entry in read.flatten() {
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        entries.push(DownloadEntry {
            name: entry.file_name().to_string_lossy().to_string(),
            path: entry.path().to_string_lossy().to_string(),
            is_dir: meta.is_dir(),
            size: meta.len(),
            modified,
        });
    }
    // Sort by modified time, newest first
    entries.sort_by(|a, b| b.modified.cmp(&a.modified));
    Ok(entries)
}

#[tauri::command]
fn open_url_in_browser(url: String) -> Result<(), String> {
    StdCommand::new("cmd")
        .args(["/C", "start", "", &url])
        .spawn()
        .map_err(|e| format!("Failed to open URL: {}", e))?;
    Ok(())
}

/* ── Running Games Manager ── */

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RunningGameInfo {
    pub program_id: String,
    pub program_name: String,
    pub pid: u32,
    pub exe_path: String,
    pub active: bool,
    pub muted: bool,
}

struct RunningGamesState {
    games: std::sync::Mutex<HashMap<String, RunningGameEntry>>,
}

struct RunningGameEntry {
    program_id: String,
    program_name: String,
    pid: u32,
    exe_path: String,
    active: bool,
    muted: bool,
}

#[cfg(target_os = "windows")]
fn set_process_mute(pid: u32, mute: bool) {
    use std::ptr;
    use windows::Win32::Media::Audio::*;
    use windows::Win32::System::Com::*;
    use windows::core::Interface;

    unsafe {
        let _ = CoInitializeEx(Some(ptr::null_mut()), COINIT_MULTITHREADED);

        let enumerator: IMMDeviceEnumerator = match CoCreateInstance(
            &MMDeviceEnumerator,
            None,
            CLSCTX_ALL,
        ) {
            Ok(e) => e,
            Err(_) => return,
        };

        let device = match enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia) {
            Ok(d) => d,
            Err(_) => return,
        };

        let manager: IAudioSessionManager2 = match device.Activate(CLSCTX_ALL, None) {
            Ok(m) => m,
            Err(_) => return,
        };

        let session_enum = match manager.GetSessionEnumerator() {
            Ok(e) => e,
            Err(_) => return,
        };

        let count = match session_enum.GetCount() {
            Ok(c) => c,
            Err(_) => return,
        };

        for i in 0..count {
            let session = match session_enum.GetSession(i) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let session2: IAudioSessionControl2 = match session.cast() {
                Ok(s) => s,
                Err(_) => continue,
            };
            let session_pid = match session2.GetProcessId() {
                Ok(p) => p,
                Err(_) => continue,
            };
            if session_pid == pid {
                let volume: ISimpleAudioVolume = match session.cast() {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let _ = volume.SetMute(mute, ptr::null());
                println!("[audio] {} PID {}", if mute { "Muted" } else { "Unmuted" }, pid);
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn set_process_mute(_pid: u32, _mute: bool) {}

#[cfg(target_os = "windows")]
fn clear_window_effects(window: &tauri::WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::*;

    // Set webview background to transparent
    if let Ok(webview) = window.as_ref().window().hwnd() {
        unsafe {
            let hwnd = HWND(webview.0 as *mut std::ffi::c_void);
            
            // Disable rounded corners
            let corner: i32 = 1; // DWMWCP_DONOTROUND
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner as *const _ as *const std::ffi::c_void,
                4,
            );
            
            // Disable backdrop
            let backdrop: i32 = 1; // DWMSBT_NONE  
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_SYSTEMBACKDROP_TYPE,
                &backdrop as *const _ as *const std::ffi::c_void,
                4,
            );
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn clear_window_effects(_window: &tauri::WebviewWindow) {}

fn find_process_pid(exe_path: &str) -> Option<u32> {
    // Try exact exe name match first
    let exe_name = std::path::Path::new(exe_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();

    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    if !exe_name.is_empty() && exe_name.ends_with(".exe") {
        for (pid, process) in sys.processes() {
            let pname = process.name().to_string_lossy().to_lowercase();
            if pname == exe_name {
                return Some(pid.as_u32());
            }
        }
    }

    // For shortcuts (.lnk, .url) or if exact match failed,
    // try matching by the stem of the filename (e.g. "Halo" from "Halo.lnk")
    let stem = std::path::Path::new(exe_path)
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();
    if stem.is_empty() {
        return None;
    }

    for (pid, process) in sys.processes() {
        let pname = process.name().to_string_lossy().to_lowercase();
        // Skip system/common processes
        if pname == "cmd.exe" || pname == "conhost.exe" || pname == "explorer.exe" {
            continue;
        }
        if pname.contains(&stem) {
            return Some(pid.as_u32());
        }
    }
    None
}

/// Snapshot current PIDs, used for before/after comparison on launch
fn snapshot_pids() -> std::collections::HashSet<u32> {
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    sys.processes().keys().map(|p| p.as_u32()).collect()
}

/// Find new processes that appeared after launch (ignoring system processes)
fn find_new_game_process(before: &std::collections::HashSet<u32>) -> Option<(u32, String)> {
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let dominated = ["cmd.exe", "conhost.exe", "rundll32.exe", "werfault.exe",
                     "backgroundtaskhost.exe", "runtimebroker.exe", "svchost.exe"];

    for (pid, process) in sys.processes() {
        let pid_u32 = pid.as_u32();
        if before.contains(&pid_u32) {
            continue;
        }
        let pname = process.name().to_string_lossy().to_lowercase();
        if dominated.iter().any(|&d| pname == d) {
            continue;
        }
        // Only consider processes with a window (heuristic: has > 5MB memory)
        if process.memory() > 5_000_000 {
            return Some((pid_u32, pname));
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn focus_process_window(pid: u32) {
    use windows::Win32::Foundation::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    unsafe {
        struct EnumData {
            pid: u32,
            hwnd: HWND,
        }
        let mut data = EnumData { pid, hwnd: HWND::default() };

        unsafe extern "system" fn enum_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let data = &mut *(lparam.0 as *mut EnumData);
            let mut window_pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut window_pid));
            if window_pid == data.pid && IsWindowVisible(hwnd).as_bool() {
                data.hwnd = hwnd;
                return FALSE;
            }
            TRUE
        }

        let _ = EnumWindows(
            Some(enum_callback),
            LPARAM(&mut data as *mut EnumData as isize),
        );

        if data.hwnd != HWND::default() {
            if IsIconic(data.hwnd).as_bool() {
                let _ = ShowWindow(data.hwnd, SW_RESTORE);
            }
            let _ = SetForegroundWindow(data.hwnd);
            println!("[focus] Brought PID {} window to foreground", pid);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn focus_process_window(_pid: u32) {}

fn running_games_to_info(games: &HashMap<String, RunningGameEntry>) -> Vec<RunningGameInfo> {
    games.values().map(|e| RunningGameInfo {
        program_id: e.program_id.clone(),
        program_name: e.program_name.clone(),
        pid: e.pid,
        exe_path: e.exe_path.clone(),
        active: e.active,
        muted: e.muted,
    }).collect()
}

#[tauri::command]
fn get_running_games(app: tauri::AppHandle) -> Vec<RunningGameInfo> {
    let state = app.state::<RunningGamesState>();
    let mut games = state.games.lock().unwrap();
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let dead_ids: Vec<String> = games
        .iter()
        .filter(|(_, entry)| {
            let pid = sysinfo::Pid::from_u32(entry.pid);
            !sys.processes().contains_key(&pid)
        })
        .map(|(id, _)| id.clone())
        .collect();
    for id in &dead_ids {
        println!("[running] Process died: {}", id);
        games.remove(id);
    }

    running_games_to_info(&games)
}

#[tauri::command]
fn switch_to_game(app: tauri::AppHandle, program_id: String) -> Result<Vec<RunningGameInfo>, String> {
    let state = app.state::<RunningGamesState>();
    let mut games = state.games.lock().unwrap();

    let target_pid = games.get(&program_id).map(|e| e.pid);
    if target_pid.is_none() {
        return Err("Game not running".into());
    }
    let target_pid = target_pid.unwrap();

    for (id, entry) in games.iter_mut() {
        if *id == program_id {
            entry.active = true;
            entry.muted = false;
            set_process_mute(entry.pid, false);
        } else {
            entry.active = false;
            entry.muted = true;
            set_process_mute(entry.pid, true);
        }
    }

    focus_process_window(target_pid);

    if let Some(window) = app.get_webview_window("main") {
        window.set_fullscreen(false).ok();
        window.minimize().ok();
        window.hide().ok();
    }
    if let Some(fab) = app.get_webview_window("fab") {
        fab.show().ok();
    }

    Ok(running_games_to_info(&games))
}

#[tauri::command]
fn close_game(app: tauri::AppHandle, program_id: String) -> Vec<RunningGameInfo> {
    let state = app.state::<RunningGamesState>();
    let mut games = state.games.lock().unwrap();

    if let Some(entry) = games.remove(&program_id) {
        set_process_mute(entry.pid, false);
        let mut sys = System::new();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        let pid = sysinfo::Pid::from_u32(entry.pid);
        if let Some(process) = sys.process(pid) {
            process.kill();
            println!("[running] Killed {} (PID {})", entry.program_name, entry.pid);
        }
    }

    running_games_to_info(&games)
}

#[tauri::command]
fn save_igdb_credentials(
    app: tauri::AppHandle,
    client_id: String,
    client_secret: String,
) -> AppSettings {
    let mut settings = load_settings(&app);
    settings.igdb_client_id = client_id;
    settings.igdb_client_secret = client_secret;
    save_settings(&app, &settings);
    settings
}

/* ── IGDB API proxy (avoids CORS) ── */

struct IgdbTokenCache {
    token: Mutex<Option<CachedToken>>,
    http: Client,
}

struct CachedToken {
    access_token: String,
    expires_at: u64,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

async fn get_igdb_token(
    cache: &IgdbTokenCache,
    client_id: &str,
    client_secret: &str,
) -> Result<String, String> {
    let mut guard = cache.token.lock().await;
    if let Some(ref t) = *guard {
        if now_secs() < t.expires_at {
            return Ok(t.access_token.clone());
        }
    }
    // Fetch new token
    let url = format!(
        "https://id.twitch.tv/oauth2/token?client_id={}&client_secret={}&grant_type=client_credentials",
        client_id, client_secret
    );
    let res = cache
        .http
        .post(&url)
        .send()
        .await
        .map_err(|e| format!("Twitch auth request failed: {}", e))?;
    if !res.status().is_success() {
        return Err(format!("Twitch auth failed: {}", res.status()));
    }
    let body: serde_json::Value = res
        .json()
        .await
        .map_err(|e| format!("Failed to parse token: {}", e))?;
    let access_token = body["access_token"]
        .as_str()
        .ok_or("Missing access_token")?
        .to_string();
    let expires_in = body["expires_in"].as_u64().unwrap_or(3600);
    *guard = Some(CachedToken {
        access_token: access_token.clone(),
        expires_at: now_secs() + expires_in.saturating_sub(60),
    });
    Ok(access_token)
}

#[tauri::command]
async fn igdb_validate(
    app: tauri::AppHandle,
    client_id: String,
    client_secret: String,
) -> Result<bool, String> {
    let cache = app.state::<IgdbTokenCache>();
    // Clear existing token so we test fresh
    {
        let mut guard = cache.token.lock().await;
        *guard = None;
    }
    match get_igdb_token(&cache, &client_id, &client_secret).await {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct IgdbGameResult {
    pub id: i64,
    pub name: String,
    pub background_image: Option<String>,
    pub rating: Option<f64>,
    pub released: Option<String>,
}

fn parse_igdb_cover_url(cover: &serde_json::Value) -> Option<String> {
    let url = cover.get("url")?.as_str()?;
    let mut s = url.to_string();
    if s.starts_with("//") {
        s = format!("https:{}", s);
    }
    // Use t_720p for high quality covers
    Some(s.replace("t_thumb", "t_720p"))
}

fn parse_igdb_image_url(img: &serde_json::Value, size: &str) -> Option<String> {
    let url = img.get("url")?.as_str()?;
    let mut s = url.to_string();
    if s.starts_with("//") {
        s = format!("https:{}", s);
    }
    Some(s.replace("t_thumb", size))
}

fn map_igdb_result(raw: &serde_json::Value) -> IgdbGameResult {
    let rating = raw.get("rating").and_then(|r| r.as_f64());
    let first_release = raw.get("first_release_date").and_then(|d| d.as_i64());
    let released = first_release.map(|ts| format!("{}", ts));

    // Try screenshot first (landscape, better for cards), fall back to cover
    let bg_image = raw
        .get("screenshots")
        .and_then(|s| s.as_array())
        .and_then(|arr| arr.first())
        .and_then(|s| parse_igdb_image_url(s, "t_screenshot_big"))
        .or_else(|| raw.get("cover").and_then(parse_igdb_cover_url));

    IgdbGameResult {
        id: raw.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
        name: raw
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        background_image: bg_image,
        rating: rating.map(|r| (r * 10.0).round() / 100.0),
        released,
    }
}

async fn igdb_query(
    cache: &IgdbTokenCache,
    client_id: &str,
    client_secret: &str,
    body: &str,
) -> Result<Vec<IgdbGameResult>, String> {
    let token = get_igdb_token(cache, client_id, client_secret).await?;
    let res = cache
        .http
        .post("https://api.igdb.com/v4/games")
        .header("Client-ID", client_id)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "text/plain")
        .body(body.to_string())
        .send()
        .await
        .map_err(|e| format!("IGDB request failed: {}", e))?;

    if res.status().as_u16() == 401 {
        // Token expired, clear and retry once
        {
            let mut guard = cache.token.lock().await;
            *guard = None;
        }
        let token2 = get_igdb_token(cache, client_id, client_secret).await?;
        let res2 = cache
            .http
            .post("https://api.igdb.com/v4/games")
            .header("Client-ID", client_id)
            .header("Authorization", format!("Bearer {}", token2))
            .header("Content-Type", "text/plain")
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| format!("IGDB retry failed: {}", e))?;
        let arr: Vec<serde_json::Value> = res2.json().await.unwrap_or_default();
        return Ok(arr.iter().map(map_igdb_result).collect());
    }

    if !res.status().is_success() {
        return Err(format!("IGDB error: {}", res.status()));
    }
    let arr: Vec<serde_json::Value> = res.json().await.unwrap_or_default();
    Ok(arr.iter().map(map_igdb_result).collect())
}

#[tauri::command]
async fn igdb_search(app: tauri::AppHandle, query: String) -> Result<Vec<IgdbGameResult>, String> {
    let settings = load_settings(&app);
    if settings.igdb_client_id.is_empty() || settings.igdb_client_secret.is_empty() {
        return Err("IGDB not configured".into());
    }
    let cache = app.state::<IgdbTokenCache>();
    let escaped = query.replace('"', "\\\"");
    let body = format!(
        "search \"{}\"; fields name,cover.url,screenshots.url,rating,first_release_date; limit 10;",
        escaped
    );
    igdb_query(&cache, &settings.igdb_client_id, &settings.igdb_client_secret, &body).await
}

#[tauri::command]
async fn igdb_popular(app: tauri::AppHandle, limit: Option<u32>, offset: Option<u32>) -> Result<Vec<IgdbGameResult>, String> {
    let settings = load_settings(&app);
    if settings.igdb_client_id.is_empty() || settings.igdb_client_secret.is_empty() {
        return Err("IGDB not configured".into());
    }
    let cache = app.state::<IgdbTokenCache>();
    let lim = limit.unwrap_or(20);
    let off = offset.unwrap_or(0);
    let body = format!(
        "fields name,cover.url,screenshots.url,rating,first_release_date; where rating > 80 & cover != null; sort rating desc; limit {}; offset {};",
        lim, off
    );
    igdb_query(&cache, &settings.igdb_client_id, &settings.igdb_client_secret, &body).await
}

/* ── Image picker: returns all images for games matching a query ── */

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct IgdbImageResult {
    pub game_id: i64,
    pub game_name: String,
    pub url: String,
    pub kind: String, // "screenshot", "cover", "artwork"
}

fn collect_images(raw: &serde_json::Value) -> Vec<IgdbImageResult> {
    let game_id = raw.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    let game_name = raw
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mut images = Vec::new();

    // Screenshots (landscape, best for cards)
    if let Some(arr) = raw.get("screenshots").and_then(|v| v.as_array()) {
        for s in arr {
            if let Some(url) = parse_igdb_image_url(s, "t_screenshot_big") {
                images.push(IgdbImageResult {
                    game_id,
                    game_name: game_name.clone(),
                    url,
                    kind: "screenshot".into(),
                });
            }
        }
    }
    // Artworks (high quality promotional art)
    if let Some(arr) = raw.get("artworks").and_then(|v| v.as_array()) {
        for a in arr {
            if let Some(url) = parse_igdb_image_url(a, "t_screenshot_big") {
                images.push(IgdbImageResult {
                    game_id,
                    game_name: game_name.clone(),
                    url,
                    kind: "artwork".into(),
                });
            }
        }
    }
    // Cover (portrait, fallback)
    if let Some(cover) = raw.get("cover") {
        if let Some(url) = parse_igdb_image_url(cover, "t_720p") {
            images.push(IgdbImageResult {
                game_id,
                game_name: game_name.clone(),
                url,
                kind: "cover".into(),
            });
        }
    }
    images
}

#[tauri::command]
async fn igdb_game_images(
    app: tauri::AppHandle,
    query: String,
) -> Result<Vec<IgdbImageResult>, String> {
    let settings = load_settings(&app);
    if settings.igdb_client_id.is_empty() || settings.igdb_client_secret.is_empty() {
        return Err("IGDB not configured".into());
    }
    let cache = app.state::<IgdbTokenCache>();
    let escaped = query.replace('"', "\\\"");
    let body = format!(
        "search \"{}\"; fields name,cover.url,screenshots.url,artworks.url; limit 5;",
        escaped
    );
    let token = get_igdb_token(&cache, &settings.igdb_client_id, &settings.igdb_client_secret).await?;
    let res = cache
        .http
        .post("https://api.igdb.com/v4/games")
        .header("Client-ID", &settings.igdb_client_id)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "text/plain")
        .body(body)
        .send()
        .await
        .map_err(|e| format!("IGDB request failed: {}", e))?;

    if !res.status().is_success() {
        return Err(format!("IGDB error: {}", res.status()));
    }
    let arr: Vec<serde_json::Value> = res.json().await.unwrap_or_default();
    let mut all_images: Vec<IgdbImageResult> = Vec::new();
    for raw in &arr {
        all_images.extend(collect_images(raw));
    }
    Ok(all_images)
}

fn start_gamepad_thread(app_handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut gilrs = match Gilrs::new() {
            Ok(g) => {
                println!("[gamepad] gilrs initialized successfully");
                g
            }
            Err(e) => {
                eprintln!("[gamepad] Failed to init gilrs: {}", e);
                return;
            }
        };

        println!("[gamepad] Polling started");

        for (_id, gamepad) in gilrs.gamepads() {
            println!("[gamepad] Found: {} (power: {:?})", gamepad.name(), gamepad.power_info());
        }

        // Track previous axis values for edge-triggered stick events
        let mut prev_lx: f32 = 0.0;
        let mut prev_ly: f32 = 0.0;
        let deadzone: f32 = 0.5;
        let mut axis_cooldown = std::time::Instant::now();
        
        // Track bumper states for LB+RB combo
        let mut lb_held = false;
        let mut rb_held = false;
        loop {
            while let Some(event) = gilrs.next_event() {
                match event.event {
                    EventType::ButtonPressed(button, _) => {
                        println!("[gamepad] Button pressed: {:?}", button);

                        // Track bumper presses for LB+RB combo
                        if button == Button::LeftTrigger {
                            lb_held = true;
                        }
                        if button == Button::RightTrigger {
                            rb_held = true;
                        }

                        // LB + RB combo OR Guide/Home button suspends game and shows launcher
                        if (lb_held && rb_held) || button == Button::Mode {
                            println!("[gamepad] LB+RB or Guide pressed — suspending game and showing launcher");
                            let main_visible = app_handle
                                .get_webview_window("main")
                                .and_then(|w| w.is_visible().ok())
                                .unwrap_or(false);
                            
                            if main_visible {
                                // Already in launcher, do nothing
                                continue;
                            }

                            // Suspend all running games (mute audio)
                            if let Some(state) = app_handle.try_state::<RunningGamesState>() {
                                let mut games = state.games.lock().unwrap();
                                for entry in games.values_mut() {
                                    entry.active = false;
                                    entry.muted = true;
                                    set_process_mute(entry.pid, true);
                                }
                            }
                            app_handle.emit("running-games-changed", ()).ok();

                            // Hide overlay and FAB, show main launcher
                            if let Some(overlay) = app_handle.get_webview_window("overlay") {
                                overlay.hide().ok();
                            }
                            if let Some(fab) = app_handle.get_webview_window("fab") {
                                fab.hide().ok();
                            }
                            if let Some(window) = app_handle.get_webview_window("main") {
                                window.show().ok();
                                window.unminimize().ok();
                                window.set_fullscreen(true).ok();
                                window.set_focus().ok();
                            }
                            if let Some(state) = app_handle.try_state::<OverlayState>() {
                                state.visible.store(false, Ordering::SeqCst);
                            }
                            continue;
                        }

                        let button_name = match button {
                            Button::South => "A",
                            Button::East => "B",
                            Button::North => "Y",
                            Button::West => "X",
                            Button::DPadUp => "DPadUp",
                            Button::DPadDown => "DPadDown",
                            Button::DPadLeft => "DPadLeft",
                            Button::DPadRight => "DPadRight",
                            Button::LeftTrigger => "LB",
                            Button::RightTrigger => "RB",
                            Button::LeftTrigger2 => "LT",
                            Button::RightTrigger2 => "RT",
                            Button::Select => "Select",
                            Button::Start => "Start",
                            _ => continue,
                        };
                        app_handle.emit("gamepad-button", button_name).ok();
                    }
                    EventType::ButtonReleased(button, _) => {
                        // Track bumper releases for LB+RB combo
                        if button == Button::LeftTrigger {
                            lb_held = false;
                        }
                        if button == Button::RightTrigger {
                            rb_held = false;
                        }
                    }
                    EventType::AxisChanged(axis, value, _) => {
                        if axis_cooldown.elapsed() < std::time::Duration::from_millis(150) {
                            // Update tracked values but don't emit
                            match axis {
                                gilrs::Axis::LeftStickX => prev_lx = value,
                                gilrs::Axis::LeftStickY => prev_ly = value,
                                _ => {}
                            }
                            continue;
                        }
                        let mut emitted = false;
                        match axis {
                            gilrs::Axis::LeftStickX => {
                                if value > deadzone && prev_lx <= deadzone {
                                    app_handle.emit("gamepad-button", "DPadRight").ok();
                                    emitted = true;
                                } else if value < -deadzone && prev_lx >= -deadzone {
                                    app_handle.emit("gamepad-button", "DPadLeft").ok();
                                    emitted = true;
                                }
                                prev_lx = value;
                            }
                            gilrs::Axis::LeftStickY => {
                                // gilrs Y axis: positive = up, negative = down
                                if value > deadzone && prev_ly <= deadzone {
                                    app_handle.emit("gamepad-button", "DPadUp").ok();
                                    emitted = true;
                                } else if value < -deadzone && prev_ly >= -deadzone {
                                    app_handle.emit("gamepad-button", "DPadDown").ok();
                                    emitted = true;
                                }
                                prev_ly = value;
                            }
                            _ => {}
                        }
                        if emitted {
                            axis_cooldown = std::time::Instant::now();
                        }
                    }
                    _ => {}
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
    });
}

fn toggle_overlay(app: &tauri::AppHandle, overlay_visible: &Arc<AtomicBool>) {
    let is_visible = overlay_visible.load(Ordering::SeqCst);
    println!("[hotkey] Toggle overlay. Currently visible: {}", is_visible);

    // Check if main window is visible
    let main_visible = app
        .get_webview_window("main")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);

    if main_visible {
        // Main is visible, do nothing (user is already in launcher)
        return;
    }

    // Suspend games and show launcher
    if let Some(state) = app.try_state::<RunningGamesState>() {
        let mut games = state.games.lock().unwrap();
        for entry in games.values_mut() {
            entry.active = false;
            entry.muted = true;
            set_process_mute(entry.pid, true);
        }
    }
    app.emit("running-games-changed", ()).ok();

    if let Some(overlay) = app.get_webview_window("overlay") {
        overlay.hide().ok();
    }
    if let Some(fab) = app.get_webview_window("fab") {
        fab.hide().ok();
    }
    if let Some(window) = app.get_webview_window("main") {
        window.show().ok();
        window.unminimize().ok();
        window.set_fullscreen(true).ok();
        window.set_focus().ok();
    }
    overlay_visible.store(false, Ordering::SeqCst);
}

struct OverlayState {
    visible: Arc<AtomicBool>,
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            // IGDB token cache
            app.manage(IgdbTokenCache {
                token: Mutex::new(None),
                http: Client::new(),
            });

            // Running games tracker
            app.manage(RunningGamesState {
                games: std::sync::Mutex::new(HashMap::new()),
            });

            // Overlay visibility state
            let overlay_visible = Arc::new(AtomicBool::new(false));
            app.manage(OverlayState {
                visible: overlay_visible.clone(),
            });

            // Clear window effects (remove Windows 11 rounded corners and backdrop) for FAB and overlay
            if let Some(fab) = app.get_webview_window("fab") {
                clear_window_effects(&fab);
            }
            if let Some(overlay) = app.get_webview_window("overlay") {
                clear_window_effects(&overlay);
            }

            // Global hotkey: Ctrl+Shift+G toggles overlay (works even when game has focus)
            let app_handle = app.handle().clone();
            let ov = overlay_visible.clone();
            app.global_shortcut().on_shortcut("Ctrl+Shift+G", move |_app, _shortcut, _event| {
                toggle_overlay(&app_handle, &ov);
            }).expect("failed to register global shortcut");
            println!("[hotkey] Registered Ctrl+Shift+G for overlay toggle");

            // System tray
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().unwrap())
                .tooltip("GameKiosk - Ctrl+Shift+G or LB+RB")
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(overlay) = app.get_webview_window("overlay") {
                            overlay.hide().ok();
                        }
                        if let Some(fab) = app.get_webview_window("fab") {
                            fab.hide().ok();
                        }
                        if let Some(window) = app.get_webview_window("main") {
                            window.show().ok();
                            window.unminimize().ok();
                            window.set_fullscreen(true).ok();
                            window.set_focus().ok();
                        }
                    }
                })
                .build(app)?;

            // Gamepad thread for overlay menu navigation (D-pad, A, B)
            start_gamepad_thread(app.handle().clone());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_programs,
            add_program,
            remove_program,
            update_program,
            launch_program,
            show_launcher,
            suspend_game_and_show_launcher,
            hide_launcher,
            show_overlay,
            hide_overlay,
            toggle_overlay_cmd,
            quit_app,
            get_settings,
            save_igdb_credentials,
            igdb_validate,
            igdb_search,
            igdb_popular,
            igdb_game_images,
            list_downloads,
            open_url_in_browser,
            get_running_games,
            switch_to_game,
            close_game,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
