// 表情面板 — 纯 Rust egui 版 (1:1 复刻 Tauri 版微信风格视觉)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod core;

use core::{Attach, Sticker};
use egui::{
    pos2, vec2, Align, Align2, Color32, FontId, Frame, Id, Layout, Margin, Pos2, Rect, RichText,
    Rounding, Sense, Stroke, TextureHandle, TextureOptions, Vec2,
};
use std::collections::HashMap;

// ---- 跟随事件钩子: 目标窗口移动/显隐变化即时唤醒跟随 (事件驱动, 非固定采样) ----
use std::sync::atomic::{AtomicIsize, Ordering as AOrder};
use std::sync::OnceLock;
static TARGET_HWND: AtomicIsize = AtomicIsize::new(0);
static WATCH_CTX: OnceLock<egui::Context> = OnceLock::new();

#[cfg(target_os = "windows")]
unsafe extern "system" fn win_loc_proc(
    _hook: windows::Win32::UI::Accessibility::HWINEVENTHOOK,
    _event: u32,
    hwnd: windows::Win32::Foundation::HWND,
    id_object: i32,
    _id_child: i32,
    _tid: u32,
    _tm: u32,
) {
    use windows::Win32::UI::WindowsAndMessaging::{OBJID_CLIENT, OBJID_WINDOW};
    if hwnd.0 as isize != TARGET_HWND.load(AOrder::Relaxed) {
        return;
    }
    if id_object == OBJID_WINDOW.0 || id_object == OBJID_CLIENT.0 {
        if let Some(ctx) = WATCH_CTX.get() {
            ctx.request_repaint(); // 目标动了/显隐变了 -> 立即唤醒跟随
        }
    }
}

use std::io::Cursor;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

const THUMB_MAX: u32 = 72; // 显示同尺寸, 降解码/纹理内存
const PREVIEW_MAX: u32 = 400;
const CELL_W: f32 = 75.0;
const CELL_H: f32 = 97.0;
const IMG: f32 = 72.0;
const COLS: usize = 4;
const W: f32 = 318.0;
const H: f32 = 445.0;
const BOTTOM_H: f32 = 46.0;
const MAX_INFLIGHT: usize = 6;

// 微信风格配色
const C_WHITE: Color32 = Color32::WHITE;
const C_BAR: Color32 = Color32::from_rgb(247, 247, 247);
const C_LINE: Color32 = Color32::from_rgb(229, 229, 229);
const C_HOVER: Color32 = Color32::from_rgb(242, 242, 242);
const C_TEXT: Color32 = Color32::from_rgb(26, 26, 26);
const C_DIM: Color32 = Color32::from_rgb(138, 138, 138);
const C_GREEN: Color32 = Color32::from_rgb(7, 193, 96);
const C_ORANGE: Color32 = Color32::from_rgb(255, 149, 0);
const C_RED: Color32 = Color32::from_rgb(250, 81, 81);

fn fit_dim(w: u32, h: u32, max: u32) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (1, 1);
    }
    let scale = (max as f32 / w.max(h) as f32).min(1.0);
    (((w as f32 * scale) as u32).max(1), ((h as f32 * scale) as u32).max(1))
}

fn trunc(s: &str, n: usize) -> String {
    let mut chars = s.chars();
    let mut out: String = chars.by_ref().take(n).collect();
    if chars.next().is_some() {
        out.push('…');
    }
    out
}

/// 解码首帧 (静态缩略图, 大部分贴图只用这个)
fn decode_static(bytes: &[u8]) -> Option<egui::ColorImage> {
    let fmt = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().ok()?.format()?;
    if fmt == image::ImageFormat::Gif {
        use image::AnimationDecoder;
        let mut it = image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).ok()?.into_frames();
        let frame = loop {
            match it.next() {
                Some(Ok(f)) => break f,
                _ => return None,
            }
        };
        let buf = frame.into_buffer();
        let (nw, nh) = fit_dim(buf.width(), buf.height(), THUMB_MAX);
        let small = image::imageops::resize(&buf, nw, nh, image::imageops::FilterType::Triangle);
        Some(egui::ColorImage::from_rgba_unmultiplied([nw as usize, nh as usize], small.as_raw()))
    } else {
        let buf = image::load_from_memory(bytes).ok()?.to_rgba8();
        let (nw, nh) = fit_dim(buf.width(), buf.height(), THUMB_MAX);
        let small = image::imageops::resize(&buf, nw, nh, image::imageops::FilterType::Triangle);
        Some(egui::ColorImage::from_rgba_unmultiplied([nw as usize, nh as usize], small.as_raw()))
    }
}

/// 解码 GIF 全部帧 (动画, 仅 hover 时按需)
fn decode_gif_frames(bytes: &[u8]) -> Option<(Vec<egui::ColorImage>, Vec<u64>)> {
    use image::AnimationDecoder;
    let mut it = image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).ok()?.into_frames();
    let mut frames = Vec::new();
    let mut delays = Vec::new();
    while frames.len() < 48 {
        match it.next() {
            Some(Ok(frame)) => {
                let (n, d) = frame.delay().numer_denom_ms(); // 毫秒有理对
                delays.push(if d == 0 { 100 } else { (n as f64 / d as f64).max(20.0) as u64 });
                let buf = frame.into_buffer();
                let (nw, nh) = fit_dim(buf.width(), buf.height(), THUMB_MAX);
                let small = image::imageops::resize(&buf, nw, nh, image::imageops::FilterType::Triangle);
                frames.push(egui::ColorImage::from_rgba_unmultiplied([nw as usize, nh as usize], small.as_raw()));
            }
            _ => break,
        }
    }
    if frames.is_empty() {
        return None;
    }
    Some((frames, delays))
}

#[derive(Clone, Copy, PartialEq)]
enum Job { Static, Anim }

struct Animated {
    frames: Vec<TextureHandle>,
    delays: Vec<u64>, // 每帧毫秒
    current: usize,
    last_time: f64,
    elapsed_ms: u64,
}

struct Avatar {
    static_tex: TextureHandle, // 常驻: 首帧 (静态显示)
    anim: Option<Animated>,    // 按需: hover 时才持有动画帧
}

