//! Konvertieren. Bilder erledigt Rust selbst (image-Crate, keine Zusatzprogramme).
//! Audio und Video laufen ueber ffmpeg — nur wenn es installiert ist, werden die Ziele angeboten.
//! Das Ergebnis landet immer NEBEN dem Original, nie wird etwas ueberschrieben.

use std::fs::File;
use std::io::BufWriter;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, ImageReader};
use serde::Serialize;
use tauri::AppHandle;

use crate::activities::{self, Action, Activity};
use crate::shelf::kind_of;
use crate::win::CREATE_NO_WINDOW;

#[derive(Serialize, Clone)]
pub struct Target {
    pub id: String,
    pub label: String,
}

fn t(id: &str, label: &str) -> Target {
    Target { id: id.into(), label: label.into() }
}

/// ffmpeg im PATH oder dort, wo winget es ablegt.
static FFMPEG: LazyLock<Option<PathBuf>> = LazyLock::new(|| {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Some(l) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(l).join("Microsoft").join("WinGet").join("Links"));
    }
    dirs.into_iter().map(|d| d.join("ffmpeg.exe")).find(|p| p.exists())
});

fn norm(ext: &str) -> &str {
    match ext {
        "jpeg" => "jpg",
        "tif" => "tiff",
        e => e,
    }
}

#[tauri::command]
pub fn convert_targets(path: String) -> Vec<Target> {
    let ext = Path::new(&path).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let own = norm(&ext).to_string();
    let ff = FFMPEG.is_some();
    let list: Vec<Target> = match kind_of(&ext) {
        "image" if !matches!(own.as_str(), "heic" | "avif") => vec![
            t("png", "PNG"),
            t("jpg", "JPG"),
            t("webp", "WEBP"),
            t("gif", "GIF"),
            t("bmp", "BMP"),
            t("tiff", "TIFF"),
            t("ico", "ICO"),
            t("small", "≤ 1600 px"),
        ],
        "audio" if ff => vec![t("mp3", "MP3"), t("m4a", "M4A"), t("wav", "WAV"), t("flac", "FLAC"), t("ogg", "OGG")],
        "video" if ff => vec![t("mp4", "MP4"), t("gif", "GIF"), t("mp3", "MP3")],
        _ => vec![],
    };
    list.into_iter().filter(|x| x.id != own).collect()
}

#[tauri::command]
pub fn ffmpeg_available() -> bool {
    FFMPEG.is_some()
}

/// name.ext, name (1).ext, name (2).ext …
fn unique(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut p = dir.join(format!("{stem}.{ext}"));
    let mut n = 1;
    while p.exists() {
        p = dir.join(format!("{stem} ({n}).{ext}"));
        n += 1;
    }
    p
}

fn image_convert(src: &Path, target: &str, out: &Path) -> Result<(), String> {
    let img = ImageReader::open(src)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| format!("Bild nicht lesbar: {e}"))?;
    let save = |img: &DynamicImage, fmt: ImageFormat| -> Result<(), String> {
        if fmt == ImageFormat::Jpeg {
            let f = BufWriter::new(File::create(out).map_err(|e| e.to_string())?);
            let rgb = DynamicImage::ImageRgb8(img.to_rgb8());
            return rgb.write_with_encoder(JpegEncoder::new_with_quality(f, 90)).map_err(|e| e.to_string());
        }
        let img = match fmt {
            ImageFormat::WebP | ImageFormat::Gif | ImageFormat::Ico => DynamicImage::ImageRgba8(img.to_rgba8()),
            _ => img.clone(),
        };
        img.save_with_format(out, fmt).map_err(|e| e.to_string())
    };
    match target {
        "png" => save(&img, ImageFormat::Png),
        "jpg" => save(&img, ImageFormat::Jpeg),
        "webp" => save(&img, ImageFormat::WebP),
        "gif" => save(&img, ImageFormat::Gif),
        "bmp" => save(&img, ImageFormat::Bmp),
        "tiff" => save(&img, ImageFormat::Tiff),
        "ico" => {
            let small = if img.width() > 256 || img.height() > 256 { img.resize(256, 256, FilterType::Lanczos3) } else { img };
            save(&small, ImageFormat::Ico)
        }
        "small" => {
            let fmt = ImageFormat::from_path(out).unwrap_or(ImageFormat::Png);
            let r = if img.width() > 1600 || img.height() > 1600 { img.resize(1600, 1600, FilterType::Lanczos3) } else { img };
            save(&r, fmt)
        }
        _ => Err("unbekanntes Ziel".into()),
    }
}

