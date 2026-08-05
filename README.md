<div align="center">

# VoxClaw

**The Next-Generation AI Voice Assistant & Input Method for Windows**

*Hold to Speak, Release to Type. Fast, Global, and Context-Aware.*

[![Platform](https://img.shields.io/badge/Platform-Windows-blue.svg?style=for-the-badge&logo=windows)](https://github.com/guxiangjun/VoxClaw/releases)
[![Rust](https://img.shields.io/badge/Built_with-Rust-f46623.svg?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![Tauri](https://img.shields.io/badge/Powered_by-Tauri-FFC131.svg?style=for-the-badge&logo=tauri)](https://tauri.app/)
[![License: MIT](https://img.shields.io/badge/License-MIT-success.svg?style=for-the-badge)](https://opensource.org/licenses/MIT)

[**English**](./README.md) • [**简体中文**](./README_zh.md)

</div>

---

## ⚡ Why VoxClaw?

Traditional dictation tools simply transcribe words. **VoxClaw** is built differently. It combines blazing-fast online streaming ASR with intelligent LLM post-correction to ensure what you speak isn't just transcribed, but perfectly formatted for the context. 

Whether you are writing code, sending emails, or triggering system skills, VoxClaw is your invisible, always-ready co-pilot.


## ✨ Features

- 🎙️ **Dual-Engine Recognition:** Real-time online ASR with live partial transcript previews, plus a brand new **Offline Mode** powered by local SenseVoice models for 100% privacy and offline usage.
- ⌨️ **Global Input:** Write directly into any native application, chat, browser, IDE, or system dialog using intelligent clipboard injection (`Ctrl+V`) and a low-latency Windows API raw input listener.
- 🤖 **Autonomous AI Agent:** A powerful ReAct agent that can browse the web, edit files, run commands, and control your system through voice.
- 📅 **Meeting Management:** Advanced meeting management features with session handling to streamline your daily workflows.
- 🎯 **Triple Modes:**
  - **Dictation Mode:** Transcribe and type text seamlessly with optional LLM polishing.
  - **Agent Mode:** Invoke an autonomous agent to perform complex multi-step tasks (e.g., "Find the latest news about Rust and summarize it in a new file").
  - **Skills Mode:** Quick voice commands for specific system actions.
- 🎧 **Audio Robustness:** Advanced audio processing module with auto-recovery for audio devices on disconnect or sleep mode via device watcher.
- 🎨 **Tasteful UI:** A gorgeous, unobtrusive glassmorphism indicator that floats intelligently on your screen.

---

## 🚀 Quick Start (Under 1 Minute)

### 1. Download & Install
Grab the latest Windows Installer (`.exe`) from our [Releases Page](https://github.com/guxiangjun/VoxClaw/releases).

### 2. Configure ASR
Launch VoxClaw, open **Settings**, and paste your Online ASR credentials.  
*(Settings -> Audio Input -> Choose your microphone and Test)*

### 3. Hold and Speak!
We highly recommend starting with the **Mouse Middle Button**:

| Mode | Trigger | Action |
| --- | --- | --- |
| 📝 **Dictation** | `Middle Mouse Button` (Hold) | Hold to speak, release to type exactly where your cursor is. |
| 📝 **Dictation** | `Right Alt` (Toggle) | Press once to start, press again to complete. |
| ⚡ **Skills** | `Ctrl + Win` (Hold) | Speak a command (e.g., "Open Calculator") and release. |

---

## 📦 Offline Mode (SenseVoice) — Runtime DLL Setup

The Offline Mode powered by the local SenseVoice model requires **Microsoft's ONNX Runtime** to run inference. VoxClaw does **not** bundle this DLL (to avoid antivirus false positives and to keep the installer small), so on first use you must download and place `onnxruntime.dll` yourself.

> ℹ️ If you skip this step, VoxClaw will fall back to whatever `onnxruntime.dll` it finds on your machine (typically from a Python install) — which is very often an **incompatible version**, causing the error `ort ... is not compatible with the ONNX Runtime binary`.

### Step 1 — Download the correct DLL

Grab the official **onnxruntime-win-x64-1.23.0** package from Microsoft (CPU build):

```
https://github.com/microsoft/onnxruntime/releases/download/v1.23.0/onnxruntime-win-x64-1.23.0.zip
```

> ⚠️ **Version requirement:** the bundled `ort` crate (v2.0.0-rc.11) requires ONNX Runtime **`>= 1.23.x`**. Do **not** use 1.20.x — it is the version most users hit the incompatibility error with.

Unzip and locate `lib\onnxruntime.dll` — that single file is all you need.

### Step 2 — Place the DLL (pick one)

VoxClaw searches these locations in order; use whichever fits your setup:

| Priority | Location | Notes |
| --- | --- | --- |
| 1 | Next to `voxclaw.exe` | **Recommended.** Right-click the desktop shortcut → "Open file location", drop the DLL there. Fully isolates from Python. |
| 2 | Env var `SONICCLAW_ONNXRUNTIME_DLL` | Set to the full path of the DLL. Requires app restart. |
| 3 | Python site-packages | **Not recommended.** May be overwritten by `pip`; version can drift. Last-resort fallback only. |

### Step 3 — Restart VoxClaw

The ONNX Runtime is loaded **once per process** and frozen for the lifetime of the process. You **must fully quit and reopen VoxClaw** for the new DLL to take effect — re-clicking "Switch Model" inside a running process will keep using the old one.

### Verifying it works

Open **Settings → switch to SenseVoice → Switch Model**. You should no longer see the `is not compatible` error. If you still do, double-check the DLL is in one of the locations above and that the app was restarted.

### One-line PowerShell fix

For a default install location, save as `fix-ort.ps1` and run:

```powershell
$ErrorActionPreference = "Stop"
Invoke-WebRequest -Uri "https://github.com/microsoft/onnxruntime/releases/download/v1.23.0/onnxruntime-win-x64-1.23.0.zip" -OutFile "$env:TEMP\ort.zip"
Expand-Archive -Path "$env:TEMP\ort.zip" -DestinationPath "$env:TEMP\ort" -Force
Copy-Item "$env:TEMP\ort\onnxruntime-win-x64-1.23.0\lib\onnxruntime.dll" -Destination "$env:LOCALAPPDATA\voxclaw\onnxruntime.dll" -Force
Write-Host "Done. Restart VoxClaw now."
```

> 💡 Replace `$env:LOCALAPPDATA\voxclaw` with your actual install path if different.

> ⚠️ **About GPU:** this CPU-only DLL does **not** include the DirectML provider. If `use_gpu` is enabled in Settings, DirectML initialization will fail and silently fall back to CPU — dictation still works, just without GPU acceleration.

---

## 🧠 Smart AI Scenes (LLM Profiles)

Take your dictation to the next level. Connect an OpenAI-compatible API to enable **Smart Scenes**. 
Instead of a one-size-fits-all prompt, you can define specific scenes:
- *Code Documentation*: Formats your speech into perfect markdown comments.
- *Executive Emails*: Adjusts to a professional, concise tone.
- *Casual Chat*: Keeps it light and natural.

---

## 🛠️ Tech Stack

VoxClaw is crafted with modern, high-performance technologies:

- **Core & Desktop**: `Rust` + `Tauri v2` *(Ultra-low memory footprint)*
- **Frontend UI**: `React` + `TypeScript` + `TailwindCSS` *(Tasteful, fluid animations)*
- **Audio Capture**: `cpal`
- **Global Input**: Native Win32 Raw Input (Zero-Hook) + Native FFI `SendInput` *(High performance, minimal footprint)*

---

## 🏗️ Development & Build

Ensure you have **Node.js 18+** and **Rust** installed on Windows 10/11.

```bash
# Install UI dependencies
pnpm install

# Run in Development mode
pnpm tauri dev

# Build the Windows Installer (NSIS)
pnpm tauri build
```
*Build artifacts will be located in `src-tauri/target/release/bundle/`.*

---

## 🔒 Privacy First

Your voice, your rules.
- **ASR Data:** Sent directly and only to the ASR provider you explicitly configure.
- **LLM Data:** Transcribed text is only sent to your LLM endpoint if the feature is enabled.
- **Local Control:** Everything is stored locally in `%APPDATA%\com.voxclaw\`.
- **System Safety:** By using native Win32 APIs instead of global hooks, VoxClaw maintains a clean security profile and avoids common antivirus false positives.

---

## 🤝 Contributing

We welcome pull requests! If you find a bug or want to suggest a feature:
1. Please include your trigger method, target application, and reproduction steps in the Issue.
2. Keep your PRs focused and minimalist. We value maintainability and clean code.

## 📄 License

This project is licensed under the [MIT License](LICENSE).

<div align="center">
  <i>Built with ❤️ for Windows Power Users.</i>
</div>
