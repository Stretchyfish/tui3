//! Cached true-color Unicode half-block image renderer for Chat Markdown and Editor previews.
use image::imageops::FilterType;
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::UNIX_EPOCH,
};

const BG_R: u8 = 15;
const BG_G: u8 = 18;
const BG_B: u8 = 24;

#[derive(Clone)]
pub struct RenderedImage {
    pub width: u32,
    pub height: u32,
    pub lines: Vec<Line<'static>>,
}

type CacheKey = (PathBuf, u64, u16, u16);

fn cache() -> &'static Mutex<HashMap<CacheKey, Option<RenderedImage>>> {
    static CACHE: OnceLock<Mutex<HashMap<CacheKey, Option<RenderedImage>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn is_image_path(path: &str) -> bool {
    let lower = path.trim().to_ascii_lowercase();
    let base = lower.split(['?', '#']).next().unwrap_or(&lower);
    matches!(
        Path::new(base).extension().and_then(|e| e.to_str()),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp")
    )
}

pub fn resolve_image_path(raw: &str, workspace: Option<&str>) -> Option<PathBuf> {
    let trimmed = raw.trim().trim_matches(['<', '>']);
    if trimmed.is_empty()
        || trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("data:")
    {
        return None;
    }
    let stripped = trimmed
        .strip_prefix("file://")
        .unwrap_or(trimmed)
        .split(['?', '#'])
        .next()
        .unwrap_or(trimmed);
    let candidate = Path::new(stripped);
    let path = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else if let Some(root) = workspace {
        Path::new(root).join(candidate)
    } else {
        candidate.to_path_buf()
    };
    path.is_file().then_some(path)
}

pub fn resolve_attachment_image(attachment: &serde_json::Value) -> Option<(String, PathBuf)> {
    let kind = attachment["type"].as_str().unwrap_or_default();
    let mime = attachment["mimeType"].as_str().unwrap_or_default();
    let name = attachment["name"]
        .as_str()
        .or_else(|| attachment["fileName"].as_str())
        .unwrap_or("image");
    if kind != "image" && !mime.starts_with("image/") && !is_image_path(name) {
        return None;
    }
    for key in ["path", "localPath", "filePath", "uri", "url"] {
        if let Some(raw) = attachment[key].as_str()
            && let Some(path) = resolve_image_path(raw, None)
        {
            return Some((name.to_owned(), path));
        }
    }
    // Check T3's local attachments directory by attachment id.
    let id = attachment["id"].as_str()?;
    let attachments_dir = crate::pairing::local_state_dir()?.join("attachments");
    let entries = std::fs::read_dir(attachments_dir).ok()?;
    for entry in entries.flatten() {
        let fname = entry.file_name();
        let fname_str = fname.to_string_lossy();
        if fname_str.starts_with(id) && is_image_path(&fname_str) && entry.path().is_file() {
            return Some((name.to_owned(), entry.path()));
        }
    }
    None
}

fn composite_rgb(px: image::Rgba<u8>) -> Color {
    let a = px[3] as u16;
    if a == 255 {
        return Color::Rgb(px[0], px[1], px[2]);
    }
    if a == 0 {
        return Color::Rgb(BG_R, BG_G, BG_B);
    }
    let inv = 255 - a;
    let r = ((px[0] as u16 * a + BG_R as u16 * inv) / 255) as u8;
    let g = ((px[1] as u16 * a + BG_G as u16 * inv) / 255) as u8;
    let b = ((px[2] as u16 * a + BG_B as u16 * inv) / 255) as u8;
    Color::Rgb(r, g, b)
}

pub fn render_lines(path: &Path, max_cols: u16, max_rows: u16) -> Option<RenderedImage> {
    if max_cols < 2 || max_rows < 1 {
        return None;
    }
    let mtime = std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    let key = (path.to_path_buf(), mtime, max_cols, max_rows);
    if let Ok(guard) = cache().lock()
        && let Some(cached) = guard.get(&key)
    {
        return cached.clone();
    }
    let rendered = render_uncached(path, max_cols, max_rows);
    if let Ok(mut guard) = cache().lock() {
        if guard.len() >= 64 {
            guard.clear();
        }
        guard.insert(key, rendered.clone());
    }
    rendered
}

fn render_uncached(path: &Path, max_cols: u16, max_rows: u16) -> Option<RenderedImage> {
    let img = image::open(path).ok()?;
    let (orig_w, orig_h) = (img.width(), img.height());
    if orig_w == 0 || orig_h == 0 {
        return None;
    }
    let max_px_w = u32::from(max_cols);
    let max_px_h = u32::from(max_rows) * 2;
    let scale = (max_px_w as f64 / orig_w as f64)
        .min(max_px_h as f64 / orig_h as f64)
        .min(1.0);
    let target_w = ((orig_w as f64 * scale).round() as u32).max(1);
    let target_h = ((orig_h as f64 * scale).round() as u32).max(2);
    let rgba = img
        .resize_exact(target_w, target_h, FilterType::Triangle)
        .to_rgba8();

    let mut lines = Vec::with_capacity(target_h.div_ceil(2) as usize);
    let mut y = 0;
    while y < target_h {
        let mut spans = Vec::with_capacity(target_w as usize);
        let mut x = 0;
        while x < target_w {
            let fg = composite_rgb(*rgba.get_pixel(x, y));
            let bg = if y + 1 < target_h {
                composite_rgb(*rgba.get_pixel(x, y + 1))
            } else {
                Color::Rgb(BG_R, BG_G, BG_B)
            };
            let style = Style::default().fg(fg).bg(bg);
            // Coalesce adjacent identical cells to keep span counts small.
            let mut run = 1usize;
            while x + (run as u32) < target_w {
                let next_fg = composite_rgb(*rgba.get_pixel(x + run as u32, y));
                let next_bg = if y + 1 < target_h {
                    composite_rgb(*rgba.get_pixel(x + run as u32, y + 1))
                } else {
                    Color::Rgb(BG_R, BG_G, BG_B)
                };
                if next_fg == fg && next_bg == bg {
                    run += 1;
                } else {
                    break;
                }
            }
            spans.push(Span::styled("▀".repeat(run), style));
            x += run as u32;
        }
        lines.push(Line::from(spans));
        y += 2;
    }
    Some(RenderedImage {
        width: orig_w,
        height: orig_h,
        lines,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_png_to_half_block_lines_with_dimensions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sample.png");
        let mut buf = image::RgbaImage::new(8, 4);
        for y in 0..4 {
            for x in 0..8 {
                let color = if y < 2 {
                    image::Rgba([255, 0, 0, 255])
                } else {
                    image::Rgba([0, 128, 255, 255])
                };
                buf.put_pixel(x, y, color);
            }
        }
        buf.save(&path).unwrap();

        assert!(is_image_path(path.to_str().unwrap()));
        let rendered = render_lines(&path, 20, 10).unwrap();
        assert_eq!((rendered.width, rendered.height), (8, 4));
        assert_eq!(rendered.lines.len(), 2);
        assert!(
            rendered.lines[0]
                .spans
                .iter()
                .any(|s| s.content.contains('▀') && s.style.fg == Some(Color::Rgb(255, 0, 0)))
        );
    }
}
