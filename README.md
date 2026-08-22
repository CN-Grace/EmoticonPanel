# 表情面板 — Tauri 版分支

本分支仅含 **Tauri 2 + Rust + vanilla-ts** 实现 (`src-tauri/`)。

- 功能: 表情网格 / 分组 Tab / 目标窗口附件 / 剪贴板插入
- 内存: ~465MB (WebView2 内核)
- 构建: `npm install && npm run tauri build -- --no-bundle` → `src-tauri/target/release/emoticon-panel.exe`

> 其他实现见分支: `main` (egui-lite 优化版) / `egui` (egui 功能完备版)
