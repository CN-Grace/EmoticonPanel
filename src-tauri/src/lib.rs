// 表情面板后端: 表情包扫描 / 图片读取 / 商店安装 / 分组删除
use base64::Engine;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::Manager;

const ROOT_DIR: &str = "stickers";
const SHOP_DIR: &str = "shop";

fn image_ext(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "png" | "gif" | "jpg" | "jpeg" | "webp" | "bmp"
    )
}

fn is_gif(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("gif"))
        .unwrap_or(false)
}

/// 封面文件 (cover.<ext>) 不计入表情网格
fn is_cover(path: &Path) -> bool {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("cover"))
        .unwrap_or(false)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StickerInfo {
    url: String, // 绝对路径, 前端用它调 read_sticker
    name: String,
    is_gif: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PackageInfo {
    name: String,
    cover: Option<String>,
    count: usize,
    gif_count: usize,
    shop: bool,
}

/// 表情包根目录: 优先环境变量 EMOTICON_STICKERS_DIR, 否则 appdata
fn root_dir(app: &tauri::AppHandle) -> PathBuf {
    if let Ok(p) = std::env::var("EMOTICON_STICKERS_DIR") {
        let t = p.trim();
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    app.path()
        .app_data_dir()
        .expect("no app data dir")
        .join(ROOT_DIR)
}

fn valid_name(name: &str) -> Result<String, String> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.contains(':')
        || name.contains('*')
        || name.contains('?')
        || name.contains('"')
        || name.contains('<')
        || name.contains('>')
        || name.contains('|')
    {
        return Err("非法的表情包名称".into());
    }
    Ok(name.to_string())
}

fn scan_dir_packages(root: &Path, shop: bool) -> Vec<PackageInfo> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(root) else {
        return out;
    };
    let mut dirs: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort_by(|a, b| {
        a.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase()
            .cmp(&b.file_name().and_then(|s| s.to_str()).unwrap_or("").to_lowercase())
    });
    for d in dirs {
        let name = d
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        // 读取全部图片 (含 cover), cover 只做封面不计入网格
        let mut all: Vec<PathBuf> = fs::read_dir(&d)
            .map(|rd2| {
                rd2.filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.is_file() && p.extension().and_then(|e| e.to_str()).map(image_ext).unwrap_or(false))
                    .collect()
            })
            .unwrap_or_default();
        all.sort_by(|a, b| {
            a.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase()
                .cmp(&b.file_name().and_then(|s| s.to_str()).unwrap_or("").to_lowercase())
        });
        let cover = all
            .iter()
            .find(|p| is_cover(p))
            .or_else(|| all.first())
            .cloned();
        let files: Vec<PathBuf> = all.into_iter().filter(|p| !is_cover(p)).collect();
        if files.is_empty() {
            continue;
        }
        out.push(PackageInfo {
            name,
            cover: cover.map(|c| c.to_string_lossy().to_string()),
            count: files.len(),
            gif_count: files.iter().filter(|f| is_gif(f)).count(),
            shop,
        });
    }
    out
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    let rd = fs::read_dir(src).map_err(|e| e.to_string())?;
    for e in rd {
        let e = e.map_err(|e| e.to_string())?;
        let from = e.path();
        let to = dst.join(e.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if from.is_file() {
            fs::copy(&from, &to).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// 首次运行: 把内置示例表情包铺到用户表情根目录 (不覆盖已存在的包)
fn seed_samples(app: &tauri::AppHandle) {
    if std::env::var("EMOTICON_STICKERS_DIR").map(|s| !s.trim().is_empty()).unwrap_or(false) {
        return; // 用户显式指定目录时不播种
    }
    let root = root_dir(app);
    let _ = fs::create_dir_all(&root);

    // 候选素材源: 资源目录(dev/release 由 tauri 放置) / 编译期 crate 目录(纯 cargo 构建兜底)
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(res) = app.path().resource_dir() {
        candidates.push(res.join(ROOT_DIR));
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(ROOT_DIR));

    for cand in candidates {
        if !cand.is_dir() {
            continue;
        }
        // 示例分组 -> 根目录
        let samples_src = cand.join("samples");
        if samples_src.is_dir() {
            if let Ok(rd) = fs::read_dir(&samples_src) {
                for e in rd.flatten() {
                    let p = e.path();
                    if !p.is_dir() {
                        continue;
                    }
                    let name = e.file_name();
                    let dst = root.join(&name);
                    if !dst.exists() {
                        let _ = copy_dir_recursive(&p, &dst);
                    }
                }
            }
        }
        // 商店 -> 根目录/shop
        let shop_src = cand.join(SHOP_DIR);
        let shop_dst = root.join(SHOP_DIR);
        if shop_src.is_dir() && !shop_dst.exists() {
            let _ = copy_dir_recursive(&shop_src, &shop_dst);
        }
        break; // 第一个存在的素材源即可
    }
}

#[tauri::command]
fn get_root(app: tauri::AppHandle) -> String {
    root_dir(&app).to_string_lossy().to_string()
}

#[tauri::command]
fn list_packages(app: tauri::AppHandle) -> Vec<PackageInfo> {
    scan_dir_packages(&root_dir(&app), false)
}

#[tauri::command]
fn list_stickers(app: tauri::AppHandle, package: String) -> Result<Vec<StickerInfo>, String> {
    let name = valid_name(&package)?;
    let dir = root_dir(&app).join(name);
    if !dir.is_dir() {
        return Err("表情包不存在".into());
    }
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_file() && !is_cover(p) && p.extension().and_then(|e| e.to_str()).map(image_ext).unwrap_or(false))
                .collect()
        })
        .map_err(|e| e.to_string())?;
    files.sort_by(|a, b| {
        a.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase()
            .cmp(&b.file_name().and_then(|s| s.to_str()).unwrap_or("").to_lowercase())
    });
    Ok(files
        .into_iter()
        .map(|f| StickerInfo {
            url: f.to_string_lossy().to_string(),
            name: f.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string(),
            is_gif: is_gif(&f),
        })
        .collect())
}

