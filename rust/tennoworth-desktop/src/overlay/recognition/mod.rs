use crate::vision::capture::CapturedFrame;
use crate::vision::frame::{encode_crop, encode_frame, NormalizedRect};
use crate::vision::ocr::{parse_tsv_lines, OcrMode, OcrWorker, TsvLine};
use super::{diagnostics_attempt_dir, elapsed_ms, unix_millis, OverlayStageTimings};
use super::{
    OverlayBox, OverlaySettings, RelicOverlayResult, RelicOverlaySlot,
    REWARD_CARD_WIDTH_PER_HEIGHT, REWARD_SLOT_SPACING_PER_HEIGHT, REWARD_TITLE_TARGET_WIDTH,
    WARFRAME_DESIGN_ASPECT,
};
use crate::services::sellables::{OverlayCatalogItem, OverlayMarketFacts};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
mod matching;
pub(super) use matching::{mark_bests, match_ocr_lines, normalize, FoundMatch};

#[derive(Debug, Clone, Serialize)]
pub(super) struct RewardSlotRect {
    pub(super) index: usize,
    pub(super) card: NormalizedRect,
    pub(super) title: NormalizedRect,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct RewardLayout {
    pub(super) count: usize,
    pub(super) confidence: f64,
    pub(super) slots: Vec<RewardSlotRect>,
}

#[derive(Clone)]
pub(super) struct LayoutRead {
    pub(super) layout: RewardLayout,
    pub(super) matches: Vec<(usize, FoundMatch)>,
}

#[derive(Default)]
pub(super) struct RecognitionConsensus {
    pub(super) reads: Vec<LayoutRead>,
}

impl RecognitionConsensus {
    pub(super) fn observe(&mut self, read: LayoutRead) {
        self.reads.push(read);
    }

