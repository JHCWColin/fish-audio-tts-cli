#![cfg_attr(not(windows), allow(dead_code))]

use std::env;
use std::ffi::{c_void, OsStr};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_REFERENCE_ID: &str = "79b9954c15f24deeab2319ec5f44f3b0";
const DEFAULT_MODEL: &str = "s2.1-pro-free";
const DEFAULT_FORMAT: &str = "mp3";
const MAX_RESPONSE_BYTES: usize = 100 * 1024 * 1024;
const WINHTTP_ACCESS_TYPE_NAMED_PROXY: u32 = 3;

const HELP: &str = r#"Fish Audio TTS CLI

用法:
  tts "文本"
  tts "文本" [--reference-id ID] [--format FORMAT] [--model MODEL]
             [--output PATH]
  tts --help | -h | help

用途:
  调用 Fish Audio TTS API，将文本合成为音频文件。成功时 stdout 只输出
  生成文件的绝对路径；失败时将错误写入 stderr 并返回非零退出码。

参数:
  文本                    必填；支持中文、日文、英文及 Unicode。
  --reference-id ID       音色 ID（默认: 79b9954c15f24deeab2319ec5f44f3b0）。
  --format FORMAT         音频格式: mp3、wav、pcm 或 opus（默认: mp3）。
  --model MODEL           模型名称（默认: s2.1-pro-free）。
  --output PATH           输出文件路径；默认保存到当前目录并自动生成文件名。
                          已存在的文件不会被覆盖。
  HTTPS_PROXY             可选 HTTPS 代理，例如 http://127.0.0.1:7890。
  --help, -h, help        显示此帮助，不需要 API Key，也不会联网。

环境变量:
  必须先在 Windows 中设置 FISH_API_KEY，例如：
    $env:FISH_API_KEY = "你的 Fish Audio API Key"
  程序只读取该变量，不会把密钥写入文件或输出到终端。

示例:
  tts "你好"
  tts "你好" --reference-id 79b9954c15f24deeab2319ec5f44f3b0 --format mp3 --model s2.1-pro
  tts "欢迎使用" --output "welcome.mp3"

常见错误:
  缺少或为空的 FISH_API_KEY、空文本、未知参数、无效格式、已有输出文件、
  网络/API 错误都会写入 stderr，并以非零退出码结束。
"#;

#[derive(Debug)]
struct Options {
    text: String,
    reference_id: String,
    format: String,
    model: String,
    output: Option<PathBuf>,
}

fn fail(message: impl AsRef<str>) -> ! {
    eprintln!("错误: {}", message.as_ref());
    std::process::exit(1);
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h" | "help") {
        println!("{}", HELP);
        std::process::exit(0);
    }
    if args.is_empty() {
        return Err("缺少文本参数。使用 tts --help 查看用法。".into());
    }
    if args[0].starts_with('-') || matches!(args[0].as_str(), "help" | "--help" | "-h") {
        return Err("缺少文本参数。文本必须是第一个位置参数。使用 tts --help 查看用法。".into());
    }
    if args[0].trim().is_empty() {
        return Err("文本不能为空或仅包含空白字符。".into());
    }

    let mut result = Options {
        text: args[0].clone(),
        reference_id: DEFAULT_REFERENCE_ID.to_string(),
        format: DEFAULT_FORMAT.to_string(),
        model: DEFAULT_MODEL.to_string(),
        output: None,
    };
    let mut i = 1;
    while i < args.len() {
        let flag = args[i].as_str();
        let value = |index: &mut usize, name: &str| -> Result<String, String> {
            *index += 1;
            if *index >= args.len() || args[*index].starts_with('-') {
                return Err(format!("参数 {} 需要一个值。", name));
            }
            Ok(args[*index].clone())
        };
        match flag {
            "--reference-id" => result.reference_id = value(&mut i, flag)?,
            "--format" => result.format = value(&mut i, flag)?.to_ascii_lowercase(),
            "--model" => result.model = value(&mut i, flag)?,
            "--output" => result.output = Some(PathBuf::from(value(&mut i, flag)?)),
            _ => return Err(format!("未知参数: {}。使用 tts --help 查看支持的参数。", flag)),
        }
        i += 1;
    }
    if !matches!(result.format.as_str(), "mp3" | "wav" | "pcm" | "opus") {
        return Err(format!("无效音频格式: {}。可用格式为 mp3、wav、pcm、opus。", result.format));
    }
    if result.reference_id.trim().is_empty() {
        return Err("--reference-id 不能为空。".into());
    }
    if result.model.trim().is_empty() {
        return Err("--model 不能为空。".into());
    }
    Ok(result)
}

