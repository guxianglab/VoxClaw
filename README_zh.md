<div align="center">

# VoxClaw

**下一代 Windows 全局 AI 语音助手与输入法**

*按住说话，松开上屏。极速、全局、懂你的上下文。*

[![Platform](https://img.shields.io/badge/Platform-Windows-blue.svg?style=for-the-badge&logo=windows)](https://github.com/guxiangjun/VoxClaw/releases)
[![Rust](https://img.shields.io/badge/Built_with-Rust-f46623.svg?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![Tauri](https://img.shields.io/badge/Powered_by-Tauri-FFC131.svg?style=for-the-badge&logo=tauri)](https://tauri.app/)
[![License: MIT](https://img.shields.io/badge/License-MIT-success.svg?style=for-the-badge)](https://opensource.org/licenses/MIT)

[**English**](./README.md) • [**简体中文**](./README_zh.md)

</div>

---

## ⚡ 为什么选择 VoxClaw？

传统的语音输入软件往往只是单纯的“把声音转成文字”。**VoxClaw** 则完全不同。它将 **极速在线流式 ASR** 与 **智能大语言模型（LLM）纠错** 完美结合，确保你口述的内容不仅精准识别，更能根据上下文自动转换为书面语言。

无论你是在敲代码、写周报、还是调用系统快捷指令，VoxClaw 都是你桌面端最优雅、如影随形的隐形副驾驶（Co-pilot）。

## ✨ 核心特性

- 🎙️ **流式极速识别:** 基于顶级在线 ASR 的实时识别，支持边说边看预览。
- ⌨️ **全局沉浸输入:** 通过底层的剪贴板注入 (`Ctrl+V`)，完美适配微信、浏览器、IDE、甚至是系统的任何输入框。
- 🤖 **大模型润色纠错 (可选):** 专治同音字错漏字！自动修正语病，不仅能“听懂”还能“润色”，且绝不改变原意。
- 🎯 **双擎驱动模式:**
  - **听写模式 (Dictation):** 高效记录，行云流水般上屏。
  - **指令模式 (Skills):** 语音成为系统捷径（例如：说“打开计算器”、“截个图”并瞬间响应）。
- 🎨 **极致优雅的 UI:** 为桌面美学打造的悬浮毛玻璃胶囊，极简、克制且充满交互细节。

---

## 🚀 30秒极速上手

### 1. 下载与安装
前往 [Releases 页面](https://github.com/guxiangjun/VoxClaw/releases) 下载最新的 Windows 安装包（`.exe`）。

### 2. 配置 ASR 引擎
首次启动后打开 **Settings (设置)**，填入你的在线 ASR 凭证。
*(你可以在 Settings -> Audio Input 测试麦克风是否正常收音)*

### 3. 按住并开始你的讲述！
我们极其推荐你使用 **鼠标中键** 作为主力触发方式：

| 模式 | 触发键位 | 交互逻辑 |
| --- | --- | --- |
| 📝 **听写功能** | `鼠标中键` (长按) | 长按开始说话，松手即刻上屏到光标处。 |
| 📝 **听写功能** | `右侧 Alt` (单点) | 点击一次开始，再点击一次结束并上屏。 |
| ⚡ **语音指令** | `Ctrl + Win` (长按) | 说出指令（如“截个图”），松开立即执行。 |

---

## 🧠 智能体场景 (LLM Profiles)

接入任何兼容 OpenAI 格式的 API，将听写拉升至“智能体”层级。
你不再受限于一套生硬的提示词，而是能为不同场景自由建立 Profile：
- **会议纪要**：提取核心要点，条理清晰。
- **商务沟通**：转换你的口语为极其专业的邮件措辞。
- **写代码/注释**：说大白话，自动生成高质量 Markdown 注释。

---

## 🛠️ 技术栈与极致性能

我们深知桌面软件“性能即正义”，采用纯血现代技术栈打造：

- **核心层 & 运行时**: `Rust` + `Tauri v2` (极低的后台常驻内存占用)
- **前端动画层**: `React` + `TypeScript` + `TailwindCSS` (丝滑交互与现代美学)
- **音频捕获**: `cpal`
- **全局事件模拟**: `rdev` + `enigo`

---

## 🏗️ 开发者指南

请确保你的 Windows 环境下已安装 **Node.js 18+** 与 **Rust 最新稳定版**。

```bash
# 安装依赖
pnpm install

# 启动开发调试模式
pnpm tauri dev

# 打包构建 Windows NSIS 安装包
pnpm tauri build
```
*构建产物将输出至: `src-tauri/target/release/bundle/`。*

---

## 🔒 隐私与数据安全

你的声音，你做主。
- **关于语音数据:** 音频仅会流式发送给**你在配置页主动作出选择的 ASR 平台**。
- **关于模型数据:** 只有当你手动打开 LLM 纠错开关，识别出来的**文本**才会被发给你配置的个人 LLM API。
- **本地存储:** 所有配置文件和历史记录都储存在本地的 `%APPDATA%\com.voxclaw\`，你可以随时一键清空。

---

## 🤝 参与贡献

欢迎提交 Issue 和 Pull Requests！如果您想反馈问题：
1. 请务必在描述中附带：触发方式、所交互的目标软件、复现步骤及可能的截图。
2. 提交 PR 时，请尽量让改动保持内聚，我们极其看重代码的可维护性，避免引入复杂的冗余状态。

## 📄 开源许可证

本项目基于 [MIT License](LICENSE) 许可协议开源。

<div align="center">
  <i>为 Windows 硬核玩家用 ❤️ 打造。</i>
</div>
