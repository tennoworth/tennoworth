use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

pub(in crate::overlay) struct OcrRequest {
    pub(in crate::overlay) image: Vec<u8>,
    pub(in crate::overlay) mode: OcrMode,
    pub(in crate::overlay) reply: mpsc::SyncSender<Result<String, String>>,
}

#[derive(Clone, Copy)]
pub(in crate::overlay) enum OcrMode {
    SparseTsv,
    SingleLine,
}

pub(in crate::overlay) struct OcrWorker {
    pub(in crate::overlay) tx: mpsc::Sender<OcrRequest>,
}

impl OcrWorker {
    pub(in crate::overlay) fn start(
        tessdata: Result<PathBuf, String>,
    ) -> (Self, Result<(), String>) {
        let (tx, rx) = mpsc::channel::<OcrRequest>();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let _ = std::thread::Builder::new()
            .name("relic-ocr".into())
            .spawn(move || {
                let mut engine = tessdata.and_then(|path| {
                    let data = path.to_str().ok_or_else(|| {
                        "ocr_unavailable: tessdata path is not valid UTF-8".to_string()
                    })?;
                    // Tauri canonicalizes bundled resources to a Windows verbatim
                    // path. Tesseract's C API cannot open the `\\?\` form.
                    #[cfg(target_os = "windows")]
                    let data = data.strip_prefix(r"\\?\").unwrap_or(data);
                    leptess::LepTess::new(Some(data), "eng").map_err(|e| {
                        format!(
                            "ocr_unavailable: starting Tesseract with bundled eng.traineddata: {e}"
                        )
                    })
                });
                let _ = ready_tx.send(engine.as_ref().map(|_| ()).map_err(Clone::clone));
                while let Ok(req) = rx.recv() {
                    let answer = match engine.as_mut() {
                        Ok(ocr) => {
                            let page_mode = match req.mode {
                                OcrMode::SparseTsv => "11",
                                OcrMode::SingleLine => "6",
                            };
                            let _ =
                                ocr.set_variable(leptess::Variable::TesseditPagesegMode, page_mode);
                            ocr.set_image_from_mem(&req.image)
                                .map_err(|e| format!("loading capture into OCR: {e}"))
                                .and_then(|_| match req.mode {
                                    OcrMode::SparseTsv => ocr
                                        .get_tsv_text(0)
                                        .map_err(|e| format!("locating reward text: {e}")),
                                    OcrMode::SingleLine => ocr
                                        .get_utf8_text()
                                        .map_err(|e| format!("recognizing reward text: {e}")),
                                })
                        }
                        Err(message) => Err(message.clone()),
                    };
                    let _ = req.reply.send(answer);
                }
            });
        let ready = ready_rx
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| Err("ocr_unavailable: Tesseract initialization timed out".into()));
        (Self { tx }, ready)
    }

    pub(in crate::overlay) fn recognize(
        &self,
        image: Vec<u8>,
        mode: OcrMode,
    ) -> Result<String, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.tx
            .send(OcrRequest {
                image,
                mode,
                reply: tx,
            })
            .map_err(|_| "OCR worker stopped".to_string())?;
        rx.recv_timeout(Duration::from_secs(5))
            .map_err(|_| "OCR timed out".to_string())?
    }
}