fn absolute_output_path(requested: Option<&Path>, format: &str) -> Result<PathBuf, String> {
    let path = match requested {
        Some(path) if path.as_os_str().is_empty() => return Err("--output 不能为空。".into()),
        Some(path) => {
            if path.is_absolute() { path.to_path_buf() } else { env::current_dir().map_err(|e| format!("无法获取当前目录: {}", e))?.join(path) }
        }
        None => {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| format!("系统时间错误: {}", e))?;
            env::current_dir().map_err(|e| format!("无法获取当前目录: {}", e))?.join(format!("tts_{}_{}.{}", now.as_secs(), now.subsec_nanos(), format))
        }
    };
    Ok(path)
}

fn json_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c <= '\u{1f}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
mod winhttp {
    use super::*;
    type Handle = *mut c_void;
    type Dword = u32;
    type Bool = i32;
    type DwordPtr = usize;
    const WINHTTP_ACCESS_TYPE_DEFAULT_PROXY: Dword = 0;
    const WINHTTP_FLAG_SECURE: Dword = 0x0080_0000;
    const WINHTTP_QUERY_STATUS_CODE: Dword = 19;
    const WINHTTP_QUERY_FLAG_NUMBER: Dword = 0x2000_0000;
    const WINHTTP_NO_PROXY_NAME: *const u16 = ptr::null();
    const WINHTTP_NO_PROXY_BYPASS: *const u16 = ptr::null();
    const WINHTTP_NO_ADDITIONAL_HEADERS: *const u16 = ptr::null();

    #[link(name = "winhttp")]
    extern "system" {
        fn WinHttpOpen(user_agent: *const u16, access_type: Dword, proxy_name: *const u16, proxy_bypass: *const u16, flags: Dword) -> Handle;
        fn WinHttpConnect(session: Handle, server_name: *const u16, server_port: u16, reserved: Dword) -> Handle;
        fn WinHttpOpenRequest(connect: Handle, verb: *const u16, object_name: *const u16, version: *const u16, referrer: *const u16, accept_types: *const *const u16, flags: Dword) -> Handle;
        fn WinHttpSetTimeouts(session: Handle, resolve: i32, connect: i32, send: i32, receive: i32) -> Bool;
        fn WinHttpAddRequestHeaders(request: Handle, headers: *const u16, headers_len: Dword, modifiers: Dword) -> Bool;
        fn WinHttpSendRequest(request: Handle, headers: *const u16, headers_len: Dword, optional: *const c_void, optional_len: Dword, total_len: Dword, context: DwordPtr) -> Bool;
        fn WinHttpReceiveResponse(request: Handle, reserved: *const c_void) -> Bool;
        fn WinHttpQueryHeaders(request: Handle, level: Dword, name: *const u16, buffer: *mut c_void, buffer_len: *mut Dword, index: *mut Dword) -> Bool;
        fn WinHttpQueryDataAvailable(request: Handle, available: *mut Dword) -> Bool;
        fn WinHttpReadData(request: Handle, buffer: *mut c_void, to_read: Dword, read: *mut Dword) -> Bool;
        fn WinHttpCloseHandle(handle: Handle) -> Bool;
        fn GetLastError() -> Dword;
    }

    struct H(Handle);
    impl Drop for H { fn drop(&mut self) { if !self.0.is_null() { unsafe { WinHttpCloseHandle(self.0); } } } }

    fn last_error(action: &str) -> String { format!("{} (Windows 错误码 {})", action, unsafe { GetLastError() }) }

