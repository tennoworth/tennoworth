use super::super::{RelicOverlaySlot, RECOMMENDATION_CONFIDENCE};
use crate::services::sellables::OverlayCatalogItem;

#[derive(Debug, Clone)]
pub(in crate::overlay) struct FoundMatch {
    pub(in crate::overlay) raw: String,
    pub(in crate::overlay) item: OverlayCatalogItem,
    pub(in crate::overlay) confidence: f64,
}

pub(in crate::overlay) fn normalize(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[allow(
    clippy::indexing_slicing,
    reason = "count <= 3.min(lines.len() - start) keeps start..start+count in bounds"
)]
pub(in crate::overlay) fn match_ocr_lines(
    text: &str,
    catalog: &[OverlayCatalogItem],
) -> Vec<FoundMatch> {
    let normalized_catalog: Vec<(String, &OverlayCatalogItem)> = catalog
        .iter()
        .map(|item| (normalize(&item.name), item))
        .filter(|(name, _)| name.len() >= 4 && !name.ends_with(" relic"))
        .collect();
    let mut found: Vec<FoundMatch> = Vec::new();
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| line.len() >= 2)
        .collect();
    let joined = lines.join(" ");
    let mut reads: Vec<String> = Vec::new();
    if joined.len() >= 4 {
        reads.push(joined);
    }
    for start in 0..lines.len() {
        for count in 1..=3.min(lines.len() - start) {
            let read = lines[start..start + count].join(" ");
            if read.len() >= 4 && !reads.contains(&read) {
                reads.push(read);
            }
        }
    }
    for raw in reads {
        let line = normalize(&raw);
        if line.is_empty() {
            continue;
        }
        let mut scores: Vec<(f64, &OverlayCatalogItem)> = normalized_catalog
            .iter()
            .map(|(candidate, item)| {
                let whole_score = if &line == candidate || line.contains(candidate) {
                    1.0
                } else {
                    strsim::normalized_levenshtein(&line, candidate)
                };
                let read_words: Vec<&str> = line.split_whitespace().collect();
                let candidate_words: Vec<&str> = candidate.split_whitespace().collect();
                let positional_score = if read_words.len() == candidate_words.len() {
                    read_words
                        .iter()
                        .zip(candidate_words.iter())
                        .map(|(read, candidate)| strsim::normalized_levenshtein(read, candidate))
                        .sum::<f64>()
                        / read_words.len().max(1) as f64
                } else {
                    0.0
                };
                let score = whole_score.max(positional_score);
                (score, *item)
            })
            .collect();
        scores.sort_by(|a, b| b.0.total_cmp(&a.0));
        let Some((best, item)) = scores.first().copied() else {
            continue;
        };
        let runner_up = scores.get(1).map(|row| row.0).unwrap_or(0.0);
        if best < 0.82 || (best < 1.0 && best - runner_up < 0.08) {
            continue;
        }
        let candidate = FoundMatch {
            raw,
            item: item.clone(),
            confidence: best,
        };
        if let Some(existing) = found
            .iter_mut()
            .find(|existing| existing.item.name.eq_ignore_ascii_case(&item.name))
        {
            if candidate.confidence > existing.confidence {
                *existing = candidate;
            }
        } else {
            found.push(candidate);
        }
        if found.len() == 4 {
            break;
        }
    }
    found
}

pub(in crate::overlay) fn mark_bests(slots: &mut [RelicOverlaySlot]) {
    for slot in slots.iter_mut() {
        slot.best_platinum = false;
        slot.best_ducats = false;
    }
    if slots.iter().any(|slot| slot.name.is_none()) {
        return;
    }
    let best_platinum = slots
        .iter()
        .filter(|slot| slot.confidence >= RECOMMENDATION_CONFIDENCE)
        .filter_map(|slot| slot.live_platinum.or(slot.cached_platinum))
        .max();
    let best_ducats = slots
        .iter()
        .filter(|slot| slot.confidence >= RECOMMENDATION_CONFIDENCE)
        .filter_map(|slot| slot.ducats)
        .max();
    for slot in slots {
        let recommendable = slot.confidence >= RECOMMENDATION_CONFIDENCE;
        slot.best_platinum = recommendable
            && best_platinum.is_some()
            && slot.live_platinum.or(slot.cached_platinum) == best_platinum;
        slot.best_ducats = recommendable && best_ducats.is_some() && slot.ducats == best_ducats;
    }
}
