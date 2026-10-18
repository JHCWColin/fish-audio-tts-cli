# Fish Audio TTS CLI for Windows

这是一个面向 AI Agent 的 Windows 独立命令行工具。发布文件为仓库根目录的 `tts.exe`，使用 Rust 标准库和 Windows 原生 WinHTTP API，不依赖 Node.js 或其他需要用户另行安装的运行时。

## 使用

先在 Windows 中设置 API Key（程序只读取，不会修改或打印它）：

```powershell
setx FISH_API_KEY "你的 Fish Audio API Key"
```

设置后请打开新的终端窗口，然后运行：

```powershell
.\tts.exe "你好"
.\tts.exe "你好" --reference-id "79b9954c15f24deeab2319ec5f44f3b0" --format mp3 --model s2.1-pro
.\tts.exe "欢迎使用" --output "welcome.mp3"
.\tts.exe --help
```

成功时 stdout 只包含生成文件的绝对路径；失败信息写入 stderr 并返回非零退出码。默认音色为 `79b9954c15f24deeab2319ec5f44f3b0`，默认模型为 `s2.1-pro-free`，默认格式为 `mp3`。支持 `mp3`、`wav`、`pcm` 和 `opus`。输出文件已存在时拒绝覆盖；未指定输出路径时会在当前目录生成唯一文件名。

如果网络环境需要本地代理，可在启动前设置 `HTTPS_PROXY`（或 `HTTP_PROXY`），例如：

```powershell
$env:HTTPS_PROXY = "http://127.0.0.1:7890"
.\tts.exe "你好"
```

未设置代理变量时程序使用 Windows 默认网络配置。

## 安装包

下载 Release 中的 `tts-setup.exe` 并双击运行。它是 Windows GUI 安装器，不会打开控制台窗口；安装完成或失败会显示原生提示框。安装器会同时把 `tts.exe` 安装到 `%LOCALAPPDATA%\FishAudioTTS` 和 `%USERPROFILE%\FishAudioTTS`（即 `C:\Users\<用户名>\FishAudioTTS`），并将两个目录加入当前用户的 `PATH`；打开新的终端后即可直接运行：

```powershell
tts "你好"
```

安装器支持 `tts-setup.exe --uninstall`，用于移除安装目录并从当前用户 `PATH` 删除该项。

## 构建

需要安装 Rust 的 `x86_64-pc-windows-msvc` 工具链：

```powershell
cargo build --release
Copy-Item .\target\release\tts.exe .\tts.exe -Force
```

构建产物是单个 Windows PE 可执行文件。程序使用 HTTPS、15 秒 DNS/连接超时、30 秒发送超时和 60 秒接收超时；仅在收到 2xx 且响应非空、非 JSON 错误内容时写入音频文件。

## 本次验证

- `tts --help`、`tts -h`、`tts help`：退出码 0，无需 API Key 或网络。
- 空文本、未知参数、无效格式：均输出 stderr 并退出码 1。
- 缺少或全空格 `FISH_API_KEY`：输出明确错误并退出码 1。
- 已存在的 `--output` 文件：拒绝覆盖并退出码 1。
- 使用当前 Windows 用户环境变量中的真实 Key 发起了真实 Fish Audio 请求；本机连接阶段返回 WinHTTP 12002（超时），因此未生成文件，也未将失败响应误保存为音频。
