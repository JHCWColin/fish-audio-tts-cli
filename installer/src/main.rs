#![windows_subsystem = "windows"]

use std::env;
use std::ffi::c_void;
use std::fs;
use std::path::{Path, PathBuf};
use std::ptr;

const INSTALL_DIR_NAME: &str = "FishAudioTTS";
const EMBEDDED_TTS: &[u8] = include_bytes!("../../tts.exe");
const HKEY_CURRENT_USER: *mut c_void = (-2147483647isize) as *mut c_void;
const KEY_QUERY_VALUE: u32 = 0x0001;
const KEY_SET_VALUE: u32 = 0x0002;
const KEY_READ_WRITE: u32 = KEY_QUERY_VALUE | KEY_SET_VALUE;
const REG_EXPAND_SZ: u32 = 2;

#[link(name = "advapi32")]
extern "system" {
    fn RegOpenKeyExW(key: *mut c_void, subkey: *const u16, options: u32, access: u32, result: *mut *mut c_void) -> i32;
    fn RegCreateKeyExW(key: *mut c_void, subkey: *const u16, reserved: u32, class: *const u16, options: u32, access: u32, security: *const c_void, result: *mut *mut c_void, disposition: *mut u32) -> i32;
    fn RegQueryValueExW(key: *mut c_void, value: *const u16, reserved: *mut u32, kind: *mut u32, data: *mut u8, size: *mut u32) -> i32;
    fn RegSetValueExW(key: *mut c_void, value: *const u16, reserved: u32, kind: u32, data: *const u8, size: u32) -> i32;
    fn RegCloseKey(key: *mut c_void) -> i32;
}

#[link(name = "user32")]
extern "system" {
    fn MessageBoxW(hwnd: *mut c_void, text: *const u16, caption: *const u16, typ: u32) -> i32;
}

fn message(title: &str, body: &str, error: bool) {
    let title: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    let body: Vec<u16> = body.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe { MessageBoxW(std::ptr::null_mut(), body.as_ptr(), title.as_ptr(), if error { 0x10 } else { 0x40 }); }
}

fn fail(error: impl AsRef<str>) -> ! {
    message("Fish Audio TTS 安装器", &format!("安装失败：{}", error.as_ref()), true);
    std::process::exit(1);
}

fn install_dir() -> PathBuf {
    env::var_os("LOCALAPPDATA").map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join(INSTALL_DIR_NAME)
}

fn profile_install_dir() -> PathBuf {
    env::var_os("USERPROFILE").map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join(INSTALL_DIR_NAME)
}

fn query_user_path() -> String {
    unsafe {
        let subkey: Vec<u16> = "Environment".encode_utf16().chain(std::iter::once(0)).collect();
        let value_name: Vec<u16> = "Path".encode_utf16().chain(std::iter::once(0)).collect();
        let mut key = ptr::null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_QUERY_VALUE, &mut key) != 0 { return env::var("Path").unwrap_or_default(); }
        let mut kind = 0u32;
        let mut bytes = 0u32;
        let result = RegQueryValueExW(key, value_name.as_ptr(), ptr::null_mut(), &mut kind, ptr::null_mut(), &mut bytes);
        if result != 0 || bytes < 2 { RegCloseKey(key); return env::var("Path").unwrap_or_default(); }
        let mut data = vec![0u8; bytes as usize];
        let result = RegQueryValueExW(key, value_name.as_ptr(), ptr::null_mut(), &mut kind, data.as_mut_ptr(), &mut bytes);
        RegCloseKey(key);
        if result != 0 { return env::var("Path").unwrap_or_default(); }
        let wide: Vec<u16> = data[..bytes as usize].chunks_exact(2).map(|x| u16::from_le_bytes([x[0], x[1]])).take_while(|c| *c != 0).collect();
        String::from_utf16_lossy(&wide)
    }
}

fn same_path(a: &str, b: &Path) -> bool {
    let left = a.trim_end_matches(['\\', '/']).to_ascii_lowercase();
    let right = b.to_string_lossy().trim_end_matches(['\\', '/']).to_ascii_lowercase();
    left == right
}

fn update_path(dir: &Path, remove: bool) -> Result<(), String> {
    let old = query_user_path();
    let mut entries: Vec<String> = old.split(';').filter(|s| !s.trim().is_empty()).map(str::to_string).collect();
    entries.retain(|entry| !same_path(entry, dir));
    if !remove { entries.push(dir.to_string_lossy().to_string()); }
    let new_path = entries.join(";");
    unsafe {
        let subkey: Vec<u16> = "Environment".encode_utf16().chain(std::iter::once(0)).collect();
        let value_name: Vec<u16> = "Path".encode_utf16().chain(std::iter::once(0)).collect();
        let value: Vec<u16> = new_path.encode_utf16().chain(std::iter::once(0)).collect();
        let mut key = ptr::null_mut();
        let mut disposition = 0u32;
        let result = RegCreateKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, ptr::null(), 0, KEY_READ_WRITE, ptr::null(), &mut key, &mut disposition);
        if result != 0 { return Err(format!("无法打开用户环境变量注册表（错误码 {}）", result)); }
        let result = RegSetValueExW(key, value_name.as_ptr(), 0, REG_EXPAND_SZ, value.as_ptr() as *const u8, (value.len() * 2) as u32);
        RegCloseKey(key);
        if result != 0 { return Err(format!("无法写入用户 PATH（错误码 {}）", result)); }
    }
    Ok(())
}

fn install(dir: &Path) -> Result<String, String> {
    fs::create_dir_all(dir).map_err(|e| format!("无法创建安装目录 {}: {}", dir.display(), e))?;
    let exe = dir.join("tts.exe");
    fs::write(&exe, EMBEDDED_TTS).map_err(|e| format!("无法写入 {}: {}", exe.display(), e))?;
    update_path(dir, false)?;
    Ok(format!("已安装 tts.exe 到：{}\n\n安装目录已加入当前用户 PATH。请重新打开终端后使用 tts 命令。", exe.display()))
}

fn uninstall(dir: &Path) -> Result<String, String> {
    update_path(dir, true)?;
    if dir.exists() { fs::remove_dir_all(dir).map_err(|e| format!("无法删除安装目录 {}: {}", dir.display(), e))?; }
    Ok(format!("已从当前用户 PATH 移除：{}", dir.display()))
}

fn main() {
    let dir = install_dir();
    let profile_dir = profile_install_dir();
    let uninstalling = env::args().nth(1).as_deref() == Some("--uninstall");
    let result = if uninstalling {
        let first = uninstall(&dir);
        let second = uninstall(&profile_dir);
        first.and(second)
    } else {
        let first = install(&dir);
        let second = install(&profile_dir);
        first.and(second).map(|_| format!("已安装两份 tts.exe：\n{}\n{}\n\n两个目录都已加入当前用户 PATH。请重新打开终端后使用 tts 命令。", dir.join("tts.exe").display(), profile_dir.join("tts.exe").display()))
    };
    match result {
        Ok(text) => message("Fish Audio TTS 安装器", &text, false),
        Err(error) => fail(error),
    }
}
