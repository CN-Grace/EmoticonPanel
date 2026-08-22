# 表情面板 (EmoticonPanel)

**Tauri 2 + Rust + vanilla TypeScript** 桌面小工具:**attach 到任意目标窗口(如微信聊天),点击表情即把图片粘贴进目标输入框**。仿微信表情面板的浏览交互:分组 Tab、7×3 网格翻页、圆点指示、悬停大预览、表情商店。

## 使用

```bash
npm run tauri dev          # 开发模式
npm run tauri build -- --no-bundle   # 正式构建 → src-tauri/target/release/emoticon-panel.exe
```

1. 启动应用后,点右下角 **📌 选择窗口**,再点击要插入表情的目标窗口(如微信聊天窗口),右侧会显示绑定到的进程/窗口名
2. 点击任意表情 → 立即写入剪贴板并模拟 **Ctrl+V** 粘贴进目标窗口输入框
3. 再点 attach 按钮可随时换目标;拾取中再点一次可取消

> 粘贴方式(剪贴板多种格式并存):
- **CF_HDROP 真实文件路径(首选)**:微信/QQ"复制图片文件+Ctrl+V"会插入**原始文件** → 透明 PNG 与动图 GIF 全保留
- **FileGroupDescriptorW + FileContents** 虚拟文件(同上语义,兼容虚拟剪贴板)
- **`PNG`/`image/png` 原始字节 + CF_DIB(BGRA+白底合成)** 兜底给只认位图的程序

## 表情包目录

默认根目录: `%APPDATA%\com.grace.emoticonpanel\stickers`

- 每个**子文件夹 = 一个表情包分组**(文件夹名即分组名),支持 `png / gif / jpg / jpeg / webp / bmp`,可混装
- `cover.png`(或 cover.gif 等)作为分组封面,不计入表情网格
- **不内置任何示例素材**;放入自己的包后点 **⭮ 刷新** 即可;右键分组 Tab 可删除该分组
- 可用环境变量指定目录: `EMOTICON_STICKERS_DIR=D:\我的表情`
- 商店包在 `<表情根目录>/shop`:点 **＋** 打开表情商店,「下载」即安装为分组;只有往 shop 里放包才会出现

## 测试

```bash
# Rust 后端单元测试 (路径安全/扫描/安装删除; 注意: 请用 --release)
cd src-tauri && cargo test --release

# 前端 UI 驱动测试 (Playwright + 本机 Edge, mock Tauri 桥, 20 项断言)
npm run build && node scripts/uitest.cjs

# Win32 注入链路自检 (真实系统: 剪贴板三格式写读回 / 窗口拾取 / 激活+Ctrl+V)
cd scripts/wintest && cargo run
```

## 已知问题

- `cargo test`(debug 模式)测试二进制在 mingw 下加载报 `0xc0000139`(GNU debug 链接怪癖); 请用 `cargo test --release`。
- 拾取窗口时若点到桌面会绑到桌面窗口(粘贴无效果),重新选择即可。

## 安全

- 读取路径 canonicalize 后必须位于表情根目录内;包名校验拒绝 `..`/`/`/`\`/特殊字符。
- 注入仅做 Windows 剪贴板 + 键盘模拟,不读写目标进程内存。

## 目录结构

```
src/                  前端 (index.html / main.ts / styles.css)
src-tauri/src/lib.rs  Rust 命令: 表情包扫描/读取/商店 + get_target/is_picking/begin_pick/
                     cancel_pick/insert_sticker (attach 目标窗口 + 剪贴板粘贴)
scripts/wintest/      独立 Win32 自检程序 (窗口/剪贴板/激活)
```