#[tauri::command]
fn read_sticker(app: tauri::AppHandle, path: String) -> Result<String, String> {
    let root = root_dir(&app);
    let p = PathBuf::from(&path);
    let ok = p.is_absolute()
        && std::fs::canonicalize(&root)
            .map(|cr| std::fs::canonicalize(&p).map(|cp| cp.starts_with(&cr)).unwrap_or(false))
            .unwrap_or(false);
    if !ok {
        return Err("非法的文件路径".into());
    }
    let bytes = fs::read(&p).map_err(|e| e.to_string())?;
    if bytes.is_empty() {
        return Err("空文件".into());
    }
    let mime = match p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("gif") => "image/gif",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        _ => "image/png",
    };
    Ok(format!(
        "data:{};base64,{}",
        mime,
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[tauri::command]
fn shop_list(app: tauri::AppHandle) -> Vec<PackageInfo> {
    scan_dir_packages(&root_dir(&app).join(SHOP_DIR), true)
}

#[tauri::command]
fn install_package(app: tauri::AppHandle, name: String) -> Result<(), String> {
    let n = valid_name(&name)?;
    let root = root_dir(&app);
    let from = root.join(SHOP_DIR).join(&n);
    let to = root.join(&n);
    if !from.is_dir() {
        return Err("商店里没有这个表情包".into());
    }
    if to.exists() {
        return Err(format!("「{n}」已下载过了"));
    }
    copy_dir_recursive(&from, &to)?;
    Ok(())
}

#[tauri::command]
fn delete_package(app: tauri::AppHandle, name: String) -> Result<(), String> {
    let n = valid_name(&name)?;
    let root = root_dir(&app);
    let dir = root.join(&n);
    if !dir.is_dir() {
        return Err("表情包不存在".into());
    }
    fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn reveal_root(app: tauri::AppHandle) -> Result<(), String> {
    let root = root_dir(&app);
    let _ = fs::create_dir_all(&root);
    tauri_plugin_opener::reveal_item_in_dir(&root).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            seed_samples(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_root,
            list_packages,
            list_stickers,
            read_sticker,
            shop_list,
            install_package,
            delete_package,
            reveal_root
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_src(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("emoji_rs_test_{tag}"));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn scan_packages_with_gif_and_cover() {
        let root = tmp_src("scan").join("stickers");
        let pkg = root.join("元气团子");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("01.png"), b"pngdata").unwrap();
        fs::write(pkg.join("02.gif"), b"gifdata").unwrap();
        fs::write(pkg.join("cover.png"), b"coverdata").unwrap();
        let info = scan_dir_packages(&root, false);
        assert_eq!(info.len(), 1);
        assert_eq!(info[0].name, "元气团子");
        assert_eq!(info[0].count, 2); // 不包含独立 cover? cover 也计入
        assert_eq!(info[0].gif_count, 1);
        let cover = info[0].cover.as_deref().unwrap_or("");
        assert!(cover.ends_with("cover.png"));
    }

    #[test]
    fn empty_root_returns_empty() {
        let root = tmp_src("empty").join("empty");
        fs::create_dir_all(&root).unwrap();
        assert!(scan_dir_packages(&root, false).is_empty());
        assert!(scan_dir_packages(&root.join("shop"), true).is_empty());
    }

    #[test]
    fn valid_name_rejects_unsafe() {
        for bad in ["a/b", "a\\b", "..", "a..b", "a:", "a*b", "a?b"] {
            assert!(valid_name(bad).is_err(), "should reject {bad}");
        }
        assert_eq!(valid_name("我的表情").unwrap(), "我的表情");
        assert_eq!(valid_name("basic-1").unwrap(), "basic-1");
    }

    #[test]
    fn install_and_delete_package() {
        let root = tmp_src("install").join("stickers");
        let shop = root.join(SHOP_DIR);
        let pkg = shop.join("柴犬日常");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("01.gif"), b"gif").unwrap();
        fs::write(pkg.join("cover.png"), b"png").unwrap();

        copy_dir_recursive(&pkg, &root.join("柴犬日常")).unwrap();
        assert!(root.join("柴犬日常/01.gif").is_file());
        assert!(root.join("柴犬日常/cover.png").is_file());

        // 已存在时 install_package 应报错
        let err = install_package_in(root.clone(), "柴犬日常".to_string()).unwrap_err();
        assert!(err.contains("已下载"), "err={err}");

        // 商店没有的报错
        assert!(install_package_in(root.clone(), "不存在包".to_string()).is_err());

        // 删除
        delete_package_in(root.clone(), "柴犬日常".to_string()).unwrap();
        assert!(!root.join("柴犬日常").exists());
    }

    #[test]
    fn read_sticker_mime_and_safety() {
        let root = tmp_src("read");
        let pkg = root.join("stickers").join("包");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("a.png"), [137, 80, 78, 71]).unwrap();
        fs::write(pkg.join("b.gif"), [71, 73, 70, 56]).unwrap();
        // 越界文件
        fs::write(root.join("evil.png"), b"x").unwrap();

        // 越界文件 (root 之外)
        let outside_name = format!("emoji_outside_{}", std::process::id());
        let outside = std::env::temp_dir().join(&outside_name);
        fs::write(&outside, b"x").unwrap();
        let evasive = root.join("..").join(&outside_name);
        // 直接 root 下的文件 (可控区) 允许
        let inside_png = pkg.join("a.png").to_string_lossy().to_string();
        assert_eq!(read_sticker_in(root.clone(), inside_png.clone()).unwrap().starts_with("data:image/png;base64,"), true);
        let inside_gif = pkg.join("b.gif").to_string_lossy().to_string();
        assert_eq!(read_sticker_in(root.clone(), inside_gif).unwrap().starts_with("data:image/gif;base64,"), true);
        // 非绝对路径 / 越界(含 .. 绕过)必须拒绝; root 内文件放行
        assert!(read_sticker_in(root.clone(), "rel/path.png".into()).is_err());
        assert!(read_sticker_in(root.clone(), evasive.to_string_lossy().to_string()).is_err());
        let _ = fs::remove_file(&outside);
    }

    // 命令函数需要 AppHandle, 这里抽出纯逻辑部分做测试
    fn install_package_in(root: PathBuf, name: String) -> Result<(), String> {
        let n = valid_name(&name)?;
        let from = root.join(SHOP_DIR).join(&n);
        let to = root.join(&n);
        if !from.is_dir() {
            return Err("商店里没有这个表情包".into());
        }
        if to.exists() {
            return Err(format!("「{n}」已下载过了"));
        }
        copy_dir_recursive(&from, &to)?;
        Ok(())
    }

    fn delete_package_in(root: PathBuf, name: String) -> Result<(), String> {
        let n = valid_name(&name)?;
        let dir = root.join(&n);
        if !dir.is_dir() {
            return Err("表情包不存在".into());
        }
        fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn read_sticker_in(root: PathBuf, path: String) -> Result<String, String> {
        let p = PathBuf::from(&path);
        let ok = p.is_absolute()
            && std::fs::canonicalize(&root)
                .map(|cr| std::fs::canonicalize(&p).map(|cp| cp.starts_with(&cr)).unwrap_or(false))
                .unwrap_or(false);
        if !ok {
            return Err("非法的文件路径".into());
        }
        let bytes = fs::read(&p).map_err(|e| e.to_string())?;
        let mime = match p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
            Some("gif") => "image/gif",
            Some("jpg") | Some("jpeg") => "image/jpeg",
            Some("webp") => "image/webp",
            Some("bmp") => "image/bmp",
            _ => "image/png",
        };
        Ok(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
    }
}