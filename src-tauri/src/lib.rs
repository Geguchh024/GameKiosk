use gilrs::{Button, EventType, Gilrs};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command as StdCommand;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use sysinfo::System;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use tokio::sync::Mutex;

/// File logger that writes to gamekiosk_debug.log next to the executable
fn log(msg: &str) {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut targets: HashSet<PathBuf> = HashSet::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            targets.insert(parent.join("gamekiosk_debug.log"));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        targets.insert(cwd.join("gamekiosk_debug.log"));
    }
    targets.insert(std::env::temp_dir().join("gamekiosk_debug.log"));

    for log_path in targets {
        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&log_path) {
            let _ = writeln!(f, "[{}] {}", timestamp, msg);
        }
    }
}

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

const TRAY_MOUSE_SPEED_MIN: f32 = 0.2;
const TRAY_MOUSE_SPEED_MAX: f32 = 1.5;
const TRAY_MOUSE_SPEED_DEFAULT: f32 = 0.6;
const TRAY_MOUSE_ENABLED_DEFAULT: bool = true;

fn default_tray_mouse_speed() -> f32 {
    TRAY_MOUSE_SPEED_DEFAULT
}

fn clamp_tray_mouse_speed(speed: f32) -> f32 {
    speed.clamp(TRAY_MOUSE_SPEED_MIN, TRAY_MOUSE_SPEED_MAX)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppSettings {
    #[serde(default)]
    pub igdb_client_id: String,
    #[serde(default)]
    pub igdb_client_secret: String,
    #[serde(default = "default_tray_mouse_speed")]
    pub tray_mouse_speed: f32,
    #[serde(default = "default_tray_mouse_enabled")]
    pub tray_mouse_enabled: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            igdb_client_id: String::new(),
            igdb_client_secret: String::new(),
            tray_mouse_speed: TRAY_MOUSE_SPEED_DEFAULT,
            tray_mouse_enabled: TRAY_MOUSE_ENABLED_DEFAULT,
        }
    }
}

fn default_tray_mouse_enabled() -> bool {
    TRAY_MOUSE_ENABLED_DEFAULT
}

#[derive(Debug, Clone, Copy)]
struct TrayMouseConfig {
    speed: f32,
    enabled: bool,
}

struct TrayMouseConfigState {
    config: std::sync::Mutex<TrayMouseConfig>,
}

struct TrayMouseToggleState {
    last_toggle_ms: AtomicU64,
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn load_settings(app: &tauri::AppHandle) -> AppSettings {
    let path = get_settings_path(app);
    if path.exists() {
        let data = fs::read_to_string(&path).unwrap_or_default();
        let mut settings: AppSettings = serde_json::from_str(&data).unwrap_or_default();
        settings.tray_mouse_speed = clamp_tray_mouse_speed(settings.tray_mouse_speed);
        settings
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
    log("cmd: get_programs called");
    let programs = load_programs(&app);
    log(&format!("cmd: get_programs returning {} programs", programs.len()));
    programs
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
                if let Some(state) = app_clone.try_state::<RunningGamesState>() {
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
                }
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
        if let Err(e) = show_fab_window(&fab) {
            println!("[launch] ERROR showing fab: {}", e);
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

    // Re-assert topmost shortly after launch because many games switch to fullscreen after startup.
    let app_for_fab = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(900));
        if let Some(fab) = app_for_fab.get_webview_window("fab") {
            show_fab_window(&fab).ok();
        }
    });

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
    app.emit("launcher-shown", ()).ok();
    Ok(())
}

#[tauri::command]
fn suspend_game_and_show_launcher(app: tauri::AppHandle) -> Result<(), String> {
    // Mute all running games (suspend audio)
    if let Some(state) = app.try_state::<RunningGamesState>() {
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
    app.emit("launcher-shown", ()).ok();
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
        show_fab_window(&fab).ok();
    }
    Ok(())
}

