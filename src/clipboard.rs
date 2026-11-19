//! Access the desktop clipboard off the input path and retain the copy owner.
use base64::Engine;
use std::sync::mpsc;

/// Owns the desktop selection across copies so X11 can serve clipboard requests.
pub struct Writer {
    sender: Option<mpsc::Sender<String>>,
    pub results: mpsc::Receiver<Result<(), String>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Writer {
    pub fn start() -> Self {
        let (sender, requests) = mpsc::channel::<String>();
        let (results, receiver) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let mut clipboard: Option<arboard::Clipboard> = None;
            let mut last_copy = None;
            for text in requests {
                if clipboard.is_none() {
                    match arboard::Clipboard::new() {
                        Ok(value) => clipboard = Some(value),
                        Err(error) => {
                            let _ = results
                                .send(Err(format!("Cannot access the desktop clipboard: {error}")));
                            continue;
                        }
                    }
                }
                let result = clipboard.as_mut().unwrap().set_text(text).map_err(|error| {
                    format!("Could not copy text to the desktop clipboard: {error}")
                });
                if result.is_ok() {
                    last_copy = Some(std::time::Instant::now());
                }
                let _ = results.send(result);
            }
            if let Some(last_copy) = last_copy {
                let minimum = std::time::Duration::from_millis(150);
                if let Some(remaining) = minimum.checked_sub(last_copy.elapsed()) {
                    std::thread::sleep(remaining);
                }
            }
            // Dropping the owner only after the UI stops copying lets clipboard managers
            // retrieve the selection and avoids arboard's quick-drop warning.
        });
        Self {
            sender: Some(sender),
            results: receiver,
            thread: Some(thread),
        }
    }

    pub fn copy(&self, text: String) -> Result<(), String> {
        self.sender
            .as_ref()
            .ok_or("Clipboard worker stopped.")?
            .send(text)
            .map_err(|_| "Clipboard worker stopped.".to_owned())
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub enum Content {
    Text(String),
    Image(serde_json::Value),
}

pub fn content() -> Result<Content, String> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|error| format!("Desktop clipboard unavailable: {error}"))?;
    match clipboard.get_text() {
        Ok(text) if !text.is_empty() => Ok(Content::Text(text)),
        Ok(_) | Err(arboard::Error::ContentNotAvailable) => image().map(Content::Image),
        Err(error) => Err(format!("Could not read the desktop clipboard: {error}")),
    }
}

/// Read-only connectivity check. An empty clipboard is healthy.
pub fn status() -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|error| format!("Desktop clipboard unavailable: {error}"))?;
    match clipboard.get_text() {
        Ok(_) | Err(arboard::Error::ContentNotAvailable) => Ok(()),
        Err(error) => Err(format!("Desktop clipboard read failed: {error}")),
    }
}

pub fn image() -> Result<serde_json::Value, String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|_| "Cannot access the desktop clipboard.".to_owned())?;
    let image = clipboard.get_image().map_err(|_| {
        "No image available in the clipboard. Use your terminal's paste shortcut for text."
            .to_owned()
    })?;
    let width = u32::try_from(image.width).map_err(|_| "Clipboard image is too large.")?;
    let height = u32::try_from(image.height).map_err(|_| "Clipboard image is too large.")?;
    let rgba = image::RgbaImage::from_raw(width, height, image.bytes.into_owned())
        .ok_or("Invalid clipboard image.")?;
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(rgba)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(|_| "Could not encode clipboard image.".to_owned())?;
    let bytes = bytes.into_inner();
    if bytes.len() > 10 * 1024 * 1024 {
        return Err("Clipboard image exceeds T3's 10 MiB image limit.".into());
    }
    Ok(serde_json::json!({
        "id": uuid::Uuid::new_v4().to_string(), "type": "image", "name": "clipboard.png",
        "mimeType": "image/png", "sizeBytes": bytes.len(),
        "dataUrl": format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
    }))
}
