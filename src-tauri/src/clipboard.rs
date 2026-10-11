//! User-initiated image paste fallback for Linux WebKit. Never polls the clipboard.

#[tauri::command]
pub async fn clipboard_image(app: tauri::AppHandle) -> Result<Option<String>, String> {
    #[cfg(target_os = "linux")]
    {
        let (send, receive) = tokio::sync::oneshot::channel();
        app.run_on_main_thread(move || {
            request_image(move |result| {
                let _ = send.send(result);
            });
        })
        .map_err(|error| error.to_string())?;
        tokio::time::timeout(std::time::Duration::from_secs(5), receive)
            .await
            .map_err(|_| "The clipboard did not respond. Try copying the image again.".to_string())?
            .map_err(|_| "The clipboard request was cancelled.".to_string())?
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = app;
        Ok(None)
    }
}

#[cfg(target_os = "linux")]
fn request_image(done: impl FnOnce(Result<Option<String>, String>) + 'static) {
    use base64::Engine;
    let clipboard = gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD);
    clipboard.request_image(move |_, image| {
        let result = image
            .map(|image| {
                // Keep IPC bounded; the frontend applies its usual send-size encoding.
                if u64::from(image.width() as u32) * u64::from(image.height() as u32)
                    > 32 * 1024 * 1024
                {
                    return Err(
                        "The clipboard image is too large. Attach it as a file instead."
                            .to_string(),
                    );
                }
                let png = image
                    .save_to_bufferv("png", &[])
                    .map_err(|error| error.to_string())?;
                if png.len() > 32 * 1024 * 1024 {
                    return Err(
                        "The clipboard image is too large. Attach it as a file instead."
                            .to_string(),
                    );
                }
                Ok(base64::engine::general_purpose::STANDARD.encode(png))
            })
            .transpose();
        done(result);
    });
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    // Opt-in: reads the user's current clipboard, never changes its contents.
    #[test]
    #[ignore = "requires a desktop display and an image copied to the clipboard"]
    fn native_image_paste() {
        use base64::Engine;
        gtk::init().expect("desktop display");
        let (send, receive) = std::sync::mpsc::channel();
        super::request_image(move |result| {
            send.send(result).unwrap();
        });
        let context = gtk::glib::MainContext::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let result = loop {
            context.iteration(false);
            if let Ok(result) = receive.try_recv() {
                break result;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "clipboard request timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        let encoded = result
            .expect("clipboard read")
            .expect("copy an image before this test");
        let png = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        let decoded = gtk::gdk_pixbuf::Pixbuf::from_read(std::io::Cursor::new(png)).unwrap();
        assert!(decoded.width() > 0 && decoded.height() > 0);
    }
}