struct App {
    attach: Attach,
    root: PathBuf,
    packages: Vec<core::Package>,
    stickers: Vec<Sticker>,
    current: usize,
    thumbs: HashMap<PathBuf, Avatar>,
    anim_order: std::collections::VecDeque<PathBuf>, // GIF 动画 LRU
    tab_cover: Vec<Option<TextureHandle>>,
    fallback: TextureHandle,
    show_settings: bool,
    always_on_top: bool, // 设置: 窗口始终置顶
    follow_window: bool, // 设置: 面板随目标进程 显示/隐藏/移动
    follow_t: f64,
    prev_min: bool,      // 上次面板最小化状态
    min_cooldown: f64,   // 用户手动恢复后冷却(防自动再最小化)
    menu: Option<(Pos2, usize)>, // 右键菜单位置 + 分组索引
    toast: Option<(String, std::time::Instant)>,
    folder_rx: Option<mpsc::Receiver<Option<PathBuf>>>,
    // 后台解码: 固定 worker 池 (MPMC)
    job_static_tx: crossbeam_channel::Sender<(PathBuf, Job)>,
    job_anim_tx: crossbeam_channel::Sender<(PathBuf, Job)>,
    retry: Vec<PathBuf>, // 队列满时未能入队的贴图, 轮询补发
    pending: std::collections::VecDeque<PathBuf>, // 大组延迟加载池
    // 后台扫描: 一次性返回全部分组 + 每组贴图列表, UI 线程零目录 IO
    scan_rx: std::sync::mpsc::Receiver<core::ScanResult>,
    scan_tx: std::sync::mpsc::Sender<core::ScanResult>,
    pkg_cache: HashMap<String, Vec<Sticker>>,
    scan_seq: u64,
    done_rx: crossbeam_channel::Receiver<(PathBuf, Job, Vec<egui::ColorImage>, Vec<u64>)>,
    done_tx: crossbeam_channel::Sender<(PathBuf, Job, Vec<egui::ColorImage>, Vec<u64>)>,
    tex_seq: u64,
    tabs_off: f32,
    tab_hover_last: Option<usize>,
    first_paths: Vec<PathBuf>,
    thumb_order: std::collections::VecDeque<PathBuf>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // 应用已保存的置顶状态
        if core::get_always_on_top() {
            cc.egui_ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(egui::WindowLevel::AlwaysOnTop));
        }
        install_fonts(&cc.egui_ctx);
        cc.egui_ctx.set_visuals(egui::Visuals::light());
        let fallback = cc
            .egui_ctx
            .load_texture("fb", egui::ColorImage::from_rgba_unmultiplied([4, 4], &[230; 64]), TextureOptions::LINEAR);
        let root = core::root_dir();
        let (scan_tx, scan_rx) = std::sync::mpsc::channel::<core::ScanResult>();
        let ectx = cc.egui_ctx.clone();
        {
            let root = root.clone();
            let tx = scan_tx.clone();
            let sx = ectx.clone();
            std::thread::spawn(move || {
                let r = core::scan_all(&root);
                let _ = tx.send(r);
                sx.request_repaint(); // 扫描完成唤醒 UI (闲置时 egui 不重绘, 必须主动唤醒)
            });
        }
        let packages: Vec<core::Package> = Vec::new();
        // done 有界(512) + send 阻塞背压: 防止按千计的解码结果(GB级 ColorImage)无限堆积 -> 系统冻结
        let (done_tx, done_rx) = crossbeam_channel::bounded::<(PathBuf, Job, Vec<egui::ColorImage>, Vec<u64>)>(512);
        let (anim_tx, anim_rx) = crossbeam_channel::bounded::<(PathBuf, Job)>(64);
        let (static_tx, static_rx) = crossbeam_channel::bounded::<(PathBuf, Job)>(6);
        for _ in 0..MAX_INFLIGHT {
            let arx = anim_rx.clone();
            let srx = static_rx.clone();
            let tx = done_tx.clone();
            let wx = ectx.clone();
            std::thread::spawn(move || {
                use crossbeam_channel::select;
                let mut handle = |path: PathBuf, job: Job| {
                    let path2 = path.clone();
                    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                        if let Ok(bytes) = std::fs::read(&path) {
                            match job {
                                Job::Anim => {
                                    let (f, d) = decode_gif_frames(&bytes).unwrap_or_default();
                                    (path2, Job::Anim, f, d)
                                }
                                Job::Static => {
                                    let c = decode_static(&bytes).map(|c| vec![c]).unwrap_or_default();
                                    (path2, Job::Static, c, Vec::new())
                                }
                            }
                        } else {
                            (path2, job, Vec::new(), Vec::new())
                        }
                    }));
                    if let Ok(x) = res {
                        let _ = tx.send(x); // 满时阻塞(背压), 内存受控
                        wx.request_repaint();
                    }
                };
                loop {
                    if let Ok((p, j)) = arx.try_recv() {
                        handle(p, j);
                        continue;
                    }
                    // 关键: select 同时监听两队列 — static 空时 anim 也能被唤醒, 不阻塞死锁
                    select! {
                        recv(srx) -> r => if let Ok((p, j)) = r { handle(p, j); },
                        recv(arx) -> r => if let Ok((p, j)) = r { handle(p, j); },
                    }
                }
            });
        }
        let mut a = Self {
            attach: Attach::default(),
            root,
            packages,
            stickers: Vec::new(),
            current: 0,
            thumbs: HashMap::new(),
            anim_order: std::collections::VecDeque::new(),
            tab_cover: Vec::new(),
            fallback,
            show_settings: false,
            always_on_top: core::get_always_on_top(),
            follow_window: core::get_follow_window(),
            follow_t: 0.0,
            prev_min: false,
            min_cooldown: 0.0,
            menu: None,
            toast: None,
            folder_rx: None,
            job_static_tx: static_tx,
            job_anim_tx: anim_tx,
            retry: Vec::new(),
            pending: std::collections::VecDeque::new(),
            scan_rx,
            scan_tx,
            pkg_cache: HashMap::new(),
            scan_seq: 0,
            done_rx,
            done_tx,
            tex_seq: 0,
            tabs_off: 0.0,
            tab_hover_last: None,
            first_paths: Vec::new(),
            thumb_order: std::collections::VecDeque::new(),
                                                };
        // debug-only: EMOTICON_TEST_ATTACH=<hwnd> 自动附加, 用于跟随功能自动化自验
        #[cfg(debug_assertions)]
        if let Ok(ht) = std::env::var("EMOTICON_TEST_ATTACH") {
            if let Ok(hw) = ht.trim().parse::<isize>() {
                if let Ok(mut tg) = a.attach.target.lock() {
                    *tg = Some(core::TargetInfo { hwnd: hw, title: "auto-test".into(), process: "auto-test".into(), pid: 0 });
                }
            }
        }
        a.load_group(&cc.egui_ctx);
        a
    }

    fn load_group(&mut self, ctx: &egui::Context) {
        // 从后台扫描缓存取贴图列表 (UI 线程不做任何目录 IO)
        self.stickers = self
            .packages
            .get(self.current)
            .and_then(|p| self.pkg_cache.get(&p.name).cloned())
            .unwrap_or_default();
        // 保留跨组缓存, 只对缺失贴图发起解码; 可视区优先
        let mut missing: Vec<PathBuf> = Vec::new();
        for st in &self.stickers {
            if !self.thumbs.contains_key(&st.path) {
                missing.push(st.path.clone());
            }
        }
        let head: Vec<PathBuf> = missing.iter().take(16).cloned().collect();
        let tail: Vec<PathBuf> = missing.iter().skip(16).cloned().collect();
        // 只入队前 128, 其余进 pending 池由每帧缓慢补充 (避免一次性堆出千级任务)
        let (now, later) = tail.split_at(tail.len().min(112));
        for p in now {
            if self.job_static_tx.try_send((p.clone(), Job::Static)).is_err() {
                self.retry.push(p.clone());
            }
        }
        for p in later {
            if !self.pending.contains(p) {
                self.pending.push_back(p.clone());
            }
        }
        for p in head.into_iter().rev() {
            if self.job_static_tx.try_send((p.clone(), Job::Static)).is_err() {
                self.retry.push(p);
            }
        }
        if !missing.is_empty() {
            ctx.request_repaint();
        }
        self.rebuild_tab_covers(ctx);
    }

    /// 每帧: 收 worker 完成结果, 主线程创建纹理
    /// 消费后台扫描结果
    /// 跟随目标进程: 面板随被选定窗口 显示/隐藏/移动 (贴靠其右侧)
    fn follow_tick(&mut self, ctx: &egui::Context) {
        if !self.follow_window {
            TARGET_HWND.store(0, AOrder::Relaxed);
            return;
        }
        let hwnd = match self.attach.target.lock().unwrap().clone() {
            Some(t) if t.hwnd != 0 => t.hwnd,
            _ => {
                TARGET_HWND.store(0, AOrder::Relaxed);
                return;
            }
        };
        // 注册目标 + 唤醒上下文 (hook 常驻, 目标移动/显隐即 repaint)
        TARGET_HWND.store(hwnd, AOrder::Relaxed);
        let _ = WATCH_CTX.set(ctx.clone());
        spawn_win_hook();
        // 兜底: 事件偶发丢失时 200ms 保底采样 (静止时低开销)
        ctx.request_repaint_after(Duration::from_millis(200));
        self.follow_t += ctx.input(|i| i.stable_dt as f64);
        if self.follow_t < 0.05 {
            return;
        }
        self.follow_t = 0.0;
        let cur_min = ctx.input(|i| i.viewport().minimized).unwrap_or(false);
        if !cur_min && self.prev_min {
            self.min_cooldown = 3.0; // 用户手动恢复, 3s 内不再自动最小化
        }
        self.prev_min = cur_min;
        if let Some((x, y, visible)) = unsafe { core::win::follow_target(hwnd, W as i32, H as i32) } {
            if visible {
                if cur_min {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x as f32, y as f32)));
            } else if !cur_min {
                if self.min_cooldown > 0.0 {
                    self.min_cooldown -= 0.1;
                } else {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
            }
        }
    }

    fn poll_scan(&mut self, ctx: &egui::Context) {
        while let Ok(r) = self.scan_rx.try_recv() {
            let changed = r.packages.len() != self.packages.len()
                || self.packages.iter().zip(r.packages.iter()).any(|(a, b)| a.name != b.name);
            if !changed {
                continue; // 重复/无变化结果
            }
            self.packages = r.packages;
            self.pkg_cache = r.entries;
            if self.current >= self.packages.len() {
                self.current = 0;
            }
            self.load_group(ctx);
            self.rebuild_tab_covers(ctx);
            ctx.request_repaint();
            break;
        }
    }

    fn poll_done(&mut self, ctx: &egui::Context) {
        let mut got = 0;
        while got < 24 {
            let it = match self.done_rx.try_recv() {
                Ok(x) => x,
                Err(_) => break,
            };
            let (path, job, colors, delays) = it;
            got += 1;
            match job {
                Job::Static => {
                    if let Some(first) = colors.first() {
                        self.tex_seq += 1;
                        let tex = ctx.load_texture(format!("s{}", self.tex_seq), first.clone(), TextureOptions::LINEAR);
                        if let Some(av) = self.thumbs.get_mut(&path) {
                            av.static_tex = tex.clone();
                        } else {
                            self.thumbs.insert(path.clone(), Avatar { static_tex: tex.clone(), anim: None });
                            self.thumb_order.push_back(path.clone());
                        }
                        // 首帧就绪 -> 更新对应 tab 封面
                        if let Some(i) = self.first_paths.iter().position(|f| *f == path) {
                            if self.tab_cover[i].as_ref().map(|h| h.id() == self.fallback.id()).unwrap_or(false) {
                                self.tab_cover[i] = Some(tex);
                            }
                        }
                    }
                }
                Job::Anim => {
                    if colors.len() > 1 && !delays.is_empty() {
                        let mut frames = Vec::with_capacity(colors.len());
                        for (i, c) in colors.iter().enumerate() {
                            self.tex_seq += 1;
                            frames.push(ctx.load_texture(format!("a{}", self.tex_seq), c.clone(), TextureOptions::LINEAR));
                        }
                        if let Some(av) = self.thumbs.get_mut(&path) {
                            av.anim = Some(Animated { frames, delays, current: 0, last_time: 0.0, elapsed_ms: 0 });
                            self.anim_order.retain(|p| *p != path);
                            self.anim_order.push_back(path.clone());
                            self.trim_anim();
                        }
                    }
                }
            }
        }
        // 缩略图静态缓存上限: 逐出最旧的非当前组
        while self.thumbs.len() > 1200 {
            if let Some(old) = self.thumb_order.pop_front() {
                let is_current = self.stickers.iter().any(|st| st.path == old);
                if is_current {
                    self.thumb_order.push_back(old);
                    if self.thumb_order.len() > self.thumbs.len() * 2 {
                        break;
                    }
                    continue;
                }
                self.thumbs.remove(&old);
            } else {
                break;
            }
        }
        // 大组延迟池: 每帧最多补 12 个 (平滑加载, 不一次堆出)
        if got < 20 {
            let mut fed = 0;
            while fed < 12 {
                match self.pending.pop_front() {
                    Some(p) => {
                        if self.job_static_tx.try_send((p.clone(), Job::Static)).is_err() {
                            self.retry.push(p);
                            break;
                        }
                        fed += 1;
                    }
                    None => break,
                }
            }
        }
        // 队列有空位时补发之前被拒的任务
        if !self.retry.is_empty() && got < 24 {
            let _ = ctx;
            self.retry.retain(|p| {
                if self.job_static_tx.try_send((p.clone(), Job::Static)).is_err() {
                    true
                } else {
                    false
                }
            });
        }
        if got > 0 {
            ctx.request_repaint();
        }
    }

    /// 动画帧 LRU: 上限 48, 逐出最旧 (释放纹理显存/内存)
    fn trim_anim(&mut self) {
        while self.anim_order.len() > 48 {
            if let Some(old) = self.anim_order.pop_front() {
                if let Some(av) = self.thumbs.get_mut(&old) {
                    av.anim = None;
                }
            } else {
                break;
            }
        }
    }

    fn rebuild_tab_covers(&mut self, ctx: &egui::Context) {
        // 封面路径直接来自扫描结果 (packages[].cover/first), UI 线程零目录 IO
        self.first_paths.clear();
        let old: Vec<Option<TextureHandle>> = std::mem::take(&mut self.tab_cover);
        for (idx, p) in self.packages.iter().enumerate() {
            self.first_paths.push(p.cover.clone().unwrap_or_default());
            let keep = old
                .get(idx)
                .and_then(|c| c.as_ref())
                .filter(|h| h.id() != self.fallback.id())
                .cloned();
            self.tab_cover.push(keep.or_else(|| Some(self.fallback.clone())));
        }
        // 为所有包的首张贴图补充解码任务 (未选中组的封面也需要)
        let mut seen: std::collections::HashSet<PathBuf> = self.thumbs.keys().cloned().collect();
        for fp in self.first_paths.clone() {
            if fp.as_os_str().is_empty() || seen.contains(&fp) {
                continue;
            }
            if self.job_static_tx.try_send((fp.clone(), Job::Static)).is_err() {
                self.retry.push(fp.clone());
            }
            seen.insert(fp);
        }
        ctx.request_repaint();
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        self.root = core::root_dir();
        self.scan_seq += 1;
        let root = self.root.clone();
        let tx = self.scan_tx.clone();
        let wx = ctx.clone();
        std::thread::spawn(move || {
            let r = core::scan_all(&root);
            let _ = tx.send(r);
            wx.request_repaint();
        });
        self.packages.clear();
        self.stickers.clear();
        self.toast = Some(("正在扫描表情包…".into(), std::time::Instant::now()));
    }

    /// 确保静态缩略图存在 (占位 + Job::Static 入队; 纹理由 worker 解码后主线程建)
    fn ensure_thumb(&mut self, _ctx: &egui::Context, path: &std::path::Path) {
        if !self.thumbs.contains_key(path) {
            let placeholder = self.fallback.clone();
            self.thumbs.insert(
                path.to_path_buf(),
                Avatar { static_tex: placeholder, anim: None },
            );
            if self.job_static_tx.try_send((path.to_path_buf(), Job::Static)).is_err() {
                self.retry.push(path.to_path_buf());
            }
        }
    }

    /// hover GIF: 按需预取动画帧 (无动画缓存时)
    fn ensure_anim(&mut self, ctx: &egui::Context, path: &std::path::Path) {
        // 动画已就绪则直接用 (不依赖静态首帧是否完成 — 否则首次 hover 会因 static 未就绪而跳过)
        if self.thumbs.get(path).map(|av| av.anim.is_some()).unwrap_or(false) {
            return;
        }
        let p = path.to_path_buf();
        if self.job_anim_tx.try_send((p.clone(), Job::Anim)).is_err() {
            // 动画队列满: 仅对小文件(<300KB)主线程同步解, 避免大 GIF 卡 UI; 大文件记录重试
            let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            if size > 0 && size <= 300 * 1024 {
                if let Ok(bytes) = std::fs::read(&p) {
                    if let Some((colors, delays)) = decode_gif_frames(&bytes) {
                        if colors.len() > 1 {
                            let mut frames = Vec::with_capacity(colors.len());
                            for (i, c) in colors.iter().enumerate() {
                                self.tex_seq += 1;
                                frames.push(ctx.load_texture(format!("a{}", self.tex_seq), c.clone(), TextureOptions::LINEAR));
                            }
                            if let Some(av) = self.thumbs.get_mut(&p) {
                                av.anim = Some(Animated { frames, delays, current: 0, last_time: 0.0, elapsed_ms: 0 });
                                self.anim_order.retain(|q| *q != p);
                                self.anim_order.push_back(p);
                                self.trim_anim();
                                ctx.request_repaint();
                            }
                        }
                    }
                }
            } else {
                self.retry.push(p.clone());
            }
        }
    }

    /// 推进已有动画帧 (仅 hover 产生的 anim)
    fn advance_animations(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let mut need = false;
        let mut min_delay = u64::MAX;
        let paths: Vec<PathBuf> = self.anim_order.iter().cloned().collect();
        for p in paths {
            if let Some(at) = self.thumbs.get_mut(&p) {
                if let Some(anim) = at.anim.as_mut() {
                    let dt_ms = ((now - anim.last_time) * 1000.0).max(0.0) as u64;
                    anim.last_time = now;
                    anim.elapsed_ms += dt_ms;
                    let mut guard = 0usize;
                    while anim.elapsed_ms >= anim.delays[anim.current] && guard < anim.delays.len() {
                        anim.elapsed_ms -= anim.delays[anim.current];
                        anim.current = (anim.current + 1) % anim.delays.len();
                        guard += 1;
                    }
                    need = true;
                    min_delay = min_delay.min(anim.delays[anim.current].max(1));
                }
            }
        }
        if need {
            ctx.request_repaint_after(Duration::from_millis(min_delay.min(100)));
        }
    }

    /// 虚拟化渲染: 仅可见行才完整绘制/解码/命中
    fn sticker_cell(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        st: &Sticker,
        rect: egui::Rect,
        resp: &egui::Response,
        clip: egui::Rect,
    ) -> bool {
        let visible = rect.bottom() >= clip.top() - 8.0 && rect.top() <= clip.bottom() + 8.0;
        if !visible {
            return false; // 不可见: 不 ensure/不绘制 (大组不再全量解码)
        }
        self.ensure_thumb(ctx, &st.path);
        let hover = ui.rect_contains_pointer(rect);
        let painter = ui.painter();
        if hover {
            painter.rect(rect, Rounding::same(6.0), C_HOVER, Stroke::NONE);
        }
        // GIF 实时预览: 悬浮时按播放帧渲染, 否则静态第一帧
        let is_gif = st.is_gif;
        if hover && is_gif {
            self.ensure_anim(ctx, &st.path);
        }
        let tex = self
            .thumbs
            .get(&st.path)
            .map(|av| {
                if hover && is_gif {
                    if let Some(anim) = &av.anim {
                        // 请求持续重绘以播放动画
                        ctx.request_repaint_after(Duration::from_millis(
                            anim.delays[anim.current].clamp(20, 100),
                        ));
                        return anim.frames[anim.current].id();
                    }
                }
                av.static_tex.id()
            })
            .unwrap_or(self.fallback.id());
        // 72x72 图片
        let img_rect = Rect::from_center_size(rect.center_top() + vec2(0.0, IMG / 2.0 + 3.0), vec2(IMG, IMG));
        painter.image(tex, img_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), C_WHITE);
        // 文件名
        painter.text(
            pos2(rect.center().x, rect.bottom() - 4.0),
            Align2::CENTER_BOTTOM,
            trunc(&st.name, 8),
            FontId::proportional(9.5),
            C_DIM,
        );
        true
    }

    fn draw_tab_chip(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        index: usize,
        name: &str,
        gif_count: usize,
        active: bool,
    ) -> egui::Response {
        let (rect, resp) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
        let painter = ui.painter();
        let r = Rounding::same(6.0);
        let (fill, stroke) = if active {
            (C_WHITE, Stroke::new(1.0, C_LINE))
        } else if resp.hovered() {
            (Color32::from_rgb(236, 236, 236), Stroke::NONE)
        } else {
            (Color32::TRANSPARENT, Stroke::NONE)
        };
        painter.rect(rect, r, fill, stroke);
        // 封面
        let img_id = self.tab_cover.get(index).and_then(|c| c.as_ref()).map(|h| h.id()).unwrap_or(self.fallback.id());
        let ir = Rect::from_center_size(rect.center(), vec2(26.0, 26.0));
        painter.image(img_id, ir, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), C_WHITE);
        // GIF 徽标
        if gif_count > 0 {
            let b = Rect::from_min_size(pos2(rect.right() - 18.0, rect.bottom() - 12.0), vec2(17.0, 11.0));
            painter.rect(b, Rounding::same(3.0), C_ORANGE, Stroke::NONE);
            painter.text(b.center(), Align2::CENTER_CENTER, "GIF", FontId::proportional(7.0), C_WHITE);
        }
        let _ = ctx;
        resp
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.follow_tick(ctx);
        self.poll_scan(ctx);
        self.poll_done(ctx);
        self.advance_animations(ctx);
        if self.attach.picking.load(std::sync::atomic::Ordering::SeqCst) {
            ctx.request_repaint_after(Duration::from_millis(120));
        }

        if let Some(rx) = &self.folder_rx {
            if let Ok(Some(dir)) = rx.try_recv() {
                if core::set_stickers_dir(&dir.to_string_lossy()).is_ok() {
                    self.toast = Some(("表情包位置已切换".into(), std::time::Instant::now()));
                    self.refresh(ctx);
                }
                self.folder_rx = None;
            }
        }

        // ---------- 底部栏 (微信风格灰底 + 上边框) ----------
        egui::TopBottomPanel::bottom("bottom")
            .exact_height(BOTTOM_H)
            .frame(Frame::none().fill(C_BAR).inner_margin(Margin::symmetric(8.0, 6.0)).stroke(Stroke::new(1.0, C_LINE)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let mut names: Vec<String> = Vec::new();
                    let mut gif_counts: Vec<usize> = Vec::new();
                    for p in self.packages.iter() {
                        names.push(p.name.clone());
                        gif_counts.push(p.gif_count);
                    }
                    // ---- Tab 自绘 + 滚轮横向滚动 ----
                    let avail_w = ui.available_width() - 40.0; // 留 ⚙ 位置
                    let per = 36.0;
                    let total_w = names.len() as f32 * per - 4.0;
                    let max_off = (total_w - avail_w).max(0.0);
                    let (row_rect, row_resp) = ui.allocate_exact_size(vec2(avail_w, 32.0), Sense::hover());
                    // 只有鼠标悬停在 Tab 栏上时才响应滚轮 (避免全局误滚)
                    let pointer_in = ui.rect_contains_pointer(row_rect);
                    if max_off > 0.0 && pointer_in {
                        let dy = ui.input(|i| i.raw_scroll_delta.y);
                        if dy != 0.0 {
                            // 滚轮向上 -> 向左滚动
                            self.tabs_off = (self.tabs_off - dy).clamp(0.0, max_off);
                        }
                    } else if max_off <= 0.0 {
                        self.tabs_off = 0.0;
                    }
                    let painter = ui.painter_at(row_rect);
                    let mut clicked = None;
                    let mut right_clicked = None;
                    let mut hovered_idx = None;
                    for (i, n) in names.iter().enumerate() {
                        let x = 2.0 + i as f32 * per - self.tabs_off;
                        let chip_rect = Rect::from_min_size(pos2(row_rect.min.x + x, row_rect.min.y), vec2(32.0, 32.0));
                        if !chip_rect.intersects(row_rect) {
                            continue;
                        }
                        let resp = ui.interact(chip_rect, Id::new(("tab", i)), Sense::click());
                        let active = self.current == i;
                        let (fill, stroke) = if active {
                            (C_WHITE, Stroke::new(1.0, C_LINE))
                        } else if resp.hovered() {
                            (Color32::from_rgb(236, 236, 236), Stroke::NONE)
                        } else {
                            (Color32::TRANSPARENT, Stroke::NONE)
                        };
                        painter.rect(chip_rect, Rounding::same(6.0), fill, stroke);
                        let img_id = self
                            .tab_cover
                            .get(i)
                            .and_then(|c| c.as_ref())
                            .map(|h| h.id())
                            .unwrap_or(self.fallback.id());
                        let ir = Rect::from_center_size(chip_rect.center(), vec2(26.0, 26.0));
                        painter.image(img_id, ir, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), C_WHITE);
                        if gif_counts[i] > 0 {
                            let b = Rect::from_min_size(pos2(chip_rect.right() - 18.0, chip_rect.bottom() - 12.0), vec2(17.0, 11.0));
                            painter.rect(b, Rounding::same(3.0), C_ORANGE, Stroke::NONE);
                            painter.text(b.center(), Align2::CENTER_CENTER, "GIF", FontId::proportional(7.0), C_WHITE);
                        }
                        if resp.clicked() {
                            clicked = Some(i);
                        }
                        if resp.secondary_clicked() {
                            right_clicked = Some((ctx.pointer_latest_pos().unwrap_or(row_rect.left_top()), i));
                        }
                        if resp.hovered() {
                            hovered_idx = Some(i);
                        }
                    }
                    if hovered_idx != self.tab_hover_last {
                        self.tab_hover_last = hovered_idx;
                        if let Some(i) = hovered_idx {
                            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                        }
                    }
                    ui.allocate_space(vec2(0.0, 0.0));
                    // ⚙ 按钮
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let (g, g_resp) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
                        let p = ui.painter();
                        p.rect(g, Rounding::same(6.0), if g_resp.hovered() { Color32::from_rgb(236, 236, 236) } else { Color32::TRANSPARENT }, Stroke::NONE);
                        p.text(g.center(), Align2::CENTER_CENTER, "⚙", FontId::proportional(16.0), C_TEXT);
                        if g_resp.clicked() {
                            self.show_settings = !self.show_settings;
                        }
                    });
                    if let Some(i) = clicked {
                        self.current = i;
                        self.load_group(ctx);
                    }
                    if let Some((pos, i)) = right_clicked {
                        self.menu = Some((pos, i));
                    }
                    let _ = row_resp;
                });
            });

        // ---------- 右键菜单 (自绘) ----------
        if let Some((pos, idx)) = self.menu.clone() {
            let pkg = self.packages.get(idx).cloned();
            if let Some(pkg) = pkg {
                let mw = 130.0;
                let mh = 34.0;
                let mut open = true;
                egui::Area::new(Id::new("ctxmenu"))
                    .fixed_pos(pos)
                    .order(egui::Order::Foreground)
                    .show(ctx, |ui| {
                        let inner = Frame::none()
                            .fill(C_WHITE)
                            .rounding(6.0)
                            .stroke(Stroke::new(1.0, C_LINE))
                            .inner_margin(Margin::symmetric(12.0, 0.0))
                            .shadow(egui::epaint::Shadow { offset: vec2(0.0, 2.0), blur: 12.0, spread: 0.0, color: Color32::from_black_alpha(40) });
                        inner.show(ui, |ui| {
                            let (r, resp) = ui.allocate_exact_size(vec2(100.0, mh), Sense::click());
                            ui.painter().text(r.center(), Align2::CENTER_CENTER, format!("删除「{}」", trunc(&pkg.name, 6)), FontId::proportional(13.0), C_RED);
                            if resp.clicked() {
                                open = false;
                                if core::delete_package(&self.root, &pkg.name).is_ok() {
                                    self.toast = Some((format!("已删除「{}」", pkg.name), std::time::Instant::now()));
                                }
                                self.refresh(ctx);
                            }
                        });
                    });
                if !open {
                    self.menu = None;
                }
            } else {
                self.menu = None;
            }
        }

        // ---------- 设置面板 (名称左加粗/按钮右/值第二行, 宽<=300) ----------
        if self.show_settings {
            egui::Area::new(Id::new("settings"))
                .anchor(Align2::RIGHT_BOTTOM, vec2(-8.0, -(BOTTOM_H + 8.0)))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    Frame::none()
                        .fill(C_WHITE)
                        .rounding(10.0)
                        .stroke(Stroke::new(1.0, C_LINE))
                        .inner_margin(Margin::same(8.0))
                        .shadow(egui::epaint::Shadow { offset: vec2(0.0, 4.0), blur: 18.0, spread: 0.0, color: Color32::from_black_alpha(46) })
                        .show(ui, |ui| {
                            ui.set_width(284.0); // 总宽 284+16 = 300
                            let row_frame = Frame::none()
                                .fill(Color32::from_rgb(245, 245, 245))
                                .rounding(8.0)
                                .inner_margin(Margin::symmetric(10.0, 7.0));
                            let name = |x: &str| RichText::new(x).size(13.0).strong().color(C_TEXT);
                            let v_text = |x: &str| RichText::new(x).size(12.0).color(C_TEXT);

                            // 目标窗口: 第一行 名+右按钮; 第二行 状态
                            let t = self.attach.target.lock().unwrap().clone();
                            let picking = self.attach.picking.load(std::sync::atomic::Ordering::SeqCst);
                            row_frame.clone().show(ui, |ui| {
                                let mut act = None;
                                ui.horizontal(|ui| {
                                    ui.label(name("目标窗口"));
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if picking {
                                            if chip(ui, vec2(56.0, 28.0), "取消", false).clicked() {
                                                act = Some(0);
                                            }
                                        } else if chip(ui, vec2(56.0, 28.0), if t.is_some() { "重选" } else { "选择" }, true).clicked() {
                                            act = Some(1);
                                        }
                                    });
                                });
                                let status = if picking {
                                    "正在选择…目前请点击目标窗口".to_string()
                                } else if let Some(t) = &t {
                                    format!("{} · {}", t.process, trunc(&t.title, 8))
                                } else {
                                    "未选择".to_string()
                                };
                                ui.label(v_text(&status).color(if picking { C_ORANGE } else { C_DIM }));
                                if act == Some(0) {
                                    core::cancel_pick(&self.attach);
                                } else if act == Some(1) {
                                    core::begin_pick(&self.attach);
                                    self.toast = Some(("请在 15 秒内点击目标窗口".into(), std::time::Instant::now()));
                                }
                            });
                            ui.add_space(8.0);

                            // 刷新: 名+右按钮 (无第二行)
                            row_frame.clone().show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(name("刷新表情包"));
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if chip(ui, vec2(56.0, 28.0), "刷新", false).clicked() {
                                            self.refresh(ctx);
                                            self.toast = Some(("已刷新表情包".into(), std::time::Instant::now()));
                                        }
                                    });
                                });
                            });
                            ui.add_space(8.0);

                            // 始终置顶: 名+右切换按钮
                            row_frame.clone().show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(name("始终置顶"));
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        let label = if self.always_on_top { "已开启" } else { "开启" };
                                        if chip(ui, vec2(60.0, 28.0), label, !self.always_on_top).clicked() {
                                            self.always_on_top = !self.always_on_top;
                                            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                                                if self.always_on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal },
                                            ));
                                            let _ = core::set_always_on_top(self.always_on_top);
                                            self.toast = Some((format!("已{}置顶", if self.always_on_top { "开启" } else { "关闭" }), std::time::Instant::now()));
                                        }
                                    });
                                });
                            });
                            ui.add_space(8.0);
                            // 跟随窗口: 面板随被选定进程 显示/隐藏/移动
                            row_frame.clone().show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(name("跟随窗口"));
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        let label = if self.follow_window { "已开启" } else { "开启" };
                                        if chip(ui, vec2(60.0, 28.0), label, !self.follow_window).clicked() {
                                            self.follow_window = !self.follow_window;
                                            let _ = core::set_follow_window(self.follow_window);
                                            self.toast = Some((format!("已{}跟随窗口", if self.follow_window { "开启" } else { "关闭" }), std::time::Instant::now()));
                                            if !self.follow_window {
                                                // 关闭跟随: 恢复手动模式, 确保窗口可见
                                                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                                            }
                                        }
                                    });
                                });
                            });
                            ui.add_space(8.0);

                            // 表情包位置: 第一行 名+右按钮; 第二行 完整路径(换行)
                            row_frame.show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(name("表情包位置"));
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if chip(ui, vec2(68.0, 28.0), "选文件夹", false).clicked() {
                                            let (tx, rx) = mpsc::channel();
                                            self.folder_rx = Some(rx);
                                            std::thread::spawn(move || {
                                                let dir = rfd::FileDialog::new().set_title("选择表情包文件夹").pick_folder();
                                                let _ = tx.send(dir);
                                            });
                                        }
                                    });
                                });
                                let path_text = self.root.to_string_lossy().to_string();
                                ui.add(egui::Label::new(v_text(&path_text).color(C_DIM)).wrap());
                            });
                            ui.add_space(4.0);
                        });
                });
        }

        // ---------- 网格 (Grid 固定列宽对齐, 隐藏滚动条) ----------
        egui::CentralPanel::default()
            .frame(Frame::none().fill(C_WHITE))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("grid")
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.add_space(4.0);
                        let stickers = self.stickers.clone();
                        let mut insert_err: Option<String> = None;
                        egui::Grid::new("stkgrid")
                            .num_columns(COLS)
                            .min_col_width(CELL_W)
                            .max_col_width(CELL_W)
                            .spacing(vec2(5.0, 0.0))
                            .show(ui, |ui| {
                                let clip = ui.clip_rect();
                                for (i, st) in stickers.iter().enumerate() {
                                    let (rect, resp) = ui.allocate_exact_size(vec2(CELL_W, CELL_H), Sense::click());
                                    let visible = self.sticker_cell(ui, ctx, st, rect, &resp, clip);
                                    if visible && resp.clicked() {
                                        match core::insert_sticker(&self.attach, &self.root, &st.path) {
                                            Ok(()) => {
                                                let proc = self.attach.target.lock().unwrap().clone().map(|t| t.process).unwrap_or_default();
                                                self.toast = Some((format!("已插入 → {proc}"), std::time::Instant::now()));
                                            }
                                            Err(e) => {
                                                insert_err = Some(e);
                                            }
                                        }
                                    }
                                    if (i + 1) % COLS == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                        if let Some(e) = insert_err {
                            self.toast = Some((e, std::time::Instant::now()));
                        }
                        if self.stickers.is_empty() {
                            ui.centered_and_justified(|ui| {
                                ui.label(RichText_13("还没有表情包 — 在 ⚙ 设置里选择位置或刷新").colored_opt(C_DIM));
                            });
                        }
                        ui.add_space(4.0);
                    });
            });

        // ---------- toast ----------
        if let Some((msg, t0)) = &self.toast {
            if t0.elapsed() < Duration::from_millis(1800) {
                egui::Area::new(Id::new("toast"))
                    .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -(BOTTOM_H + 14.0)))
                    .order(egui::Order::Foreground)
                    .show(ctx, |ui| {
                        Frame::none()
                            .fill(Color32::from_rgba_unmultiplied(0, 0, 0, 200))
                            .rounding(6.0)
                            .inner_margin(Margin::symmetric(12.0, 7.0))
                            .show(ui, |ui| {
                                ui.label(RichText_12(msg).colored(C_WHITE));
                            });
                    });
            } else {
                self.toast = None;
            }
        }
    }
}