#[tauri::command]
fn show_overlay(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(fab) = app.get_webview_window("fab") {
        fab.hide().ok();
    }
    if let Some(overlay) = app.get_webview_window("overlay") {
        show_overlay_window(&overlay)?;
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
        show_fab_window(&fab).ok();
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
    log("cmd: get_settings called");
    let settings = load_settings(&app);
    log(&format!("cmd: get_settings returning (igdb configured: {})", !settings.igdb_client_id.is_empty()));
    settings
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

#[cfg(target_os = "windows")]
fn force_window_topmost(window: &tauri::WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    };

    if let Ok(webview) = window.as_ref().window().hwnd() {
        unsafe {
            let hwnd = HWND(webview.0 as *mut std::ffi::c_void);
            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn force_window_topmost(_window: &tauri::WebviewWindow) {}

fn show_fab_window(fab: &tauri::WebviewWindow) -> Result<(), String> {
    fab.show().map_err(|e| e.to_string())?;
    fab.set_always_on_top(true).ok();
    force_window_topmost(fab);
    Ok(())
}

fn show_overlay_window(overlay: &tauri::WebviewWindow) -> Result<(), String> {
    overlay.show().map_err(|e| e.to_string())?;
    overlay.set_always_on_top(true).ok();
    force_window_topmost(overlay);
    Ok(())
}

fn is_launcher_active(app: &tauri::AppHandle) -> bool {
    let main_visible = app
        .get_webview_window("main")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);

    let main_minimized = app
        .get_webview_window("main")
        .and_then(|w| w.is_minimized().ok())
        .unwrap_or(false);

    let fab_visible = app
        .get_webview_window("fab")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);

    let overlay_visible = app
        .get_webview_window("overlay")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);

    main_visible && !main_minimized && !fab_visible && !overlay_visible
}

fn is_overlay_visible(app: &tauri::AppHandle) -> bool {
    app.get_webview_window("overlay")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}

fn should_forward_gamepad_to_ui(app: &tauri::AppHandle) -> bool {
    is_launcher_active(app) || is_overlay_visible(app)
}

fn current_tray_mouse_speed(app: &tauri::AppHandle) -> f32 {
    app.try_state::<TrayMouseConfigState>()
        .map(|state| {
            let cfg = state.config.lock().unwrap();
            cfg.speed
        })
        .unwrap_or(TRAY_MOUSE_SPEED_DEFAULT)
}

fn is_tray_mouse_enabled(app: &tauri::AppHandle) -> bool {
    app.try_state::<TrayMouseConfigState>()
        .map(|state| {
            let cfg = state.config.lock().unwrap();
            cfg.enabled
        })
        .unwrap_or(TRAY_MOUSE_ENABLED_DEFAULT)
}

fn toggle_tray_mouse_enabled(app: &tauri::AppHandle) -> bool {
    let mut enabled = TRAY_MOUSE_ENABLED_DEFAULT;

    if let Some(state) = app.try_state::<TrayMouseConfigState>() {
        let mut cfg = state.config.lock().unwrap();
        cfg.enabled = !cfg.enabled;
        enabled = cfg.enabled;
    }

    let mut settings = load_settings(app);
    settings.tray_mouse_enabled = enabled;
    save_settings(app, &settings);

    app.emit("tray-mouse-enabled-changed", enabled).ok();
    log(&format!("[tray-mouse] enabled set to {}", enabled));
    enabled
}

fn request_toggle_tray_mouse_enabled(app: &tauri::AppHandle) -> bool {
    const TOGGLE_DEBOUNCE_MS: u64 = 250;
    let now = now_millis();

    if let Some(state) = app.try_state::<TrayMouseToggleState>() {
        let last = state.last_toggle_ms.load(Ordering::SeqCst);
        if now.saturating_sub(last) < TOGGLE_DEBOUNCE_MS {
            return is_tray_mouse_enabled(app);
        }
        state.last_toggle_ms.store(now, Ordering::SeqCst);
    }

    toggle_tray_mouse_enabled(app)
}

fn is_tray_mouse_control_active(app: &tauri::AppHandle) -> bool {
    if should_forward_gamepad_to_ui(app) {
        return false;
    }
    if !is_tray_mouse_enabled(app) {
        return false;
    }

    app.get_webview_window("main")
        .map(|w| {
            let visible = w.is_visible().ok().unwrap_or(false);
            let minimized = w.is_minimized().ok().unwrap_or(false);
            !visible || minimized
        })
        .unwrap_or(false)
}

#[cfg(target_os = "windows")]
fn tray_move_mouse_from_stick(lx: f32, ly: f32, speed_scale: f32) {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetCursorPos};

    const DEADZONE: f32 = 0.2;
    const BASE_MAX_SPEED: f32 = 10.0;
    let max_speed = BASE_MAX_SPEED * clamp_tray_mouse_speed(speed_scale);

    let scaled = |v: f32| -> f32 {
        if v.abs() <= DEADZONE {
            return 0.0;
        }
        let normalized = ((v.abs() - DEADZONE) / (1.0 - DEADZONE)).clamp(0.0, 1.0);
        let curve = normalized.powf(1.4);
        v.signum() * curve * max_speed
    };

    let dx = scaled(lx).round() as i32;
    // gilrs Y axis is positive when pushing up; screen Y is negative when moving up.
    let dy = (-scaled(ly)).round() as i32;

    if dx == 0 && dy == 0 {
        return;
    }

    unsafe {
        let mut pos = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut pos).is_ok() {
            let _ = SetCursorPos(pos.x + dx, pos.y + dy);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn tray_move_mouse_from_stick(_lx: f32, _ly: f32, _speed_scale: f32) {}

#[cfg(target_os = "windows")]
fn tray_left_click() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEINPUT,
    };

    let down = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_LEFTDOWN,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let up = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_LEFTUP,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };

    unsafe {
        let _ = SendInput(&[down, up], std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(not(target_os = "windows"))]
fn tray_left_click() {}

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
    let state = match app.try_state::<RunningGamesState>() {
        Some(s) => s,
        None => return vec![],
    };
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
    let state = app.try_state::<RunningGamesState>().ok_or("State not ready")?;
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
        show_fab_window(&fab).ok();
    }

    Ok(running_games_to_info(&games))
}

