use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const INSTALL_DIR_NAME: &str = "FishAudioTTS";
const EMBEDDED_TTS: &[u8] = include_bytes!("../../tts.exe");

fn fail(message: impl AsRef<str>) -> ! {
    eprintln!("安装失败: {}", message.as_ref());
    std::process::exit(1);
}

fn install_dir() -> PathBuf {
    env::var_os("LOCALAPPDATA").map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join(INSTALL_DIR_NAME)
}

fn query_user_path() -> String {
    let output = Command::new("reg.exe")
        .args(["query", r"HKCU\Environment", "/v", "Path"])
        .output();
    if let Ok(output) = output {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if let Some(pos) = line.find("REG_") {
                let value = line[pos..].split_once(char::is_whitespace).map(|(_, rest)| rest.trim()).unwrap_or("");
                if !value.is_empty() { return value.to_string(); }
            }
        }
    }
    env::var("Path").unwrap_or_default()
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
    let status = Command::new("reg.exe")
        .args(["add", r"HKCU\Environment", "/v", "Path", "/t", "REG_EXPAND_SZ", "/d", &new_path, "/f"])
        .status().map_err(|e| format!("无法更新用户 PATH: {}", e))?;
    if !status.success() { return Err(format!("reg.exe 更新用户 PATH 失败，退出码 {}", status)); }
    Ok(())
}

fn install(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("无法创建安装目录 {}: {}", dir.display(), e))?;
    let exe = dir.join("tts.exe");
    fs::write(&exe, EMBEDDED_TTS).map_err(|e| format!("无法写入 {}: {}", exe.display(), e))?;
    update_path(dir, false)?;
    println!("已安装 tts.exe 到 {}", exe.display());
    println!("已将安装目录加入当前用户 PATH。请重新打开终端后使用 tts 命令。");
    Ok(())
}

fn uninstall(dir: &Path) -> Result<(), String> {
    update_path(dir, true)?;
    if dir.exists() { fs::remove_dir_all(dir).map_err(|e| format!("无法删除安装目录 {}: {}", dir.display(), e))?; }
    println!("已从当前用户 PATH 移除 {}", dir.display());
    Ok(())
}

fn main() {
    let dir = install_dir();
    let result = if env::args().nth(1).as_deref() == Some("--uninstall") { uninstall(&dir) } else { install(&dir) };
    if let Err(e) = result { fail(e); }
}