    pub(super) fn resolve(
        &self,
        expected_slots: usize,
        require_complete: bool,
    ) -> Option<LayoutRead> {
        let mut by_count: BTreeMap<usize, Vec<&LayoutRead>> = BTreeMap::new();
        for read in &self.reads {
            by_count.entry(read.layout.count).or_default().push(read);
        }
        let mut best: Option<(bool, bool, usize, f64, LayoutRead)> = None;
        for reads in by_count.into_values() {
            let layout = reads
                .iter()
                .max_by(|a, b| a.layout.confidence.total_cmp(&b.layout.confidence))?
                .layout
                .clone();
            let mut candidates: BTreeMap<usize, HashMap<String, (usize, FoundMatch)>> =
                BTreeMap::new();
            for read in reads {
                for (index, found) in &read.matches {
                    let identity = found.item.name.to_ascii_lowercase();
                    let entry = candidates
                        .entry(*index)
                        .or_default()
                        .entry(identity)
                        .or_insert_with(|| (0, found.clone()));
                    entry.0 += 1;
                    if found.confidence > entry.1.confidence {
                        entry.1 = found.clone();
                    }
                }
            }
            let mut matches = Vec::new();
            for (index, votes) in candidates {
                let mut ranked: Vec<(usize, FoundMatch)> = votes.into_values().collect();
                ranked.sort_by(|a, b| {
                    b.0.cmp(&a.0)
                        .then_with(|| b.1.confidence.total_cmp(&a.1.confidence))
                });
                let Some((top_votes, top)) = ranked.first() else {
                    continue;
                };
                let unambiguous = ranked.get(1).is_none_or(|(runner_votes, runner)| {
                    top_votes > runner_votes || top.confidence - runner.confidence >= 0.12
                });
                if unambiguous {
                    matches.push((index, top.clone()));
                }
            }
            let read = LayoutRead { layout, matches };
            let complete = layout_read_is_complete(&read, expected_slots);
            if require_complete && !complete {
                continue;
            }
            let expected_count = expected_slots > 0 && read.layout.count == expected_slots;
            let confidence = read.matches.iter().map(|(_, found)| found.confidence).sum();
            let score = (complete, expected_count, read.matches.len(), confidence);
            if best.as_ref().is_none_or(|current| {
                (score.0 && !current.0)
                    || (score.0 == current.0 && score.1 && !current.1)
                    || (score.0 == current.0 && score.1 == current.1 && score.2 > current.2)
                    || (score.0 == current.0
                        && score.1 == current.1
                        && score.2 == current.2
                        && score.3 > current.3)
            }) {
                best = Some((score.0, score.1, score.2, score.3, read));
            }
        }
        best.map(|(_, _, _, _, read)| read)
    }
}

pub(super) fn centered_slot_centers(
    image_width: u32,
    count: usize,
    anchors: &[(&TsvLine, FoundMatch)],
) -> Vec<f64> {
    if count == 1 {
        return vec![image_width as f64 / 2.0];
    }
    let offsets: Vec<f64> = (0..count)
        .map(|index| index as f64 - (count as f64 - 1.0) / 2.0)
        .collect();
    let anchor_x: Vec<f64> = anchors
        .iter()
        .map(|(line, _)| (line.x as f64 + line.width as f64 / 2.0) / image_width as f64)
        .collect();
    let mut candidates = vec![0.077];
    for x in &anchor_x {
        for offset in &offsets {
            if offset.abs() < f64::EPSILON {
                continue;
            }
            let spacing = (*x - 0.5) / offset;
            if (0.05..=0.13).contains(&spacing) {
                candidates.push(spacing);
            }
        }
    }
    let spacing = candidates
        .into_iter()
        .min_by(|a, b| {
            let score = |candidate: f64| {
                let grid: Vec<f64> = offsets
                    .iter()
                    .map(|offset| 0.5 + offset * candidate)
                    .collect();
                let mut used = Vec::new();
                let mut error = 0.0;
                for anchor in &anchor_x {
                    let (index, distance) = grid
                        .iter()
                        .enumerate()
                        .map(|(index, center)| (index, (anchor - center).abs()))
                        .min_by(|left, right| left.1.total_cmp(&right.1))
                        .unwrap_or((0, 1.0));
                    error += distance;
                    if used.contains(&index) {
                        error += 0.05;
                    }
                    used.push(index);
                }
                error
            };
            score(*a).total_cmp(&score(*b))
        })
        .unwrap_or(0.077);
    offsets
        .iter()
        .map(|offset| (0.5 + offset * spacing) * image_width as f64)
        .collect()
}

pub(super) fn read_dynamic_layout(
    ocr: &OcrWorker,
    frame: &CapturedFrame,
    catalog: &[OverlayCatalogItem],
    expected_slots: &AtomicUsize,
    run_dir: Option<&Path>,
    attempt: usize,
    timings: &mut OverlayStageTimings,
) -> Result<LayoutRead, String> {
    let debug_dir = diagnostics_attempt_dir(run_dir, attempt);
    let frame_png = encode_frame(&frame.image)?;
    let started = Instant::now();
    let tsv = ocr
        .recognize(frame_png, OcrMode::SparseTsv)
        .map_err(|e| format!("ocr_unavailable: {e}"))?;
    timings.sparse_ocr_ms += elapsed_ms(started);
    if let Some(dir) = &debug_dir {
        let _ = std::fs::write(dir.join("sparse.tsv"), &tsv);
    }
    let lines = parse_tsv_lines(&tsv);
    let expected_slots = expected_slots.load(Ordering::Acquire);
    let matching_started = Instant::now();
    let slot_before = timings.slot_ocr_ms;
    let result = layout_from_lines(
        frame,
        &lines,
        catalog,
        expected_slots,
        debug_dir.as_deref(),
        |slot| {
            let started = Instant::now();
            let text = encode_crop(&frame.image, slot.title, REWARD_TITLE_TARGET_WIDTH)
                .and_then(|crop| ocr.recognize(crop, OcrMode::SingleLine))
                .unwrap_or_default();
            timings.slot_ocr_ms += elapsed_ms(started);
            text
        },
    );
    timings.matching_ms += elapsed_ms(matching_started)
        .saturating_sub(timings.slot_ocr_ms.saturating_sub(slot_before));
    result.ok_or_else(|| "reward_row_not_detected: no relic reward title row was detected".into())
}

pub(super) fn read_expected_layout(
    ocr: &OcrWorker,
    frame: &CapturedFrame,
    catalog: &[OverlayCatalogItem],
    expected_slots: usize,
    debug_dir: Option<&Path>,
    timings: &mut OverlayStageTimings,
) -> Result<LayoutRead, String> {
    let layout = expected_reward_layout(frame, expected_slots);
    let slots = &layout.slots;
    let mut matches = Vec::new();
    let mut text_log = Vec::new();
    for slot in slots {
        let crop = encode_crop(&frame.image, slot.title, REWARD_TITLE_TARGET_WIDTH)?;
        if let Some(dir) = debug_dir {
            let _ = std::fs::write(dir.join(format!("slot-{}.pgm", slot.index)), &crop);
        }
        let started = Instant::now();
        let text = ocr
            .recognize(crop, OcrMode::SingleLine)
            .map_err(|e| format!("ocr_unavailable: {e}"))?;
        timings.slot_ocr_ms += elapsed_ms(started);
        let started = Instant::now();
        let best = match_ocr_lines(&text, catalog)
            .into_iter()
            .max_by(|a, b| a.confidence.total_cmp(&b.confidence));
        timings.matching_ms += elapsed_ms(started);
        text_log.push(format!("slot {}: {:?}", slot.index, text.trim()));
        if let Some(found) = best {
            matches.push((slot.index, found));
        }
    }
    if let Some(dir) = debug_dir {
        let _ = std::fs::write(dir.join("fast-path.txt"), text_log.join("\n"));
        if let Ok(json) = serde_json::to_vec_pretty(&layout) {
            let _ = std::fs::write(dir.join("layout.json"), json);
        }
    }
    (!matches.is_empty())
        .then_some(LayoutRead { layout, matches })
        .ok_or_else(|| {
            format!(
                "catalog_match_incomplete: recognized 0 of {expected_slots} expected reward names"
            )
        })
}

pub(super) fn centered_layout_is_complete(read: &LayoutRead) -> bool {
    let indices: Vec<usize> = read.matches.iter().map(|(index, _)| *index).collect();
    match read.layout.count {
        // Warframe centers two rewards in the inner positions of its four-card
        // row, and a solo reward in the middle position of its three-card row.
        4 => indices == [0, 1, 2, 3] || indices == [1, 2],
        3 => indices == [0, 1, 2] || indices == [1],
        _ => false,
    }
}

pub(super) fn layout_read_is_complete(read: &LayoutRead, expected_slots: usize) -> bool {
    if expected_slots == 0 {
        return centered_layout_is_complete(read);
    }
    if read.matches.len() != expected_slots {
        return false;
    }
    let indices: Vec<usize> = read.matches.iter().map(|(index, _)| *index).collect();
    match (read.layout.count, expected_slots) {
        (count, expected) if count == expected => indices == (0..expected).collect::<Vec<_>>(),
        (4, 2) => indices == [1, 2],
        (3, 1) => indices == [1],
        _ => false,
    }
}

pub(super) fn read_centered_reward_layout(
    ocr: &OcrWorker,
    frame: &CapturedFrame,
    catalog: &[OverlayCatalogItem],
    debug_dir: Option<&Path>,
    timings: &mut OverlayStageTimings,
) -> Result<LayoutRead, String> {
    let mut diagnostic = Vec::new();
    let mut crop_number = 0usize;
    let matching_started = Instant::now();
    let slot_before = timings.slot_ocr_ms;
    let result = layout_from_reward_header(frame, catalog, &mut diagnostic, &mut |slot| {
        let current_crop = crop_number;
        crop_number += 1;
        let crop = match encode_crop(&frame.image, slot.title, REWARD_TITLE_TARGET_WIDTH) {
            Ok(crop) => crop,
            Err(_) => return String::new(),
        };
        if let Some(dir) = debug_dir {
            let _ = std::fs::write(dir.join(format!("fast-slot-{current_crop}.pgm")), &crop);
        }
        let started = Instant::now();
        let text = ocr.recognize(crop, OcrMode::SingleLine).unwrap_or_default();
        timings.slot_ocr_ms += elapsed_ms(started);
        text
    });
    timings.matching_ms += elapsed_ms(matching_started)
        .saturating_sub(timings.slot_ocr_ms.saturating_sub(slot_before));
    if let Some(dir) = debug_dir {
        let _ = std::fs::write(dir.join("centered-fast-path.txt"), diagnostic.join("\n"));
    }
    result.ok_or_else(|| {
        "centered_reward_incomplete: fixed reward crops did not recognize any reward names".into()
    })
}

pub(super) fn expected_reward_layout(frame: &CapturedFrame, expected_slots: usize) -> RewardLayout {
    let mut layout = reward_header_layout(frame, expected_slots);
    layout.confidence = 1.0;
    layout
}

pub(super) fn reward_header_visible(frame: &CapturedFrame, lines: &[TsvLine]) -> bool {
    let header = lines
        .iter()
        .filter(|line| line.y + line.height / 2 <= frame.height / 5)
        .map(|line| normalize(&line.text))
        .collect::<Vec<_>>()
        .join("");
    header.contains("fissure") && header.contains("reward")
}

pub(super) fn reward_design_viewport(frame: &CapturedFrame) -> (f64, f64) {
    let design_height = (frame.width as f64 / WARFRAME_DESIGN_ASPECT).min(frame.height as f64);
    let top = (frame.height as f64 - design_height) / 2.0;
    (top, design_height)
}

pub(super) fn reward_header_layout(frame: &CapturedFrame, count: usize) -> RewardLayout {
    let (design_top, design_height) = reward_design_viewport(frame);
    let spacing = REWARD_SLOT_SPACING_PER_HEIGHT * design_height / frame.width as f64;
    let width = REWARD_CARD_WIDTH_PER_HEIGHT * design_height / frame.width as f64;
    let card_y = (design_top + design_height * 0.22) / frame.height as f64;
    let card_height = design_height * 0.25 / frame.height as f64;
    let title_y = (design_top + design_height * 0.35) / frame.height as f64;
    let title_height = design_height * 0.11 / frame.height as f64;
    let offsets = (0..count).map(|index| index as f64 - (count as f64 - 1.0) / 2.0);
    let slots = offsets
        .enumerate()
        .map(|(index, offset)| {
            let center = 0.5 + offset * spacing;
            RewardSlotRect {
                index,
                card: NormalizedRect {
                    x: center - width / 2.0,
                    y: card_y,
                    width,
                    height: card_height,
                },
                title: NormalizedRect {
                    x: center - width / 2.0,
                    y: title_y,
                    width,
                    height: title_height,
                },
            }
        })
        .collect();
    RewardLayout {
        count,
        confidence: 0.0,
        slots,
    }
}

pub(super) fn vertical_reward_edge_profile(frame: &CapturedFrame) -> Vec<f64> {
    let image = &frame.image;
    let mut profile = vec![0.0; image.width() as usize];
    let (design_top, design_height) = reward_design_viewport(frame);
    let top = (design_top + design_height * 0.20).round() as u32;
    let bottom = (design_top + design_height * 0.48).round() as u32;
    for x in 2..image.width().saturating_sub(2) {
        let mut strength = 0.0;
        for y in (top..bottom).step_by(2) {
            let left = image.get_pixel(x - 2, y).0;
            let right = image.get_pixel(x + 2, y).0;
            let left_luma = u32::from(left[0]) + u32::from(left[1]) + u32::from(left[2]);
            let right_luma = u32::from(right[0]) + u32::from(right[1]) + u32::from(right[2]);
            strength += left_luma.abs_diff(right_luma) as f64;
        }
        if let Some(value) = profile.get_mut(x as usize) {
            *value = strength;
        }
    }
    profile
}

pub(super) fn local_edge_strength(profile: &[f64], x: f64) -> f64 {
    let center = x.round() as isize;
    (-3..=3)
        .filter_map(|offset| profile.get((center + offset).max(0) as usize))
        .copied()
        .fold(0.0, f64::max)
}

/// Recover the horizontal card grid without invoking OCR. Card borders form a
/// repeated set of long vertical edges, so a small projection over the reward
/// band can adjust the center and spacing before title crops are made.
pub(super) fn detected_reward_header_layout(
    frame: &CapturedFrame,
    count: usize,
) -> Option<RewardLayout> {
    if !(3..=4).contains(&count) {
        return None;
    }
    let profile = vertical_reward_edge_profile(frame);
    let search_left = (frame.width as f64 * 0.18).round() as usize;
    let search_right = (frame.width as f64 * 0.82).round() as usize;
    let background = profile.get(search_left..search_right)?;
    let mean = background.iter().sum::<f64>() / background.len().max(1) as f64;
    let variance = background
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / background.len().max(1) as f64;
    let deviation = variance.sqrt().max(1.0);

    let (design_top, design_height) = reward_design_viewport(frame);
    let nominal_spacing = REWARD_SLOT_SPACING_PER_HEIGHT * design_height;
    let width_ratio = REWARD_CARD_WIDTH_PER_HEIGHT / REWARD_SLOT_SPACING_PER_HEIGHT;
    let step = (design_height / 600.0).round().max(1.0);
    let center = frame.width as f64 / 2.0;
    let mut best: Option<(f64, f64)> = None;
    let spacing_min = nominal_spacing * 0.82;
    let spacing_max = nominal_spacing * 1.18;
    let mut spacing = spacing_min;
    while spacing <= spacing_max {
        let width = spacing * width_ratio;
        let score = (0..count)
            .flat_map(|index| {
                let offset = index as f64 - (count as f64 - 1.0) / 2.0;
                let card_center = center + offset * spacing;
                [card_center - width / 2.0, card_center + width / 2.0]
            })
            .map(|edge| local_edge_strength(&profile, edge))
            .sum::<f64>()
            / (count * 2) as f64;
        if best.is_none_or(|(best_score, _)| score > best_score) {
            best = Some((score, spacing));
        }
        spacing += step;
    }
    let (score, spacing) = best?;
    let confidence = (score - mean) / deviation;
    if confidence < 0.8 {
        return None;
    }
    let width = spacing * width_ratio;
    let card_y = (design_top + design_height * 0.22) / frame.height as f64;
    let card_height = design_height * 0.25 / frame.height as f64;
    let title_y = (design_top + design_height * 0.35) / frame.height as f64;
    let title_height = design_height * 0.11 / frame.height as f64;
    let slots = (0..count)
        .map(|index| {
            let offset = index as f64 - (count as f64 - 1.0) / 2.0;
            let card_center = center + offset * spacing;
            let x = ((card_center - width / 2.0) / frame.width as f64).max(0.0);
            let normalized_width = (width / frame.width as f64).min(1.0 - x);
            RewardSlotRect {
                index,
                card: NormalizedRect {
                    x,
                    y: card_y,
                    width: normalized_width,
                    height: card_height,
                },
                title: NormalizedRect {
                    x,
                    y: title_y,
                    width: normalized_width,
                    height: title_height,
                },
            }
        })
        .collect();
    Some(RewardLayout {
        count,
        confidence,
        slots,
    })
}

pub(super) fn layout_from_reward_header(
    frame: &CapturedFrame,
    catalog: &[OverlayCatalogItem],
    diagnostic: &mut Vec<String>,
    read_crop: &mut impl FnMut(&RewardSlotRect) -> String,
) -> Option<LayoutRead> {
    let mut best: Option<(usize, f64, LayoutRead)> = None;
    // Four-slot geometry also covers the centered two-slot row; three-slot
    // geometry likewise covers a centered solo reward.
    for count in [4, 3] {
        let mut layout = detected_reward_header_layout(frame, count)
            .unwrap_or_else(|| reward_header_layout(frame, count));
        diagnostic.push(format!(
            "header grid {count} geometry confidence={:.2}",
            layout.confidence
        ));
        let mut matches = Vec::new();
        let mut confidence_sum = 0.0;
        for slot in &layout.slots {
            let text = read_crop(slot);
            let found = match_ocr_lines(&text, catalog)
                .into_iter()
                .max_by(|a, b| a.confidence.total_cmp(&b.confidence));
            diagnostic.push(format!(
                "header grid {count} slot {} ocr={:?} match={:?}",
                slot.index,
                text.trim(),
                found
                    .as_ref()
                    .map(|candidate| (&candidate.item.name, candidate.confidence))
            ));
            if let Some(found) = found {
                confidence_sum += found.confidence;
                matches.push((slot.index, found));
            }
        }
        if matches.is_empty() {
            continue;
        }
        layout.confidence = matches.len() as f64 / count as f64;
        let candidate = LayoutRead { layout, matches };
        let score = (candidate.matches.len(), confidence_sum);
        if best
            .as_ref()
            .is_none_or(|(matches, confidence, _)| score > (*matches, *confidence))
        {
            best = Some((score.0, score.1, candidate));
        }
    }
    best.map(|(_, _, read)| read)
}

/// The post-OCR half of read_dynamic_layout: find the reward band from the
/// sparse-TSV lines, recover the slot grid, and match each slot crop against
/// the catalog. Pure (the crop reader is injected) so the corpus gate can feed
/// synthetic screens without a Tesseract engine.
#[allow(
    clippy::indexing_slicing,
    reason = "windows(2) pairs are exactly two elements; gaps is non-empty whenever the centers.len() > 1 branch runs"
)]
pub(super) fn layout_from_lines(
    frame: &CapturedFrame,
    lines: &[TsvLine],
    catalog: &[OverlayCatalogItem],
    expected_slots: usize,
    debug_dir: Option<&std::path::Path>,
    mut read_crop: impl FnMut(&RewardSlotRect) -> String,
) -> Option<LayoutRead> {
    let mut diagnostic = Vec::new();
    let has_reward_header = reward_header_visible(frame, lines);
    let anchors: Vec<(&TsvLine, FoundMatch)> = lines
        .iter()
        .filter(|line| {
            let center_x = line.x as f64 + line.width as f64 / 2.0;
            let center_y = line.y as f64 + line.height as f64 / 2.0;
            center_x >= frame.width as f64 * 0.2
                && center_x <= frame.width as f64 * 0.8
                && center_y >= frame.height as f64 * 0.15
                && center_y <= frame.height as f64 * 0.72
        })
        .filter_map(|line| {
            match_ocr_lines(&line.text, catalog)
                .into_iter()
                .max_by(|a, b| a.confidence.total_cmp(&b.confidence))
                .map(|found| (line, found))
        })
        .collect();
    let anchor_y = anchors
        .iter()
        .map(|(line, _)| line.y + line.height / 2)
        .min();
    let Some(anchor_y) = anchor_y else {
        let result = has_reward_header
            .then(|| layout_from_reward_header(frame, catalog, &mut diagnostic, &mut read_crop))
            .flatten();
        if let Some(dir) = debug_dir {
            let _ = std::fs::write(dir.join("ocr.txt"), diagnostic.join("\n"));
        }
        return result;
    };
    let y_tolerance = (frame.height as f64 * 0.025).round() as u32;
    let mut row: Vec<&TsvLine> = lines
        .iter()
        .filter(|line| {
            let center_x = line.x as f64 + line.width as f64 / 2.0;
            let center_y = line.y + line.height / 2;
            let normalized = normalize(&line.text);
            center_x >= frame.width as f64 * 0.2
                && center_x <= frame.width as f64 * 0.8
                && center_y.abs_diff(anchor_y) <= y_tolerance
                && line.width >= frame.width / 40
                && line.width <= frame.width / 5
                && line.height >= frame.height / 200
                && line.height <= frame.height / 14
                && normalized.len() >= 6
        })
        .collect();
    row.sort_by_key(|line| line.x);
    row.dedup_by(|a, b| {
        let a_center = a.x + a.width / 2;
        let b_center = b.x + b.width / 2;
        a_center.abs_diff(b_center) < frame.width / 50
    });
    if row.is_empty() || (expected_slots == 0 && row.len() > 4) {
        diagnostic.push(format!("dynamic title row had {} candidates", row.len()));
        let result = has_reward_header
            .then(|| layout_from_reward_header(frame, catalog, &mut diagnostic, &mut read_crop))
            .flatten();
        if let Some(dir) = debug_dir {
            let _ = std::fs::write(dir.join("ocr.txt"), diagnostic.join("\n"));
        }
        return result;
    }

    let observed_centers: Vec<f64> = row
        .iter()
        .map(|line| line.x as f64 + line.width as f64 / 2.0)
        .collect();
    let count = if (1..=4).contains(&expected_slots) {
        expected_slots
    } else {
        observed_centers.len()
    };
    let centers: Vec<f64> = if (1..=4).contains(&expected_slots) {
        centered_slot_centers(frame.width, expected_slots, &anchors)
    } else {
        observed_centers.clone()
    };
    let card_width = if centers.len() > 1 {
        let mut gaps: Vec<f64> = centers.windows(2).map(|pair| pair[1] - pair[0]).collect();
        gaps.sort_by(|a, b| a.total_cmp(b));
        gaps[gaps.len() / 2] * 0.92
    } else {
        frame.width as f64 * 0.13
    }
    .clamp(frame.width as f64 * 0.065, frame.width as f64 * 0.18);
    let row_top = row.iter().map(|line| line.y).min()? as f64;
    let row_bottom = row.iter().map(|line| line.y + line.height).max()? as f64;
    let crop_top = (row_top - frame.height as f64 * 0.012).max(0.0);
    let crop_bottom = (row_bottom + frame.height as f64 * 0.012).min(frame.height as f64);
    let slots: Vec<RewardSlotRect> = centers
        .iter()
        .enumerate()
        .map(|(index, center)| {
            let title = NormalizedRect {
                x: ((center - card_width / 2.0) / frame.width as f64).max(0.0),
                y: crop_top / frame.height as f64,
                width: card_width / frame.width as f64,
                height: (crop_bottom - crop_top) / frame.height as f64,
            };
            RewardSlotRect {
                index,
                card: NormalizedRect {
                    x: title.x,
                    y: (title.y - 0.20).max(0.0),
                    width: title.width,
                    height: 0.28,
                },
                title,
            }
        })
        .collect();
    let mut matches = Vec::new();
    for slot in &slots {
        let crop = encode_crop(&frame.image, slot.title, REWARD_TITLE_TARGET_WIDTH).ok()?;
        if let Some(dir) = &debug_dir {
            let _ = std::fs::write(dir.join(format!("slot-{}.pgm", slot.index)), &crop);
        }
        let text = read_crop(slot);
        let best = match_ocr_lines(&text, catalog)
            .into_iter()
            .max_by(|a, b| a.confidence.total_cmp(&b.confidence));
        diagnostic.push(format!(
            "slot {} ocr={:?} match={:?}",
            slot.index,
            text.trim(),
            best.as_ref()
                .map(|found| (&found.item.name, found.confidence))
        ));
        if let Some(found) = best {
            matches.push((slot.index, found));
        }
    }
    let required = if expected_slots > 0 {
        slots.len()
    } else {
        slots.len().min(2)
    };
    let confidence = matches.len() as f64 / slots.len() as f64;
    let layout = RewardLayout {
        count,
        confidence,
        slots,
    };
    if let Some(dir) = debug_dir {
        if let Ok(json) = serde_json::to_vec_pretty(&layout) {
            let _ = std::fs::write(dir.join("layout.json"), json);
        }
        let _ = std::fs::write(dir.join("ocr.txt"), diagnostic.join("\n"));
    }
    (matches.len() >= required).then_some(LayoutRead { layout, matches })
}