fn RichText_13(s: &str) -> egui::RichText {
    egui::RichText::new(s).size(13.0).color(C_TEXT)
}
fn RichText_12(s: &str) -> egui::RichText {
    egui::RichText::new(s).size(12.0).color(C_TEXT)
}
fn RichText_11(s: &str) -> egui::RichText {
    egui::RichText::new(s).size(11.0).color(C_DIM)
}

trait ColoredOpt {
    fn colored_opt(self, c: Color32) -> egui::RichText;
}
impl ColoredOpt for egui::RichText {
    fn colored_opt(self, _c: Color32) -> egui::RichText {
        self
    }
}
trait RichTextExt {
    fn colored(self, c: Color32) -> egui::RichText;
}
impl RichTextExt for egui::RichText {
    fn colored(self, c: Color32) -> egui::RichText {
        self.color(c)
    }
}

fn chip(ui: &mut egui::Ui, size: Vec2, text: &str, filled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    let r = Rounding::same(6.0);
    let (fill, fg, stroke) = if filled {
        (C_GREEN, C_WHITE, Stroke::new(1.0, C_GREEN))
    } else {
        let fill = if resp.hovered() { Color32::from_rgb(236, 236, 236) } else { C_WHITE };
        (fill, C_GREEN, Stroke::new(1.0, C_GREEN))
    };
    painter.rect(rect, r, fill, stroke);
    painter.text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(12.0), fg);
    resp
}