#[tauri::command]
fn close_game(app: tauri::AppHandle, program_id: String) -> Vec<RunningGameInfo> {
    let state = match app.try_state::<RunningGamesState>() {
        Some(s) => s,
        None => return vec![],
    };
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

#[tauri::command]
fn save_tray_mouse_settings(app: tauri::AppHandle, speed: f32) -> AppSettings {
    let mut settings = load_settings(&app);
    settings.tray_mouse_speed = clamp_tray_mouse_speed(speed);
    save_settings(&app, &settings);

    if let Some(state) = app.try_state::<TrayMouseConfigState>() {
        let mut cfg = state.config.lock().unwrap();
        cfg.speed = settings.tray_mouse_speed;
    }

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
        let mut tray_lx: f32 = 0.0;
        let mut tray_ly: f32 = 0.0;
        let mut tray_rx: f32 = 0.0;
        let mut tray_ry: f32 = 0.0;
        let deadzone: f32 = 0.5;
        let mut axis_cooldown = std::time::Instant::now();
        
        loop {
            while let Some(event) = gilrs.next_event() {
                match event.event {
                    EventType::ButtonPressed(button, _) => {
                        println!("[gamepad] Button pressed: {:?}", button);
                        log(&format!("[gamepad] Button pressed: {:?}", button));

                        // Toggle tray mouse controls globally.
                        if button == Button::Start {
                            request_toggle_tray_mouse_enabled(&app_handle);
                            continue;
                        }

                        // Home/Guide and Select/Back can return to launcher from an active game.
                        if button == Button::Mode || button == Button::Select {
                            let trigger = if button == Button::Mode { "Home/Guide" } else { "Select/Back" };
                            println!("[gamepad] {} pressed - suspending game and showing launcher", trigger);
                            log(&format!("[gamepad] {} return branch hit", trigger));
                            if is_launcher_active(&app_handle) {
                                // Keep Select available for in-launcher actions.
                                if button == Button::Select {
                                    app_handle.emit("gamepad-button", "Select").ok();
                                }
                                continue;
                            }

                            suspend_game_and_show_launcher(app_handle.clone()).ok();
                            if let Some(state) = app_handle.try_state::<OverlayState>() {
                                state.visible.store(false, Ordering::SeqCst);
                            }
                            continue;
                        }

                        // Do not drive hidden launcher UI while a game has focus.
                        if !should_forward_gamepad_to_ui(&app_handle) {
                            if button == Button::South && is_tray_mouse_control_active(&app_handle) {
                                tray_left_click();
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
                            _ => continue,
                        };
                        app_handle.emit("gamepad-button", button_name).ok();
                    }
                    EventType::AxisChanged(axis, value, _) => {
                        match axis {
                            gilrs::Axis::LeftStickX => tray_lx = value,
                            gilrs::Axis::LeftStickY => tray_ly = value,
                            gilrs::Axis::RightStickX => tray_rx = value,
                            gilrs::Axis::RightStickY => tray_ry = value,
                            _ => {}
                        }

                        if !should_forward_gamepad_to_ui(&app_handle) {
                            match axis {
                                gilrs::Axis::LeftStickX => prev_lx = value,
                                gilrs::Axis::LeftStickY => prev_ly = value,
                                _ => {}
                            }
                            continue;
                        }
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

            if is_tray_mouse_control_active(&app_handle) {
                let tray_speed = current_tray_mouse_speed(&app_handle);
                let left_mag = tray_lx.abs().max(tray_ly.abs());
                let right_mag = tray_rx.abs().max(tray_ry.abs());
                if right_mag > left_mag {
                    tray_move_mouse_from_stick(tray_rx, tray_ry, tray_speed);
                } else {
                    tray_move_mouse_from_stick(tray_lx, tray_ly, tray_speed);
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(16));
        }
    });
}

#[cfg(target_os = "windows")]
fn start_xinput_return_button_thread(app_handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        use windows::Win32::UI::Input::XboxController::{
            XINPUT_GAMEPAD_BUTTON_FLAGS, XINPUT_STATE, XInputGetState,
        };

        const XINPUT_BACK_MASK: XINPUT_GAMEPAD_BUTTON_FLAGS = XINPUT_GAMEPAD_BUTTON_FLAGS(0x0020);
        const XINPUT_A_MASK: XINPUT_GAMEPAD_BUTTON_FLAGS = XINPUT_GAMEPAD_BUTTON_FLAGS(0x1000);
        const XINPUT_START_MASK: XINPUT_GAMEPAD_BUTTON_FLAGS = XINPUT_GAMEPAD_BUTTON_FLAGS(0x0010);
        let mut was_back_down = [false; 4];
        let mut was_a_down = [false; 4];
        let mut was_start_down = [false; 4];
        log("[xinput] return-button polling started");

        loop {
            for user_index in 0..4u32 {
                let idx = user_index as usize;
                let mut state = XINPUT_STATE::default();
                if unsafe { XInputGetState(user_index, &mut state) } == 0 {
                    let start_down = (state.Gamepad.wButtons & XINPUT_START_MASK).0 != 0;
                    if start_down && !was_start_down[idx] {
                        request_toggle_tray_mouse_enabled(&app_handle);
                    }
                    was_start_down[idx] = start_down;

                    if is_tray_mouse_control_active(&app_handle) {
                        let tray_speed = current_tray_mouse_speed(&app_handle);
                        // Polling fallback: keep tray mouse controls responsive even when gilrs
                        // axis/button events don't update while the launcher is hidden.
                        let normalize = |raw: i16| -> f32 {
                            if raw >= 0 {
                                (raw as f32 / 32767.0).clamp(-1.0, 1.0)
                            } else {
                                (raw as f32 / 32768.0).clamp(-1.0, 1.0)
                            }
                        };

                        let lx = normalize(state.Gamepad.sThumbLX);
                        let ly = normalize(state.Gamepad.sThumbLY);
                        let rx = normalize(state.Gamepad.sThumbRX);
                        let ry = normalize(state.Gamepad.sThumbRY);

                        let left_mag = lx.abs().max(ly.abs());
                        let right_mag = rx.abs().max(ry.abs());
                        if right_mag > left_mag {
                            tray_move_mouse_from_stick(rx, ry, tray_speed);
                        } else {
                            tray_move_mouse_from_stick(lx, ly, tray_speed);
                        }

                        let a_down = (state.Gamepad.wButtons & XINPUT_A_MASK).0 != 0;
                        if a_down && !was_a_down[idx] {
                            tray_left_click();
                        }
                        was_a_down[idx] = a_down;
                    } else {
                        was_a_down[idx] = false;
                    }

                    let back_down = (state.Gamepad.wButtons & XINPUT_BACK_MASK).0 != 0;
                    if back_down && !was_back_down[idx] {
                        log(&format!("[xinput] BACK pressed on controller {}", user_index));
                        if is_launcher_active(&app_handle) {
                            app_handle.emit("gamepad-button", "Select").ok();
                        } else {
                            suspend_game_and_show_launcher(app_handle.clone()).ok();
                            if let Some(state) = app_handle.try_state::<OverlayState>() {
                                state.visible.store(false, Ordering::SeqCst);
                            }
                        }
                    }
                    was_back_down[idx] = back_down;
                } else {
                    was_back_down[idx] = false;
                    was_a_down[idx] = false;
                    was_start_down[idx] = false;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
    });
}

#[cfg(not(target_os = "windows"))]
fn start_xinput_return_button_thread(_app_handle: tauri::AppHandle) {}

fn toggle_overlay(app: &tauri::AppHandle, overlay_visible: &Arc<AtomicBool>) {
    let is_visible = overlay_visible.load(Ordering::SeqCst);
    println!("[hotkey] Toggle overlay. Currently visible: {}", is_visible);

    if is_launcher_active(app) {
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
    app.emit("launcher-shown", ()).ok();
    overlay_visible.store(false, Ordering::SeqCst);
}

struct OverlayState {
    visible: Arc<AtomicBool>,
}

pub fn run() {
    // Log panics to file (release builds hide the console)
    std::panic::set_hook(Box::new(|info| {
        log(&format!("PANIC: {}", info));
    }));

    log("=== GameKiosk starting ===");

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            log("setup: entered");

            // IGDB token cache
            app.manage(IgdbTokenCache {
                token: Mutex::new(None),
                http: Client::new(),
            });
            log("setup: igdb token cache created");

            // Running games tracker
            app.manage(RunningGamesState {
                games: std::sync::Mutex::new(HashMap::new()),
            });
            log("setup: running games state created");

            // Overlay visibility state
            let overlay_visible = Arc::new(AtomicBool::new(false));
            app.manage(OverlayState {
                visible: overlay_visible.clone(),
            });
            log("setup: overlay state created");

            // Tray mouse runtime config
            let initial_settings = load_settings(&app.handle().clone());
            app.manage(TrayMouseConfigState {
                config: std::sync::Mutex::new(TrayMouseConfig {
                    speed: initial_settings.tray_mouse_speed,
                    enabled: initial_settings.tray_mouse_enabled,
                }),
            });
            app.manage(TrayMouseToggleState {
                last_toggle_ms: AtomicU64::new(0),
            });
            log(&format!(
                "setup: tray mouse config loaded (speed {:.2}, enabled {})",
                initial_settings.tray_mouse_speed,
                initial_settings.tray_mouse_enabled
            ));

            // Clear window effects (remove Windows 11 rounded corners and backdrop) for FAB and overlay
            if let Some(fab) = app.get_webview_window("fab") {
                log("setup: clearing fab window effects");
                clear_window_effects(&fab);
            } else {
                log("setup: fab window not found");
            }
            if let Some(overlay) = app.get_webview_window("overlay") {
                log("setup: clearing overlay window effects");
                clear_window_effects(&overlay);
            } else {
                log("setup: overlay window not found");
            }

            // Global hotkey: Ctrl+Shift+G toggles overlay (works even when game has focus)
            log("setup: registering global shortcut");
            let app_handle = app.handle().clone();
            let ov = overlay_visible.clone();
            if let Err(e) = app.global_shortcut().on_shortcut("Ctrl+Shift+G", move |_app, _shortcut, _event| {
                toggle_overlay(&app_handle, &ov);
            }) {
                log(&format!("setup: FAILED to register hotkey: {}", e));
            } else {
                log("setup: hotkey registered");
            }

            // System tray
            log("setup: creating system tray");
            let tray_icon = app.default_window_icon().cloned();
            if let Some(icon) = tray_icon {
                log("setup: building tray icon");
                let _tray = TrayIconBuilder::new()
                    .icon(icon)
                    .tooltip("GameKiosk - Ctrl+Shift+G or Home")
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
                log("setup: tray icon built");
            } else {
                log("setup: no default window icon for tray");
            }

            // Gamepad thread for overlay menu navigation (D-pad, A, B)
            log("setup: starting gamepad thread");
            start_gamepad_thread(app.handle().clone());
            log("setup: starting xinput return-button thread");
            start_xinput_return_button_thread(app.handle().clone());

            log("setup: complete");
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
            save_tray_mouse_settings,
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
        .unwrap_or_else(|e| {
            log(&format!("FATAL: tauri app failed to run: {}", e));
        });
    log("=== GameKiosk exited ===");
}
