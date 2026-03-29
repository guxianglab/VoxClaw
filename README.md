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

- 🎙️ **Streaming Recognition:** Real-time online ASR with live partial transcript previews.
- ⌨️ **Global Input:** Write directly into any native application, chat, browser, IDE, or system dialog using intelligent clipboard injection (`Ctrl+V`).
- 🤖 **LLM Post-Correction (Optional):** Fixes homophones, missing words, and formats your spoken intent into written prose without altering the meaning.
- 🎯 **Dual Modes:**
  - **Dictation Mode:** Transcribe and type text seamlessly.
  - **Skills Mode:** Use voice real-time to execute commands (e.g., Open Calculator, Take Screenshot).
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
- **Global Input**: `rdev` + `enigo`

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
- **Local Control:** Everything is stored locally in `%APPDATA%\com.voxclaw\`. You can wipe your history at any time.

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
