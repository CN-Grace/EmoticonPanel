# 表情面板 EmoticonPanel — egui-lite 内存优化版

> 仿微信表情面板的桌面小工具:**点一下表情,自动粘贴进你想发送的窗口**(微信/QQ/任意聊天输入框)。
> egui 纯 Rust 实现,无浏览器内核,内存占用仅为 Tauri 版的 ~1/6。

`main` 分支 = **内存优化版(现行交付)**。其他实现见 [分支说明](#-分支说明)。

---

## ✨ 功能特性

| 能力 | 说明 |
|---|---|
| 🗂️ 表情包分组 | 每个文件夹 = 一个分组,支持 png / gif / jpg / webp / bmp 混装,`cover.*` 作 Tab 封面 |
| 🖼️ 4 列网格 | 75px 预览 + 文件名,上下滚动浏览,不填满页面,默认窗口 318×445 |
| 🎞️ GIF 原位动画 | 悬浮在动图上**原地播放动画**(真实帧节奏),移开回静态首帧,无浮层 |
| 📌 目标窗口附件 | ⚙ → 目标窗口 → 点选任意窗口(微信等),绑定后点击表情即插入 |
| 📋 原生剪贴板注入 | **CF_HDROP 真实文件路径** + Ctrl+V,透明/动图原样保留(微信识别为原文件) |
| 🗂️ Tab 横滑 | 底部分组栏支持鼠标滚轮横向滑动;右键删除分组 |
| ⚙ 设置面板 | 目标窗口选择 / 刷新表情包 / **任意文件夹作为表情包根目录**(持久化) |
| 🌍 全局数据位置 | 根目录优先级:`EMOTICON_STICKERS_DIR` 环境变量 > 设置面板选择 > 默认 appdata |
| 🚀 轻量 | 单 exe 6.8MB(release),内存 ~82MB WS / ~88MB 私有,1 个进程 |

---

## 🧰 技术栈

- **Rust** + [egui/eframe](https://github.com/emilk/egui)(即时模式 GUI,软件/GPU 渲染)
- [image](https://crates.io/crates/image):图片解码(png/gif/jpg/webp/bmp)+ GIF 逐帧
- [winapi](https://crates.io/crates/winapi):窗口枚举 / 前台激活 / 剪贴板(CF_HDROP + 虚拟文件)/ 模拟 Ctrl+V
- **GB2312 中文字体子集**(2MB,fonttools 生成,内嵌 `assets/cjk-subset.ttf`)替代 19MB simhei

## 🏗️ 内存优化设计(lite 版)

| 手段 | 效果 |
|---|---|
| 缩略图解码 72px(与显示同尺寸) | 纹理内存 -44% |
| GIF 动画帧**按需解码**(悬浮才解)+ LRU 24 逐出 | 常态只常驻静态首帧 |
| 动画/静态**双队列**,动画优先,队列满主线程同步兜底 | 悬浮必出动画,不被静态洪水饿死 |
| GB2312 中文字体子集内嵌 | 常驻字体 19MB → 2MB |
| 3 worker + bounded 队列 | 解码峰值可控 |

**实测稳态(385 组表情包)**:WS **~82MB** · 私有 **~88MB**
Tauri 版 ~465MB · egui 原版 ~120/250MB

---

## 🚀 快速开始

### 构建(Windows)

```bash
# 前置: Rust 工具链(msvc 或 gnu 均可)
cd egui-app-lite
cargo build --release
# → egui-app-lite/target/release/emoticon-panel-lite.exe (单文件, 绿色运行)
```

> 不需要安装任何运行时 — 纯 Rust,静态链接,双击即用。

### 使用

1. 双击运行,首次会显示表情面板
2. **放表情包**:把文件夹丢进 `%APPDATA%\EmoticonPanel-egui\stickers\`(每个子文件夹 = 一个分组)
   - 不改默认位置也可以:⚙ → **表情包位置** → 选你自己的文件夹
   - 或设置环境变量 `EMOTICON_STICKERS_DIR=C:\你的表情目录`
3. **绑定目标窗口**:⚙ → 目标窗口 → 点击微信/聊天窗口 → 绑定成功(显示进程名)
4. **发表情**:鼠标悬停 GIF 可实时预览动画 → 点击表情 → 自动粘贴进目标窗口

### 目录结构

```
%APPDATA%\EmoticonPanel-egui\
├── stickers\            ← 表情包根目录(每个子文件夹一个分组)
│   ├── 搞笑日常\
│   │   ├── cover.png    ← 可选: Tab 封面
│   │   ├── 01.png
│   │   └── 02.gif
│   └── ...
└── settings.json        ← 持久化: 目标窗口 / 表情包位置
```

---

## 🔀 分支说明

这个项目有三个独立实现,分别放在不同分支:

| 分支 | 实现 | 内存(WS) | 说明 |
|---|---|---|---|
| **main** | **egui-lite(本版)** | ~82MB | 内存优化版,现行交付 |
| `egui` | egui 功能完备版 | ~120MB | 96px 全帧缓存,功能等同 |
| `tauri` | Tauri 2 + WebView2 | ~465MB | 最早版本,前端 vanilla-ts |

```bash
git checkout egui    # 切换(注意: 切换会清掉被忽略的 target/ 缓存, 需重新 cargo build --release)
```

各分支含独立 README/依赖;exe 归档在仓库外 `H:\VibeCoding\EmoticonPanel-archives\`。

---

## 🧪 测试

```bash
cd egui-app-lite
cargo test --release decode_smoke   # 解码冒烟(需先关闭运行中的程序)
```

诊断脚本(`scripts/`):`hover_shot.ps1` 按 PID 定位窗口并截图,配合 Python/Pillow 做像素级回归验证。

---

## 📌 常见问题

- **GIF 悬浮不动?** 部分 ".gif" 实为单帧静态图(实测 5268 个中占 39%),属正常;真动图悬浮必播。
- **点击没粘贴?** 确认已绑定目标窗口(⚙→目标窗口),且目标窗口在前台可接收 Ctrl+V。
- **生僻字显示不全?** 内嵌 GB2312 字集外字符会缺字,请联系补充字集。

## 📄 License

MIT(代码可自由使用/修改)。