pub(super) fn compact_overlay_geometry(
    frame: &CapturedFrame,
    slots: &mut [RelicOverlaySlot],
) -> (i32, i32, u32, u32) {
    let left = (slots.iter().map(|slot| slot.box_.x).fold(1.0, f64::min) - 0.008).max(0.0);
    let right = (slots
        .iter()
        .map(|slot| slot.box_.x + slot.box_.width)
        .fold(0.0, f64::max)
        + 0.008)
        .min(1.0);
    let top = (slots.iter().map(|slot| slot.box_.y).fold(1.0, f64::min) - 0.005).max(0.0);
    let bottom = (slots
        .iter()
        .map(|slot| slot.box_.y + slot.box_.height)
        .fold(0.0, f64::max)
        + 0.015)
        .min(1.0);
    let span_x = (right - left).max(0.01);
    let span_y = (bottom - top).max(0.01);
    for slot in slots {
        slot.box_.x = (slot.box_.x - left) / span_x;
        slot.box_.y = (slot.box_.y - top) / span_y;
        slot.box_.width /= span_x;
        slot.box_.height /= span_y;
    }
    (
        frame.x + (left * frame.width as f64).floor() as i32,
        frame.y + (top * frame.height as f64).floor() as i32,
        (span_x * frame.width as f64).ceil().max(1.0) as u32,
        (span_y * frame.height as f64).ceil().max(1.0) as u32,
    )
}

