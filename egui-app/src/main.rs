// 表情面板 — 纯 Rust egui 版
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;

use core::{Attach, Sticker};
use egui::{Color32, ColorImage, RichText, TextureHandle, TextureOptions, Vec2};
use std::collections::HashMap;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

const THUMB_MAX: u32 = 96;   // 网格缩略图最大边长
const PREVIEW_MAX: u32 = 400; // hover 大图最大边长
const CELL_W: f32 = 79.0;
const IMG: f32 = 75.0;
const COLS: usize = 4;

struct AnimatedTex {
    frames: Vec<TextureHandle>,
    delays: Vec<u64>, // 每帧毫秒
    current: usize,
    last_time: f64, // 上一次推进的 egui 时间 (秒)
}

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

/// 解码 png/jpg/webp/bmp/gif → 帧纹理列表 + 每帧毫秒
fn decode_frames(bytes: &[u8], ctx: &egui::Context, max: u32, tag: &str) -> Option<(Vec<TextureHandle>, Vec<u64>)> {
    let fmt = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().ok()?.format()?;
    let mut frames: Vec<ColorImage> = Vec::new();
    let mut delays: Vec<u64> = Vec::new();

    if fmt == image::ImageFormat::Gif {
        use image::AnimationDecoder;
        let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).ok()?;
        let decoded = decoder.into_frames().collect_frames().ok()?;
        for (i, frame) in decoded.into_iter().enumerate() {
            if i >= 48 {
                break;
            }
            let (n, d) = frame.delay().numer_denom_ms();
            delays.push(if d == 0 { 100 } else { ((n as f64 / d as f64) * 1000.0).max(20.0) as u64 });
            let buf = frame.into_buffer();
            let (nw, nh) = fit_dim(buf.width(), buf.height(), max);
            let small = image::imageops::resize(&buf, nw, nh, image::imageops::FilterType::Lanczos3);
            frames.push(ColorImage::from_rgba_unmultiplied([nw as usize, nh as usize], small.as_raw()));
        }
    } else {
        let buf = image::load_from_memory(bytes).ok()?.to_rgba8();
        let (nw, nh) = fit_dim(buf.width(), buf.height(), max);
        let small = image::imageops::resize(&buf, nw, nh, image::imageops::FilterType::Lanczos3);
        frames.push(ColorImage::from_rgba_unmultiplied([nw as usize, nh as usize], small.as_raw()));
        delays.push(0);
    }
    if frames.is_empty() {
        return None;
    }
    let handles = frames
        .into_iter()
        .enumerate()
        .map(|(i, c)| ctx.load_texture(format!("{tag}#{i}"), c, TextureOptions::LINEAR))
        .collect();
    Some((handles, delays))
}

