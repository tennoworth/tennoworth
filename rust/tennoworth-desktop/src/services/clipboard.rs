//! Writing text to the system clipboard from Rust.
//!
//! One clipboard handle lives for the whole process: on X11 the owner of a
//! selection must stay alive to serve it, so a handle dropped after copying
//! would take the copied text with it.

use std::sync::Mutex;

static CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

pub(crate) fn copy(text: &str) -> Result<(), String> {
    let mut guard = CLIPBOARD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // A handle whose display connection broke fails every call; reopen once.
    for attempt in 0..2 {
        if guard.is_none() {
            *guard =
                Some(arboard::Clipboard::new().map_err(|e| format!("opening the clipboard: {e}"))?);
        }
        let Some(clipboard) = guard.as_mut() else {
            continue;
        };
        match clipboard.set_text(text) {
            Ok(()) => return Ok(()),
            Err(error) if attempt == 0 => {
                eprintln!("tennoworth: clipboard write failed, reopening: {error}");
                *guard = None;
            }
            Err(error) => return Err(format!("writing the clipboard: {error}")),
        }
    }
    Err("writing the clipboard failed".into())
}