fn install_fonts(ctx: &egui::Context) {
    use egui::{FontData, FontDefinitions, FontFamily};
    let mut fonts = FontDefinitions::default();
    // 内嵌 GB2312 子集字体 (2MB, 覆盖 6692 汉字 + ASCII + 中文标点)
    let subset: &[u8] = include_bytes!("../assets/cjk-subset.ttf");
    fonts.font_data.insert("cjk".into(), FontData::from_owned(subset.to_vec()));
    for f in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts.families.entry(f).or_default().insert(0, "cjk".into());
    }
    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decode_smoke() {
        let base = std::env::var("APPDATA").unwrap();
        let root = std::path::Path::new(&base).join("EmoticonPanel-egui").join("stickers");
        let mut found = 0;
        let mut animated = 0;
        for rd in std::fs::read_dir(&root).unwrap() {
            let dir = rd.unwrap().path();
            if !dir.is_dir() {
                continue;
            }
            let ss = core::list_stickers(&root, dir.file_name().unwrap().to_str().unwrap()).unwrap();
            for st in ss.iter().take(2) {
                let bytes = std::fs::read(&st.path).unwrap();
                assert!(decode_static(&bytes).is_some(), "static decode failed: {}", st.path.display());
                found += 1;
                if st.is_gif {
                    if let Some((f, d)) = decode_gif_frames(&bytes) {
                        assert!(!f.is_empty() && !d.is_empty());
                        animated += 1;
                        eprintln!("OK gif {} frames={} delay0={}", st.path.file_name().unwrap().to_string_lossy(), f.len(), d[0]);
                    }
                }
            }
        }
        assert!(found >= 1, "no stickers");
        assert!(animated >= 1, "no gif tested");
    }
}