struct App {
    attach: Attach,
    root: PathBuf,
    packages: Vec<core::Package>,
    stickers: Vec<Sticker>,
    current: usize,
    thumbs: HashMap<PathBuf, AnimatedTex>,
    previews: HashMap<PathBuf, TextureHandle>,
    fallback_tex: TextureHandle,
    show_settings: bool,
    toast: Option<(String, std::time::Instant)>,
    folder_rx: Option<mpsc::Receiver<Option<PathBuf>>>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_fonts(&cc.egui_ctx);
        let fallback = cc.egui_ctx.load_texture(
            "fallback",
            ColorImage::from_rgba_unmultiplied([4, 4], &[230; 64]),
            TextureOptions::LINEAR,
        );
        let root = core::root_dir();
        let packages = core::list_packages(&root);
        let mut a = Self {
            attach: Attach::default(),
            root,
            packages,
            stickers: Vec::new(),
            current: 0,
            thumbs: HashMap::new(),
            previews: HashMap::new(),
            fallback_tex: fallback,
            show_settings: false,
            toast: None,
            folder_rx: None,
        };
        a.load_group(&cc.egui_ctx);
        a
    }

    fn load_group(&mut self, ctx: &egui::Context) {
        self.stickers = self
            .packages
            .get(self.current)
            .and_then(|p| core::list_stickers(&self.root, &p.name).ok())
            .unwrap_or_default();
        self.thumbs.clear();
        self.previews.clear();
        let paths: Vec<PathBuf> = self.stickers.iter().map(|s| s.path.clone()).collect();
        for p in paths {
            self.ensure_thumb(ctx, &p);
        }
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        self.root = core::root_dir();
        if self.current >= self.packages.len() {
            self.current = 0;
        }
        self.packages = core::list_packages(&self.root);
        self.load_group(ctx);
    }

    fn ensure_thumb(&mut self, ctx: &egui::Context, path: &std::path::Path) {
        if !self.thumbs.contains_key(path) {
            if let Ok(bytes) = std::fs::read(path) {
                let tag = path.to_string_lossy().to_string();
                if let Some((frames, delays)) = decode_frames(&bytes, ctx, THUMB_MAX, &tag) {
                    self.thumbs.insert(
                        path.to_path_buf(),
                        AnimatedTex { frames, delays, current: 0, last_time: 0.0 },
                    );
                }
            }
        }
    }

    fn ensure_preview(&mut self, ctx: &egui::Context, st: &Sticker) {
        if !self.previews.contains_key(&st.path) {
            if let Ok(bytes) = std::fs::read(&st.path) {
                if let Ok(img) = image::load_from_memory(&bytes) {
                    let buf = img.to_rgba8();
                    let (nw, nh) = fit_dim(buf.width(), buf.height(), PREVIEW_MAX);
                    let small = image::imageops::resize(&buf, nw, nh, image::imageops::FilterType::Lanczos3);
                    let color = ColorImage::from_rgba_unmultiplied([nw as usize, nh as usize], small.as_raw());
                    let h = ctx.load_texture(format!("pv_{}", st.path.to_string_lossy()), color, TextureOptions::LINEAR);
                    self.previews.insert(st.path.clone(), h);
                }
            }
        }
    }

    /// 推进 GIF 动画 (基于 egui 已流逝秒数)
    fn advance_animations(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let mut need = false;
        let mut min_delay = u64::MAX;
        let paths: Vec<PathBuf> = self.stickers.iter().map(|s| s.path.clone()).collect();
        for p in paths {
            if let Some(at) = self.thumbs.get_mut(&p) {
                if at.delays.len() > 1 {
                    let dt_ms = ((now - at.last_time) * 1000.0).max(0.0);
                    let mut acc = dt_ms as u64;
                    let mut guard = 0usize;
                    while acc >= at.delays[at.current] && guard < at.delays.len() {
                        acc -= at.delays[at.current];
                        at.current = (at.current + 1) % at.delays.len();
                        guard += 1;
                    }
                    at.last_time = now;
                    let d = at.delays[at.current].max(1);
                    min_delay = min_delay.min(d);
                    need = true;
                } else {
                    at.last_time = now;
                }
            }
        }
        if need {
            ctx.request_repaint_after(Duration::from_millis(min_delay.min(100)));
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.advance_animations(ctx);

        // 拾取状态轮询
        if self.attach.picking.load(std::sync::atomic::Ordering::SeqCst) {
            ctx.request_repaint_after(Duration::from_millis(120));
        }

        // 文件夹对话框结果
        if let Some(rx) = &self.folder_rx {
            if let Ok(Some(dir)) = rx.try_recv() {
                if core::set_stickers_dir(&dir.to_string_lossy()).is_ok() {
                    self.toast = Some(("表情包位置已切换".into(), std::time::Instant::now()));
                    self.refresh(ctx);
                }
                self.folder_rx = None;
            }
        }

        // ---------- 底部栏: 分组 Tab + ⚙ ----------
        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                let mut clicked = None;
                let mut del = None;
                let mut delname = String::new();
                let mut names: Vec<String> = Vec::new();
                let mut gif_counts: Vec<usize> = Vec::new();
                for p in self.packages.iter() {
                    names.push(p.name.clone());
                    gif_counts.push(p.gif_count);
                }
                egui::ScrollArea::horizontal().id_salt("tabs").show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (i, p) in names.iter().enumerate() {
                            let mut text = p.clone();
                            if gif_counts[i] > 0 {
                                text.push_str(&format!(" [GIF×{}]", gif_counts[i]));
                            }
                            let sel = ui.selectable_label(self.current == i, RichText::new(text).size(13.0));
                            if sel.clicked() {
                                clicked = Some(i);
                            }
                            let pname = p.clone();
                            sel.context_menu(|ui| {
                                if ui
                                    .button(RichText::new(format!("删除「{pname}」")).color(Color32::from_rgb(250, 81, 81)))
                                    .clicked()
                                {
                                    del = Some(i);
                                    delname = pname;
                                    ui.close_menu();
                                }
                            });
                        }
                    });
                });
                if let Some(i) = clicked {
                    self.current = i;
                    self.load_group(ctx);
                }
                if let Some(i) = del {
                    if let Some(p) = self.packages.get(i) {
                        if core::delete_package(&self.root, &p.name).is_ok() {
                            self.toast = Some((format!("已删除「{}」", delname), std::time::Instant::now()));
                        }
                        self.refresh(ctx);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if self.show_settings { "⚙ 关闭" } else { "⚙ 设置" };
                    if ui.button(RichText::new(label).size(13.0)).clicked() {
                        self.show_settings = !self.show_settings;
                    }
                });
            });
        });

        // ---------- 设置面板 ----------
        if self.show_settings {
            egui::Window::new("设置")
                .id(egui::Id::new("settings"))
                .collapsible(false)
                .resizable(false)
                .default_pos(egui::pos2(20.0, 60.0))
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("目标窗口");
                        let t = self.attach.target.lock().unwrap().clone();
                        let picking = self.attach.picking.load(std::sync::atomic::Ordering::SeqCst);
                        let status = if picking {
                            "正在选择…点击目标窗口".to_string()
                        } else if let Some(t) = &t {
                            format!("{} · {}", t.process, trunc(&t.title, 14))
                        } else {
                            "未选择".to_string()
                        };
                        ui.add(egui::Label::new(RichText::new(status).size(11.0)).truncate());
                        if picking {
                            if ui.button("取消").clicked() {
                                core::cancel_pick(&self.attach);
                            }
                        } else if ui.button(if t.is_some() { "重选" } else { "选择" }).clicked() {
                            core::begin_pick(&self.attach);
                            self.toast = Some(("请在 15 秒内点击目标窗口".into(), std::time::Instant::now()));
                        }
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("刷新表情包");
                        if ui.button("刷新").clicked() {
                            self.refresh(ctx);
                            self.toast = Some(("已刷新表情包".into(), std::time::Instant::now()));
                        }
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("表情包位置");
                        ui.add(egui::Label::new(RichText::new(trunc(&self.root.to_string_lossy(), 22)).size(11.0)).truncate());
                        if ui.button("选择文件夹").clicked() {
                            let (tx, rx) = mpsc::channel();
                            self.folder_rx = Some(rx);
                            std::thread::spawn(move || {
                                let dir = rfd::FileDialog::new().set_title("选择表情包文件夹").pick_folder();
                                let _ = tx.send(dir);
                            });
                        }
                    });
                });
        }

        // ---------- 网格 ----------
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().id_salt("grid").show(ui, |ui| {
                ui.add_space(6.0);
                let stickers = self.stickers.clone();
                for chunk in stickers.chunks(COLS) {
                    ui.horizontal(|ui| {
                        ui.add_space(5.0);
                        for st in chunk {
                            self.ensure_thumb(ctx, &st.path);
                            let tex = self
                                .thumbs
                                .get(&st.path)
                                .map(|at| at.frames[at.current].clone())
                                .unwrap_or_else(|| self.fallback_tex.clone());
                            let cell = ui.vertical(|ui| {
                                let img = egui::Image::from_texture(&tex).fit_to_exact_size(Vec2::new(IMG, IMG));
                                let resp = ui.add(egui::ImageButton::new(img));
                                ui.add(
                                    egui::Label::new(RichText::new(trunc(&st.name, 8)).size(9.5))
                                        .truncate(),
                                );
                                resp
                            })
                            .inner;
                            if cell.clicked() {
                                match core::insert_sticker(&self.attach, &self.root, &st.path) {
                                    Ok(()) => {
                                        let proc = self
                                            .attach
                                            .target
                                            .lock()
                                            .unwrap()
                                            .clone()
                                            .map(|t| t.process)
                                            .unwrap_or_default();
                                        self.toast = Some((format!("已插入 → {proc}"), std::time::Instant::now()));
                                    }
                                    Err(e) => {
                                        self.toast = Some((e, std::time::Instant::now()));
                                    }
                                }
                            }
                            cell.clone().on_hover_ui(|ui| {
                                self.ensure_preview(ctx, st);
                                if let Some(h) = self.previews.get(&st.path) {
                                    let size = h.size_vec2();
                                    let s = Vec2::new(150.0, 150.0).min(size);
                                    ui.add(egui::Image::from_texture(h).fit_to_exact_size(s));
                                }
                            });
                            ui.add_space(2.0);
                        }
                    });
                }
                if self.stickers.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            RichText::new("还没有表情包 — 在 ⚙ 设置里选择位置或刷新")
                                .size(13.0)
                                .color(Color32::from_gray(140)),
                        );
                    });
                }
            });
        });

        // ---------- toast ----------
        if let Some((msg, t0)) = &self.toast {
            if t0.elapsed() < Duration::from_millis(1800) {
                egui::Area::new(egui::Id::new("toast"))
                    .anchor(egui::Align2::CENTER_BOTTOM, Vec2::new(0.0, -46.0))
                    .show(ctx, |ui| {
                        egui::Frame::none()
                            .fill(Color32::from_rgba_unmultiplied(0, 0, 0, 200))
                            .rounding(6.0)
                            .inner_margin(egui::Margin::symmetric(12.0, 7.0))
                            .show(ui, |ui| {
                                ui.label(RichText::new(msg).color(Color32::WHITE).size(12.0));
                            });
                    });
            } else {
                self.toast = None;
            }
        }
    }
}

fn install_fonts(ctx: &egui::Context) {
    use egui::{FontData, FontDefinitions, FontFamily};
    let mut fonts = FontDefinitions::default();
    for c in [
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyh.ttf",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\Deng.ttf",
    ] {
        if let Ok(bytes) = std::fs::read(c) {
            fonts.font_data.insert("cjk".into(), FontData::from_owned(bytes));
            for f in [FontFamily::Proportional, FontFamily::Monospace] {
                fonts.families.entry(f).or_default().insert(0, "cjk".into());
            }
            break;
        }
    }
    ctx.set_fonts(fonts);
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([350.0, 470.0])
            .with_min_inner_size([300.0, 450.0])
            .with_title("表情面板 (egui)"),
        ..Default::default()
    };
    eframe::run_native("表情面板 (egui)", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}