/// Assemble recognized matches into positioned, fact-joined overlay slots plus
/// the compact window geometry. Missing matches remain visible placeholders;
/// recommendations stay off until the complete row is known.
pub(super) fn assemble_result(
    frame: &CapturedFrame,
    read: LayoutRead,
    settings: &OverlaySettings,
    expected_slots: usize,
    market_facts: impl Fn(Option<&str>) -> OverlayMarketFacts,
    owned: Option<&HashMap<String, u32>>,
    capture_id: String,
) -> (RelicOverlayResult, (i32, i32, u32, u32)) {
    let complete = layout_read_is_complete(&read, expected_slots);
    let panel_y = read
        .layout
        .slots
        .iter()
        .map(|slot| slot.card.y + slot.card.height)
        .fold(0.5, f64::max)
        + 0.035;
    let panel_y = panel_y.min(0.82);
    let mut layout = read.layout;
    let mut matches: HashMap<usize, FoundMatch> = read.matches.into_iter().collect();
    if complete {
        // A complete two-card or solo row may occupy only the inner positions
        // of a wider detection grid. Those outer positions are not rewards.
        layout
            .slots
            .retain(|slot| matches.contains_key(&slot.index));
    }
    let mut slots: Vec<RelicOverlaySlot> = layout
        .slots
        .iter()
        .map(|detected| {
            if let Some(found) = matches.remove(&detected.index) {
                let facts = market_facts(found.item.slug.as_deref());
                return RelicOverlaySlot {
                    index: detected.index,
                    box_: OverlayBox {
                        x: detected.card.x,
                        y: panel_y,
                        width: detected.card.width,
                        height: 0.14,
                    },
                    raw_text: found.raw,
                    name: Some(found.item.name),
                    slug: found.item.slug.clone(),
                    confidence: found.confidence,
                    cached_platinum: facts.cached_platinum,
                    live_platinum: None,
                    ducats: facts.ducats,
                    owned: match (owned, found.item.slug.as_deref()) {
                        (Some(totals), Some(slug)) => Some(*totals.get(slug).unwrap_or(&0)),
                        _ => None,
                    },
                    best_platinum: false,
                    best_ducats: false,
                };
            }
            RelicOverlaySlot {
                index: detected.index,
                box_: OverlayBox {
                    x: detected.card.x,
                    y: panel_y,
                    width: detected.card.width,
                    height: 0.14,
                },
                raw_text: "Reward not recognized".into(),
                name: None,
                slug: None,
                confidence: 0.0,
                cached_platinum: None,
                live_platinum: None,
                ducats: None,
                owned: None,
                best_platinum: false,
                best_ducats: false,
            }
        })
        .collect();
    mark_bests(&mut slots);
    let overlay_geometry = compact_overlay_geometry(frame, &mut slots);
    let result = RelicOverlayResult {
        capture_id,
        captured_at: unix_millis().to_string(),
        scale: settings.scale,
        slots,
    };
    (result, overlay_geometry)
}