fn ffmpeg_convert(src: &Path, target: &str, out: &Path, video: bool) -> Result<(), String> {
    let ff = FFMPEG.as_ref().ok_or("ffmpeg fehlt")?;
    let mut c = Command::new(ff);
    c.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-n", "-i"]).arg(src);
    let a: &[&str] = match (target, video) {
        ("mp4", _) => &["-c:v", "libx264", "-preset", "veryfast", "-crf", "23", "-c:a", "aac", "-b:a", "160k", "-movflags", "+faststart"],
        ("gif", true) => &["-vf", "fps=12,scale='min(640,iw)':-2:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse", "-loop", "0"],
        ("mp3", _) => &["-vn", "-c:a", "libmp3lame", "-q:a", "2"],
        ("m4a", _) => &["-vn", "-c:a", "aac", "-b:a", "192k"],
        ("wav", _) => &["-vn"],
        ("flac", _) => &["-vn", "-c:a", "flac"],
        ("ogg", _) => &["-vn", "-c:a", "libvorbis", "-q:a", "5"],
        _ => return Err("unbekanntes Ziel".into()),
    };
    let o = c.args(a).arg(out).creation_flags(CREATE_NO_WINDOW).output().map_err(|e| e.to_string())?;
    if o.status.success() {
        Ok(())
    } else {
        let _ = std::fs::remove_file(out);
        Err(String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("ffmpeg-Fehler").to_string())
    }
}

static JOB: AtomicU64 = AtomicU64::new(0);

#[tauri::command]
pub fn convert(app: AppHandle, path: String, target: String) -> Result<(), String> {
    let src = PathBuf::from(&path);
    let ext = src.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let kind = kind_of(&ext);
    let dir = src.parent().ok_or("kein Ordner")?.to_path_buf();
    let stem = src.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let out = if target == "small" {
        unique(&dir, &format!("{stem}-klein"), norm(&ext))
    } else {
        unique(&dir, &stem, &target)
    };
    let id = format!("notch:convert:{}", JOB.fetch_add(1, Ordering::Relaxed));
    let name = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let label = if target == "small" { "kleiner".to_string() } else { target.to_uppercase() };
    let icon = Some("⇄".to_string());

    activities::upsert(
        &app,
        Activity {
            id: id.clone(),
            app: "Konvertieren".into(),
            title: name.clone(),
            subtitle: Some(format!("→ {label}")),
            icon: icon.clone(),
            color: Some("#8ab4ff".into()),
            progress: Some(-1.0),
            priority: 3,
            ..Default::default()
        },
    );

    std::thread::spawn(move || {
        let r = match kind {
            "image" => image_convert(&src, &target, &out),
            "audio" => ffmpeg_convert(&src, &target, &out, false),
            "video" => ffmpeg_convert(&src, &target, &out, true),
            _ => Err("Format wird nicht unterstuetzt".into()),
        };
        match r {
            Ok(()) => {
                crate::shelf::add_one(&app, &out);
                let out_s = out.to_string_lossy().to_string();
                let out_name = out.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                activities::upsert(
                    &app,
                    Activity {
                        id,
                        app: "Konvertieren".into(),
                        title: out_name,
                        subtitle: Some("fertig".into()),
                        icon,
                        color: Some("#5ee38a".into()),
                        progress: Some(1.0),
                        priority: 3,
                        ttl: Some(8),
                        open: Some(out_s.clone()),
                        actions: vec![Action {
                            id: "reveal".into(),
                            label: "Im Ordner".into(),
                            icon: Some("folder".into()),
                            open: Some(format!("reveal:{out_s}")),
                            ..Default::default()
                        }],
                        ..Default::default()
                    },
                );
            }
            Err(e) => activities::upsert(
                &app,
                Activity {
                    id,
                    app: "Konvertieren".into(),
                    title: name,
                    subtitle: Some(e),
                    icon,
                    color: Some("#ff453a".into()),
                    priority: 3,
                    ttl: Some(12),
                    ..Default::default()
                },
            ),
        }
    });
    Ok(())
}
