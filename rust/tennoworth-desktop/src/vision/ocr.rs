//! Text recognition on captured frames: the Tesseract worker and its TSV
//! output. It knows nothing about what a feature is looking for.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

pub(crate) struct OcrRequest {
    pub(crate) image: Vec<u8>,
    pub(crate) mode: OcrMode,
    pub(crate) reply: mpsc::SyncSender<Result<String, String>>,
}

#[derive(Clone, Copy)]
pub(crate) enum OcrMode {
    SparseTsv,
    SingleLine,
}

pub(crate) struct OcrWorker {
    pub(crate) tx: mpsc::Sender<OcrRequest>,
}

impl OcrWorker {
    pub(crate) fn start(tessdata: Result<PathBuf, String>) -> (Self, Result<(), String>) {
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

    pub(crate) fn recognize(&self, image: Vec<u8>, mode: OcrMode) -> Result<String, String> {
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

#[derive(Debug, Clone)]
pub(crate) struct TsvLine {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) text: String,
}

#[allow(
    clippy::indexing_slicing,
    reason = "fields.len() == 12 is checked before every index, and the fields[0] guard filters the rest"
)]
pub(crate) fn parse_tsv_lines(tsv: &str) -> Vec<TsvLine> {
    #[derive(Default)]
    struct LineBuilder {
        left: u32,
        top: u32,
        right: u32,
        bottom: u32,
        words: Vec<String>,
    }

    let mut groups: BTreeMap<(u32, u32, u32, u32), LineBuilder> = BTreeMap::new();
    for row in tsv.lines().skip(1) {
        let fields: Vec<&str> = row.splitn(12, '\t').collect();
        if fields.len() != 12 || fields[0] != "5" {
            continue;
        }
        let parsed = (|| {
            Some((
                fields[1].parse::<u32>().ok()?,
                fields[2].parse::<u32>().ok()?,
                fields[3].parse::<u32>().ok()?,
                fields[4].parse::<u32>().ok()?,
                fields[6].parse::<u32>().ok()?,
                fields[7].parse::<u32>().ok()?,
                fields[8].parse::<u32>().ok()?,
                fields[9].parse::<u32>().ok()?,
            ))
        })();
        let Some((page, block, paragraph, line, x, y, width, height)) = parsed else {
            continue;
        };
        let text = fields[11].trim();
        if text.is_empty() {
            continue;
        }
        let entry = groups.entry((page, block, paragraph, line)).or_default();
        if entry.words.is_empty() {
            entry.left = x;
            entry.top = y;
            entry.right = x.saturating_add(width);
            entry.bottom = y.saturating_add(height);
        } else {
            entry.left = entry.left.min(x);
            entry.top = entry.top.min(y);
            entry.right = entry.right.max(x.saturating_add(width));
            entry.bottom = entry.bottom.max(y.saturating_add(height));
        }
        entry.words.push(text.to_string());
    }
    groups
        .into_values()
        .filter_map(|line| {
            Some(TsvLine {
                x: line.left,
                y: line.top,
                width: line.right.checked_sub(line.left)?,
                height: line.bottom.checked_sub(line.top)?,
                text: line.words.join(" "),
            })
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, reason = "tests index known fixtures")]
mod tests {
    use super::*;

    #[test]
    fn tsv_words_are_grouped_into_positioned_lines() {
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
    5\t1\t51\t1\t1\t1\t1313\t637\t35\t14\t87.7\tCedo\n\
    5\t1\t51\t1\t1\t2\t1354\t638\t41\t13\t95.2\tPrime\n\
    5\t1\t51\t1\t1\t3\t1400\t637\t40\t14\t94.8\tBarrel\n\
    5\t1\t52\t1\t1\t1\t1514\t638\t45\t13\t97.0\tForma\n\
    5\t1\t52\t1\t1\t2\t1564\t637\t64\t18\t96.8\tBlueprint";
        let lines = parse_tsv_lines(tsv);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "Cedo Prime Barrel");
        assert_eq!(
            (lines[0].x, lines[0].y, lines[0].width, lines[0].height),
            (1313, 637, 127, 14)
        );
        assert_eq!(lines[1].text, "Forma Blueprint");
    }
}
