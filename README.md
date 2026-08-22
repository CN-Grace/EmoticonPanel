# 表情面板 EmoticonPanel — Tauri 版

> 仿微信表情面板的桌面小工具:**点一下表情,自动粘贴进你想发送的窗口**(微信/QQ/任意聊天输入框)。

本分支 = **Tauri 2 + Rust + vanilla-ts**(`src-tauri/`),最早实现。

---

## ✨ 功能特性

| 能力 | 说明 |
|---|---|
| 🗂️ 表情包分组 | 每个文件夹 = 一个分组,png / gif 混装,`cover.*` 作 Tab 封面 |
| 🖼️ 网格浏览 | 4 列网格 + 文件名,上下滚动 |
| 📌 目标窗口附件 | 设置 → 目标窗口 → 点选任意窗口,绑定后点击表情即插入 |
| 📋 原生剪贴板注入 | CF_HDROP 真实文件路径 + Ctrl+V,透明/动图原样保留 |
| ⚙ 设置 | 目标窗口选择 / 刷新 / 表情包位置(持久化 settings.json) |
| 🚀 WebView2 体验 | 前端 CSS 完美还原微信风格 |

## 🧰 技术栈

- **Tauri 2**(Rust 后端)+ **vanilla TypeScript / Vite** 前端
- WebView2(Windows 系统级运行时,多应用共享)
- Rust 侧:窗口枚举 / 前台激活 / 剪贴板(CF_HDROP + 虚拟文件)/ 模拟 Ctrl+V

## 🏗️ 与 egui 版差异

| 维度 | Tauri 版(本分支) | egui-lite 版 (main 分支) |
|---|---|---|
| 渲染内核 | WebView2(浏览器内核) | egui 自绘(无内核) |
| 进程数 | 7(主 + 6 × WebView2) | 1 |
| 内存(WS) | ~465MB | **~82MB** |
| exe 大小 | 29MB | 6.8MB |
| 视觉还原 | CSS 完美还原微信 | 自绘近似 |

**本版优势**:前端自由度高、UI 像素级还原;代价是 WebView2 内核常驻内存。

## 🚀 快速开始

```bash
# 前置: Rust 工具链 + Node.js
npm install
npm run tauri dev                 # 开发模式(热重载)
npm run tauri build -- --no-bundle  # 构建 → src-tauri/target/release/emoticon-panel.exe
```

1. 运行,把表情包文件夹放进 `%APPDATA%\com.grace.emoticonpanel\stickers\`
2. 设置 → 目标窗口 → 点选微信/聊天窗口
3. 点击表情 → 自动粘贴

## 🔀 分支说明

| 分支 | 实现 | 内存(WS) |
|---|---|---|
| `main` | egui-lite 内存优化版 | ~82MB |
| `egui` | egui 功能完备版 | ~120MB |
| **`tauri`(本分支)** | Tauri 2 + WebView2 | ~465MB |

```bash
git checkout main / git checkout egui   # 切换(切换会清掉忽略的 target/, 需重新构建)
```

exe 归档在仓库外 `H:\VibeCoding\EmoticonPanel-archives\`。

## 📄 License

MIT。