    pub fn post(api_key: &str, model: &str, body: &[u8]) -> Result<(u32, Vec<u8>), String> {
        unsafe {
            let agent = wide("fish-audio-tts/1.0");
            let proxy_value = env::var("HTTPS_PROXY").ok().filter(|v| !v.trim().is_empty())
                .or_else(|| env::var("HTTP_PROXY").ok().filter(|v| !v.trim().is_empty()));
            let proxy_wide = proxy_value.as_deref().map(wide);
            let (access_type, proxy_name) = match proxy_wide.as_ref() {
                Some(proxy) => (WINHTTP_ACCESS_TYPE_NAMED_PROXY, proxy.as_ptr()),
                None => (WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_NO_PROXY_NAME),
            };
            let session = H(WinHttpOpen(agent.as_ptr(), access_type, proxy_name, WINHTTP_NO_PROXY_BYPASS, 0));
            if session.0.is_null() { return Err(last_error("无法初始化 HTTPS 客户端")); }
            if WinHttpSetTimeouts(session.0, 15_000, 15_000, 30_000, 60_000) == 0 { return Err(last_error("无法设置网络超时")); }
            let host = wide("api.fish.audio");
            let connect = H(WinHttpConnect(session.0, host.as_ptr(), 443, 0));
            if connect.0.is_null() { return Err(last_error("无法连接 Fish Audio")); }
            let verb = wide("POST");
            let object = wide("/v1/tts");
            let request = H(WinHttpOpenRequest(connect.0, verb.as_ptr(), object.as_ptr(), ptr::null(), ptr::null(), ptr::null(), WINHTTP_FLAG_SECURE));
            if request.0.is_null() { return Err(last_error("无法创建 HTTPS 请求")); }
            let headers = wide(format!("Authorization: Bearer {}\r\nContent-Type: application/json\r\nmodel: {}\r\n", api_key, model));
            if WinHttpAddRequestHeaders(request.0, headers.as_ptr(), u32::MAX, 0) == 0 { return Err(last_error("无法设置请求头")); }
            if WinHttpSendRequest(request.0, WINHTTP_NO_ADDITIONAL_HEADERS, 0, body.as_ptr() as *const c_void, body.len() as u32, body.len() as u32, 0) == 0 { return Err(last_error("发送 TTS 请求失败")); }
            if WinHttpReceiveResponse(request.0, ptr::null()) == 0 { return Err(last_error("接收 TTS 响应失败")); }
            let mut status = 0u32;
            let mut status_len = std::mem::size_of::<u32>() as u32;
            if WinHttpQueryHeaders(request.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, ptr::null(), &mut status as *mut _ as *mut c_void, &mut status_len, ptr::null_mut()) == 0 { return Err(last_error("读取 HTTP 状态失败")); }
            let mut response = Vec::new();
            loop {
                let mut available = 0u32;
                if WinHttpQueryDataAvailable(request.0, &mut available) == 0 { return Err(last_error("读取 API 响应大小失败")); }
                if available == 0 { break; }
                if response.len().saturating_add(available as usize) > MAX_RESPONSE_BYTES { return Err("API 响应超过 100 MB 限制。".into()); }
                let old_len = response.len();
                response.resize(old_len + available as usize, 0);
                let mut read = 0u32;
                if WinHttpReadData(request.0, response[old_len..].as_mut_ptr() as *mut c_void, available, &mut read) == 0 { return Err(last_error("读取 API 响应失败")); }
                response.truncate(old_len + read as usize);
                if read == 0 { break; }
            }
            Ok((status, response))
        }
    }
}

fn api_error(status: u32, body: &[u8], secret: &str) -> String {
    let detail = String::from_utf8_lossy(body);
    let detail = detail.trim();
    if detail.is_empty() { format!("Fish Audio API 返回 HTTP {}。", status) }
    else {
        let safe_detail = detail.replace(secret, "[已隐藏]");
        format!("Fish Audio API 返回 HTTP {}: {}", status, safe_detail.chars().take(500).collect::<String>())
    }
}

fn save_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent).map_err(|e| format!("无法创建输出目录 {}: {}", parent.display(), e))?;
        }
    }
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)
        .map_err(|e| if e.kind() == io::ErrorKind::AlreadyExists { format!("输出文件已存在，拒绝覆盖: {}", path.display()) } else { format!("无法创建输出文件 {}: {}", path.display(), e) })?;
    if let Err(e) = file.write_all(bytes).and_then(|_| file.flush()) {
        let _ = fs::remove_file(path);
        return Err(format!("写入输出文件失败: {}", e));
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let options = parse_args(&args).unwrap_or_else(|e| fail(e));
    let output = absolute_output_path(options.output.as_deref(), &options.format).unwrap_or_else(|e| fail(e));
    let api_key = env::var("FISH_API_KEY").ok().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| fail("未设置有效的 FISH_API_KEY 环境变量。请先在 Windows 中设置该变量。"));
    if options.output.is_some() && output.exists() { fail(format!("输出文件已存在，拒绝覆盖: {}", output.display())); }
    let request = format!("{{\"text\":\"{}\",\"reference_id\":\"{}\",\"format\":\"{}\"}}", json_escape(&options.text), json_escape(&options.reference_id), json_escape(&options.format));
    let (status, response) = winhttp::post(&api_key, &options.model, request.as_bytes()).unwrap_or_else(|e| fail(e));
    if !(200..300).contains(&status) { fail(api_error(status, &response, &api_key)); }
    if response.is_empty() { fail("Fish Audio API 返回了空响应，未生成音频文件。" ); }
    let first = response.iter().copied().find(|b| !b.is_ascii_whitespace());
    if matches!(first, Some(b'{') | Some(b'[')) { fail("Fish Audio API 返回了 JSON 内容而非音频数据。" ); }
    save_new(&output, &response).unwrap_or_else(|e| fail(e));
    let absolute = fs::canonicalize(&output).unwrap_or(output);
    let printed = absolute.to_string_lossy();
    if let Some(normal) = printed.strip_prefix(r"\\?\UNC\") {
        println!(r"\\{}", normal);
    } else if let Some(normal) = printed.strip_prefix(r"\\?\") {
        println!("{}", normal);
    } else {
        println!("{}", printed);
    }
}
