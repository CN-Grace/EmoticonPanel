# 表情面板 — egui-lite 内存优化版 (main 分支)

本分支为**现行交付**: egui 纯 Rust **内存优化版** (`egui-app-lite/`)。

- 内存: **~82MB WS / ~88MB 私有** (Tauri 版 ~465MB · egui 原版 ~120/250)
- 优化: 72px 缩略图 · GIF 动画按需解码 + LRU · GB2312 中文字体子集(2MB 内嵌) · 动画优先双队列
- 功能: 4列网格(75px+文件名) / 分组Tab(滚轮横滑/右键删除) / ⚙设置(目标窗口/刷新/位置) / 悬浮GIF原位动画 / 点击 CF_HDROP+Ctrl+V 插入目标窗口
- 数据: `%APPDATA%\EmoticonPanel-egui\stickers` (可⚙改位置)

```bash
cd egui-app-lite && cargo build --release
# → egui-app-lite/target/release/emoticon-panel-lite.exe
```

> 其他实现见分支: `egui` (功能完备版) / `tauri` (Tauri/WebView2 版)
> exe 归档: `H:\VibeCoding\EmoticonPanel-archives\` (git 外)