fn app_icon() -> Option<std::sync::Arc<egui::IconData>> {
    // 内嵌图标 PNG -> rgba -> IconData (窗口/任务栏图标)
    let bytes = include_bytes!("../assets/app.png");
    let dynimg = image::load_from_memory(bytes).ok()?;
    let rgba = dynimg.to_rgba8();
    let (w, h) = rgba.dimensions();
    Some(std::sync::Arc::new(egui::IconData {
        rgba: rgba.into_raw(),
        width: w,
        height: h,
    }))
}


#[cfg(target_os = "windows")]
#[test]
fn clip_no_dib() {
    use core::win::{set_clipboard, set_hdrop, set_png_formats, set_virtual_file};
    use windows::Win32::System::DataExchange::{
        EnumClipboardFormats, GetClipboardFormatNameW, IsClipboardFormatAvailable, RegisterClipboardFormatW,
    };
    // 模拟插入: 只写 HDROP + 虚拟文件 + PNG 原始字节 (与 insert_sticker 同集合)
    let png = include_bytes!("../assets/app.png");
    let r = unsafe {
        set_clipboard(|| {
            set_hdrop(r"H:\组.png")?;
            set_virtual_file(png, "a.png")?;
            set_png_formats(png)?;
            Ok(())
        })
    };
    assert!(r.is_ok(), "write failed: {r:?}");
    unsafe {
        use windows::Win32::System::DataExchange::{CloseClipboard, OpenClipboard};
        // EnumClipboardFormats 要求先 OpenClipboard
        assert!(OpenClipboard(None).is_ok(), "无法打开剪贴板枚举");
        let mut f = 0u32;
        let mut buf = [0u16; 64];
        let mut names = Vec::new();
        while {
            f = EnumClipboardFormats(f);
            f != 0
        } {
            let n = GetClipboardFormatNameW(f, &mut buf) as usize;
            let name = if n > 0 { String::from_utf16_lossy(&buf[..n]) } else { String::new() };
            names.push(format!("{f}:{name}"));
            assert!(f != 8, "CF_DIB 不应出现 (白底元凶): {names:?}");
            assert!(f != 1, "CF_TEXT 意外出现: {names:?}");
        }
        let _ = CloseClipboard();
        assert!(
            IsClipboardFormatAvailable(15).is_ok(),
            "CF_HDROP 缺失: {names:?}"
        );
        let png_fmt = RegisterClipboardFormatW(windows::core::w!("PNG"));
        assert!(
            IsClipboardFormatAvailable(png_fmt).is_ok(),
            "PNG 注册格式缺失: {names:?}"
        );
        eprintln!("[clip] formats = {names:?} (无 CF_DIB/8, 有 HDROP/15 + PNG)");
    }
}

#[cfg(target_os = "windows")]
fn spawn_win_hook() {
    use std::sync::Once;
    use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent};
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, TranslateMessage, MSG, EVENT_OBJECT_LOCATIONCHANGE,
        WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
    };
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::thread::spawn(move || {
            unsafe {
                let hook = SetWinEventHook(
                    EVENT_OBJECT_LOCATIONCHANGE,
                    EVENT_OBJECT_LOCATIONCHANGE,
                    None,
                    Some(win_loc_proc),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                );
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                if !hook.0.is_null() {
                    let _ = UnhookWinEvent(hook);
                }
            }
        });
    });
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([W, H])
            .with_resizable(false)
            .with_title("表情面板")
            .with_icon(app_icon().unwrap_or_else(|| std::sync::Arc::new(egui::IconData {
                rgba: vec![0, 0, 0, 0], width: 1, height: 1,
            }))),
        ..Default::default()
    };
    eframe::run_native("表情面板", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
