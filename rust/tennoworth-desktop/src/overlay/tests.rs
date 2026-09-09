use super::recognition::*;
use super::*;
use crate::services::sellables::{OverlayCatalogItem, OverlayMarketFacts};
use image::{imageops::FilterType, RgbaImage};
use std::collections::HashMap;

fn catalog() -> Vec<OverlayCatalogItem> {
    vec![
        OverlayCatalogItem {
            name: "Paris Prime Blueprint".into(),
            slug: Some("paris_prime_blueprint".into()),
        },
        OverlayCatalogItem {
            name: "Wisp Prime Systems Blueprint".into(),
            slug: Some("wisp_prime_systems_blueprint".into()),
        },
        OverlayCatalogItem {
            name: "Forma Blueprint".into(),
            slug: None,
        },
    ]
}

fn positioned_slot(index: usize, x: f64) -> RelicOverlaySlot {
    RelicOverlaySlot {
        index,
        box_: OverlayBox {
            x,
            y: 0.54,
            width: 0.07,
            height: 0.14,
        },
        raw_text: String::new(),
        name: None,
        slug: None,
        confidence: 1.0,
        cached_platinum: None,
        live_platinum: None,
        ducats: None,
        owned: None,
        best_platinum: false,
        best_ducats: false,
    }
}

#[test]
fn exact_and_guarded_fuzzy_matches_resolve_but_noise_does_not() {
    let got = match_ocr_lines("SELECT A REWARD\nParis Prime Blueprint\nWisp Prime Systerns Blueprint\nrandom mission text", &catalog());
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].item.slug.as_deref(), Some("paris_prime_blueprint"));
    assert_eq!(got[0].confidence, 1.0);
    assert_eq!(got[1].item.name, "Wisp Prime Systems Blueprint");
    assert!(got[1].confidence >= 0.82 && got[1].confidence < 1.0);
}

#[test]
fn relic_inventory_labels_are_not_reward_candidates() {
    let mut candidates = catalog();
    candidates.push(OverlayCatalogItem {
        name: "Axi V13 Relic".into(),
        slug: Some("axi_v13_relic".into()),
    });

    let got = match_ocr_lines(
        "VOID RELICS / REFINEMENT\nAxi V13 Relic\nAxi V14 Relic",
        &candidates,
    );
    assert!(got.is_empty());
}

#[test]
fn untradeable_rewards_stay_recognized_without_a_slug() {
    let got = match_ocr_lines("Forma Blueprint", &catalog());
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].item.slug, None);
}

#[test]
fn surrounding_crop_noise_does_not_downgrade_an_exact_reward_phrase() {
    let got = match_ocr_lines("aS i@\nWisp Prime Systems Blueprint\n-", &catalog());
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].item.name, "Wisp Prime Systems Blueprint");
    assert_eq!(got[0].confidence, 1.0);
}

#[test]
fn wrapped_reward_title_tolerates_one_bad_word() {
    let mut candidates = catalog();
    candidates.extend([
        OverlayCatalogItem {
            name: "Grendel Prime Chassis Blueprint".into(),
            slug: Some("grendel_prime_chassis_blueprint".into()),
        },
        OverlayCatalogItem {
            name: "Grendel Prime Systems Blueprint".into(),
            slug: Some("grendel_prime_systems_blueprint".into()),
        },
    ]);
    let got = match_ocr_lines(
        "screen noise\nGrendel Brin Chassis\nBlueprint\nmore noise",
        &candidates,
    );
    let grendel = got
        .iter()
        .find(|found| found.item.name == "Grendel Prime Chassis Blueprint")
        .expect("wrapped title should match the chassis reward");
    assert!(grendel.confidence >= 0.82);
}

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

#[test]
fn one_outer_anchor_recovers_a_centered_four_slot_grid() {
    let line = TsvLine {
        x: 1501,
        y: 627,
        width: 139,
        height: 24,
        text: "Braton Prime Barrel".into(),
    };
    let found = FoundMatch {
        raw: line.text.clone(),
        item: OverlayCatalogItem {
            name: line.text.clone(),
            slug: Some("braton_prime_barrel".into()),
        },
        confidence: 1.0,
    };
    let anchors = vec![(&line, found)];
    let centers = centered_slot_centers(2560, 4, &anchors);
    assert_eq!(centers.len(), 4);
    assert!((centers[0] - 989.5).abs() < 2.0);
    assert!((centers[3] - 1570.5).abs() < 2.0);
}

#[test]
fn expected_slot_fast_path_uses_the_height_scaled_reward_grid() {
    let image = RgbaImage::new(2048, 1152);
    let frame = CapturedFrame {
        image,
        x: 0,
        y: 0,
        width: 2048,
        height: 1152,
    };
    let layout = expected_reward_layout(&frame, 3);
    let centers: Vec<f64> = layout
        .slots
        .iter()
        .map(|slot| (slot.title.x + slot.title.width / 2.0) * frame.width as f64)
        .collect();
    let spacing = frame.height as f64 * REWARD_SLOT_SPACING_PER_HEIGHT;
    assert!((centers[0] - (1024.0 - spacing)).abs() < 0.01);
    assert!((centers[1] - 1024.0).abs() < 0.01);
    assert!((centers[2] - (1024.0 + spacing)).abs() < 0.01);
}

#[test]
fn partial_attempts_resolve_one_complete_reward_row() {
    let frame = corpus_frame();
    let catalog = catalog();
    let reads = [
        (0, "Paris Prime Blueprint"),
        (1, "Wisp Prime Systems Blueprint"),
        (2, "Forma Blueprint"),
    ];
    let mut consensus = RecognitionConsensus::default();
    for (index, text) in reads {
        let found = match_ocr_lines(text, &catalog)
            .into_iter()
            .max_by(|a, b| a.confidence.total_cmp(&b.confidence))
            .unwrap();
        consensus.observe(LayoutRead {
            layout: reward_header_layout(&frame, 3),
            matches: vec![(index, found)],
        });
    }
    let resolved = consensus.resolve(3, true).unwrap();
    assert_eq!(
        resolved
            .matches
            .iter()
            .map(|(_, found)| found.item.name.as_str())
            .collect::<Vec<_>>(),
        [
            "Paris Prime Blueprint",
            "Wisp Prime Systems Blueprint",
            "Forma Blueprint"
        ]
    );
}

#[test]
fn partial_consensus_prefers_the_expected_slot_count() {
    let frame = corpus_frame();
    let found = match_ocr_lines("Paris Prime Blueprint", &catalog())
        .into_iter()
        .next()
        .unwrap();
    let mut consensus = RecognitionConsensus::default();
    consensus.observe(LayoutRead {
        layout: reward_header_layout(&frame, 4),
        matches: vec![(1, found.clone()), (2, found.clone())],
    });
    consensus.observe(LayoutRead {
        layout: reward_header_layout(&frame, 3),
        matches: vec![(0, found)],
    });
    let resolved = consensus.resolve(3, false).unwrap();
    assert_eq!(resolved.layout.count, 3);
}

#[test]
fn partial_results_keep_placeholders_and_suppress_recommendations() {
    let frame = corpus_frame();
    let candidates = corpus_catalog();
    let find = |name| {
        match_ocr_lines(name, &candidates)
            .into_iter()
            .next()
            .unwrap()
    };
    let read = LayoutRead {
        layout: reward_header_layout(&frame, 3),
        matches: vec![
            (0, find("Paris Prime Blueprint")),
            (2, find("Cedo Prime Barrel")),
        ],
    };
    let (result, _) = assemble_result(
        &frame,
        read,
        &OverlaySettings::default(),
        3,
        corpus_facts(
            &[("paris_prime_blueprint", 15), ("cedo_prime_barrel", 30)],
            &[("paris_prime_blueprint", 45), ("cedo_prime_barrel", 100)],
        ),
        None,
        "partial".into(),
    );
    assert_eq!(result.slots.len(), 3);
    assert_eq!(result.slots[1].name, None);
    assert_eq!(result.slots[1].raw_text, "Reward not recognized");
    assert!(result
        .slots
        .iter()
        .all(|slot| !slot.best_platinum && !slot.best_ducats));
}

#[test]
fn centered_rows_distinguish_empty_positions_from_missing_rewards() {
    let frame = corpus_frame();
    let catalog = corpus_catalog();
    for (grid, indices) in [(4, vec![1, 2]), (3, vec![1])] {
        for expected in [0, indices.len(), grid] {
            let read = LayoutRead {
                layout: reward_header_layout(&frame, grid),
                matches: indices
                    .iter()
                    .map(|index| {
                        (
                            *index,
                            match_ocr_lines("Paris Prime Blueprint", &catalog)
                                .into_iter()
                                .next()
                                .unwrap(),
                        )
                    })
                    .collect(),
            };
            let (result, _) = assemble_result(
                &frame,
                read,
                &OverlaySettings::default(),
                expected,
                corpus_facts(&[("paris_prime_blueprint", 15)], &[]),
                None,
                "centered".into(),
            );
            if expected == grid {
                assert_eq!(result.slots.len(), grid);
                assert_eq!(
                    result
                        .slots
                        .iter()
                        .filter(|slot| slot.name.is_none())
                        .count(),
                    grid - indices.len()
                );
                assert!(result
                    .slots
                    .iter()
                    .all(|slot| !slot.best_platinum && !slot.best_ducats));
            } else {
                assert_eq!(result.slots.len(), indices.len());
                assert!(result
                    .slots
                    .iter()
                    .all(|slot| slot.name.is_some() && slot.best_platinum));
            }
        }
    }
}

#[test]
fn reward_grid_uses_viewport_height_across_aspect_ratios() {
    let frame_16_9 = CapturedFrame {
        image: RgbaImage::new(2560, 1440),
        x: 0,
        y: 0,
        width: 2560,
        height: 1440,
    };
    let frame_ultrawide = CapturedFrame {
        image: RgbaImage::new(3440, 1440),
        x: 0,
        y: 0,
        width: 3440,
        height: 1440,
    };
    let frame_narrow = CapturedFrame {
        image: RgbaImage::new(795, 632),
        x: 0,
        y: 0,
        width: 795,
        height: 632,
    };
    let spacing = |layout: &RewardLayout, width: u32| {
        let center = |index: usize| {
            let title = layout.slots[index].title;
            (title.x + title.width / 2.0) * width as f64
        };
        center(1) - center(0)
    };
    let standard = reward_header_layout(&frame_16_9, 4);
    let ultrawide = reward_header_layout(&frame_ultrawide, 4);
    let narrow = reward_header_layout(&frame_narrow, 4);
    assert!((spacing(&standard, 2560) - spacing(&ultrawide, 3440)).abs() < 0.01);
    assert!((spacing(&standard, 2560) - 1440.0 * REWARD_SLOT_SPACING_PER_HEIGHT).abs() < 0.01);
    let narrow_design_height = 795.0 / WARFRAME_DESIGN_ASPECT;
    assert!(
        (spacing(&narrow, 795) - narrow_design_height * REWARD_SLOT_SPACING_PER_HEIGHT).abs()
            < 0.01
    );
    let narrow_title_top = narrow.slots[0].title.y * frame_narrow.height as f64;
    assert!((narrow_title_top - 249.0).abs() < 1.0);
}

#[test]
fn reward_title_crops_are_normalized_for_ocr() {
    let image = RgbaImage::from_pixel(512, 100, image::Rgba([240, 120, 60, 80]));
    let pgm = encode_crop(
        &image,
        NormalizedRect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        },
    )
    .unwrap();
    let header = b"P5\n256 50\n255\n";
    assert!(pgm.starts_with(header));
    let pixels = &pgm[header.len()..];
    assert_eq!(pixels.len(), REWARD_TITLE_TARGET_WIDTH as usize * 50);
    assert!(pixels.iter().all(|pixel| *pixel == 149));
}

#[test]
fn repeated_card_edges_recover_a_shifted_reward_grid() {
    let mut image = RgbaImage::new(2560, 1440);
    let center = 1280.0;
    let spacing = 330.0;
    let width = spacing * REWARD_CARD_WIDTH_PER_HEIGHT / REWARD_SLOT_SPACING_PER_HEIGHT;
    for index in 0..4 {
        let offset = index as f64 - 1.5;
        let card_center = center + offset * spacing;
        for edge in [card_center - width / 2.0, card_center + width / 2.0] {
            let x = edge.round() as u32;
            for y in 288..692 {
                image.put_pixel(x, y, image::Rgba([220, 220, 220, 255]));
            }
        }
    }
    let frame = CapturedFrame {
        image,
        x: 0,
        y: 0,
        width: 2560,
        height: 1440,
    };
    let layout = detected_reward_header_layout(&frame, 4).expect("detect reward grid");
    let centers: Vec<f64> = layout
        .slots
        .iter()
        .map(|slot| (slot.title.x + slot.title.width / 2.0) * frame.width as f64)
        .collect();
    assert!((centers[0] - (center - 1.5 * spacing)).abs() < 5.0);
    assert!((centers[3] - (center + 1.5 * spacing)).abs() < 5.0);
}

#[test]
fn fragmented_reward_header_falls_back_to_centered_title_crops() {
    let frame = CapturedFrame {
        image: RgbaImage::new(1898, 1024),
        x: 0,
        y: 0,
        width: 1898,
        height: 1024,
    };
    let lines = vec![TsvLine {
        x: 388,
        y: 66,
        width: 372,
        height: 52,
        text: "FISSURE/REWARDS".into(),
    }];
    let candidates = vec![
        OverlayCatalogItem {
            name: "Wisp Prime Neuroptics Blueprint".into(),
            slug: Some("wisp_prime_neuroptics_blueprint".into()),
        },
        OverlayCatalogItem {
            name: "Lavos Prime Chassis Blueprint".into(),
            slug: Some("lavos_prime_chassis_blueprint".into()),
        },
        OverlayCatalogItem {
            name: "Protea Prime Chassis Blueprint".into(),
            slug: Some("protea_prime_chassis_blueprint".into()),
        },
        OverlayCatalogItem {
            name: "Dethcube Prime Cerebrum".into(),
            slug: Some("dethcube_prime_cerebrum".into()),
        },
    ];
    let names = [
        "Wisp Prime Neuroptics Blueprint",
        "Lavos Prime Chassis Blueprint",
        "Protea Prime Chassis Blueprint",
        "Dethcube Prime Cerebrum",
    ];
    let read = layout_from_lines(&frame, &lines, &candidates, 0, None, |slot| {
        let center = slot.title.x + slot.title.width / 2.0;
        [0.3215, 0.4405, 0.5595, 0.6785]
            .iter()
            .position(|expected| (center - expected).abs() < 0.001)
            .map(|index| names[index].to_string())
            .unwrap_or_else(|| "screen noise".into())
    })
    .expect("the reward header should activate centered crop fallback");
    assert_eq!(read.layout.count, 4);
    assert_eq!(read.matches.len(), 4);
}

#[test]
fn centered_crop_fallback_requires_the_reward_header() {
    let frame = CapturedFrame {
        image: RgbaImage::new(1898, 1024),
        x: 0,
        y: 0,
        width: 1898,
        height: 1024,
    };
    let lines = vec![TsvLine {
        x: 388,
        y: 66,
        width: 372,
        height: 52,
        text: "MISSION COMPLETE".into(),
    }];
    assert!(layout_from_lines(&frame, &lines, &catalog(), 0, None, |_| {
        "Paris Prime Blueprint".into()
    })
    .is_none());
}

#[test]
fn overlay_window_is_compact_and_slot_positions_become_window_relative() {
    let image = RgbaImage::new(2560, 1440);
    let frame = CapturedFrame {
        image,
        x: 0,
        y: 0,
        width: 2560,
        height: 1440,
    };
    let mut slots = vec![positioned_slot(0, 0.35), positioned_slot(3, 0.58)];
    let (x, y, width, height) = compact_overlay_geometry(&frame, &mut slots);
    assert_eq!((x, y), (875, 770));
    assert!(width < 900 && height < 240);
    assert!(slots[0].box_.x < 0.05);
    assert!(slots[1].box_.x > 0.7);
}

#[test]
fn best_marks_use_live_over_cached_and_keep_ties() {
    let mut slots = vec![
        RelicOverlaySlot {
            index: 0,
            box_: OverlayBox {
                x: 0.0,
                y: 0.0,
                width: 0.5,
                height: 1.0,
            },
            raw_text: "a".into(),
            name: Some("a".into()),
            slug: Some("a".into()),
            confidence: 1.0,
            cached_platinum: Some(50),
            live_platinum: Some(20),
            ducats: Some(100),
            owned: Some(1),
            best_platinum: false,
            best_ducats: false,
        },
        RelicOverlaySlot {
            index: 1,
            box_: OverlayBox {
                x: 0.5,
                y: 0.0,
                width: 0.5,
                height: 1.0,
            },
            raw_text: "b".into(),
            name: Some("b".into()),
            slug: Some("b".into()),
            confidence: 1.0,
            cached_platinum: Some(30),
            live_platinum: None,
            ducats: Some(100),
            owned: Some(0),
            best_platinum: false,
            best_ducats: false,
        },
    ];
    mark_bests(&mut slots);
    assert!(!slots[0].best_platinum);
    assert!(slots[1].best_platinum);
    assert!(slots.iter().all(|slot| slot.best_ducats));
}

#[test]
fn low_confidence_matches_never_receive_a_recommendation() {
    let mut slots = vec![
        RelicOverlaySlot {
            index: 0,
            box_: OverlayBox {
                x: 0.0,
                y: 0.0,
                width: 0.5,
                height: 1.0,
            },
            raw_text: "a".into(),
            name: Some("a".into()),
            slug: Some("a".into()),
            confidence: 0.89,
            cached_platinum: Some(500),
            live_platinum: None,
            ducats: Some(100),
            owned: None,
            best_platinum: false,
            best_ducats: false,
        },
        RelicOverlaySlot {
            index: 1,
            box_: OverlayBox {
                x: 0.5,
                y: 0.0,
                width: 0.5,
                height: 1.0,
            },
            raw_text: "b".into(),
            name: Some("b".into()),
            slug: Some("b".into()),
            confidence: 1.0,
            cached_platinum: Some(10),
            live_platinum: None,
            ducats: Some(15),
            owned: None,
            best_platinum: false,
            best_ducats: false,
        },
    ];
    mark_bests(&mut slots);
    assert!(!slots[0].best_platinum && !slots[0].best_ducats);
    assert!(slots[1].best_platinum && slots[1].best_ducats);
}

#[test]
fn reward_marker_updates_are_bounded_and_trimmed() {
    install_reward_markers(&["  Rewards ready  ".into()]).unwrap();
    assert_eq!(&*REWARD_MARKERS.read().unwrap(), &["Rewards ready"]);
    assert!(install_reward_markers(&["".into()]).is_err());
    install_reward_markers(&[DEFAULT_REWARD_MARKER.into()]).unwrap();
}

#[test]
fn recent_log_snapshot_finds_only_an_active_reward_batch() {
    for slots in 2..=4 {
        let active = format!(
            "1.0 Script [Info]: ProjectionRewardChoice.lua: Got rewards\n{}",
            "1.1 Script [Info]: ProjectionRewardChoice.lua: Missing icon data!\n".repeat(slots)
        );
        let batch = latest_active_reward_batch(&active).unwrap();
        assert!(batch.0.contains("Got rewards"));
        assert_eq!(batch.1, slots);

        let closed = format!("{active}2.0 Script [Info]: {REWARD_CLOSE_MARKER}\n");
        assert!(latest_active_reward_batch(&closed).is_none());
    }
}

#[test]
fn shared_result_fixture_obeys_the_recommendation_contract() {
    let raw = include_str!("../../../../tests/fixtures/relic-ocr/result.json");
    let expected: RelicOverlayResult = serde_json::from_str(raw).unwrap();
    let expected_marks: Vec<(bool, bool)> = expected
        .slots
        .iter()
        .map(|slot| (slot.best_platinum, slot.best_ducats))
        .collect();
    let mut actual = expected.slots;
    mark_bests(&mut actual);
    assert_eq!(
        actual
            .iter()
            .map(|slot| (slot.best_platinum, slot.best_ducats))
            .collect::<Vec<_>>(),
        expected_marks
    );
}

#[test]
fn settings_validation_clamps_scale_and_refuses_empty_shortcuts() {
    assert!(!OverlaySettings::default().diagnostics);
    let settings = OverlaySettings {
        scale: 9.0,
        ..OverlaySettings::default()
    };
    assert_eq!(validate_settings(settings).unwrap().scale, 1.5);
    let settings = OverlaySettings {
        shortcut: "   ".into(),
        ..OverlaySettings::default()
    };
    assert!(validate_settings(settings).is_err());
}

#[test]
fn diagnostics_retention_keeps_the_newest_ten_runs() {
    let root = std::env::temp_dir().join(format!(
        "tennoworth-overlay-retention-{}-{}",
        std::process::id(),
        unix_millis()
    ));
    for index in 0..12 {
        create_diagnostics_run(&root, &format!("{index:02}")).expect("create diagnostics run");
    }
    let mut names: Vec<_> = std::fs::read_dir(&root)
        .expect("read diagnostics root")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        (2..12)
            .map(|index| format!("{index:02}"))
            .collect::<Vec<_>>()
    );
    std::fs::remove_dir_all(root).expect("clean diagnostics fixture");
}

// ---- corpus gate: the plan's acceptance criteria, driven end to end ----
//
// ">=95% correct visible-slot recognition" and "0 wrong best-pick marks in
// the gate corpus": synthetic reward screens run through the REAL pipeline
// (parse_tsv_lines -> layout_from_lines -> assemble_result) with the OCR
// seams injected, and every recognized slot and best-pick mark is asserted.

fn corpus_frame() -> CapturedFrame {
    CapturedFrame {
        image: RgbaImage::new(2560, 1440),
        x: 0,
        y: 0,
        width: 2560,
        height: 1440,
    }
}

/// A reward band: one line per name, left to right, centered in the band
/// the pipeline filters on, with mission/HUD noise around it.
fn corpus_screen(names: &[&str]) -> Vec<TsvLine> {
    let n = names.len().max(1);
    let spacing = 2560.0 / (n as f64 + 1.0);
    let mut lines = vec![
        TsvLine {
            x: 200,
            y: 120,
            width: 300,
            height: 20,
            text: "VOID FISSURE COMPLETE".into(),
        },
        TsvLine {
            x: 300,
            y: 900,
            width: 260,
            height: 18,
            text: "Mission reward objective text".into(),
        },
    ];
    for (i, name) in names.iter().enumerate() {
        lines.push(TsvLine {
            x: (spacing * (i as f64 + 1.0) - 100.0).round() as u32,
            y: 708,
            width: 200,
            height: 24,
            text: name.to_string(),
        });
    }
    lines
}

fn corpus_catalog() -> Vec<OverlayCatalogItem> {
    vec![
        OverlayCatalogItem {
            name: "Paris Prime Blueprint".into(),
            slug: Some("paris_prime_blueprint".into()),
        },
        OverlayCatalogItem {
            name: "Wisp Prime Systems Blueprint".into(),
            slug: Some("wisp_prime_systems_blueprint".into()),
        },
        OverlayCatalogItem {
            name: "Cedo Prime Barrel".into(),
            slug: Some("cedo_prime_barrel".into()),
        },
        OverlayCatalogItem {
            name: "Forma Blueprint".into(),
            slug: None,
        },
    ]
}

fn corpus_facts(
    platinum: &[(&str, u32)],
    ducats: &[(&str, u32)],
) -> impl Fn(Option<&str>) -> OverlayMarketFacts {
    let plat: HashMap<String, u32> = platinum.iter().map(|(s, p)| (s.to_string(), *p)).collect();
    let duc: HashMap<String, u32> = ducats.iter().map(|(s, d)| (s.to_string(), *d)).collect();
    move |slug| OverlayMarketFacts {
        cached_platinum: slug.and_then(|s| plat.get(s).copied()),
        ducats: slug.and_then(|s| duc.get(s).copied()),
    }
}

#[test]
fn corpus_four_reward_screen_recognizes_all_slots_and_marks_only_the_best() {
    let names = [
        "Paris Prime Blueprint",
        "Wisp Prime Systems Blueprint",
        "Cedo Prime Barrel",
        "Forma Blueprint",
    ];
    let frame = corpus_frame();
    let screen = corpus_screen(&names);
    let read = layout_from_lines(&frame, &screen, &corpus_catalog(), 4, None, |slot| {
        names[slot.index].to_string()
    })
    .expect("a four-reward screen must be detected");
    assert_eq!(read.layout.slots.len(), 4);
    assert_eq!(read.matches.len(), 4);
    assert!(read
        .matches
        .iter()
        .all(|(_, found)| found.confidence == 1.0));

    let (result, _) = assemble_result(
        &frame,
        read,
        &OverlaySettings::default(),
        4,
        corpus_facts(
            &[
                ("paris_prime_blueprint", 15),
                ("wisp_prime_systems_blueprint", 60),
                ("cedo_prime_barrel", 30),
            ],
            &[
                ("paris_prime_blueprint", 45),
                ("wisp_prime_systems_blueprint", 100),
                ("cedo_prime_barrel", 100),
            ],
        ),
        None,
        "corpus-1".into(),
    );
    assert_eq!(result.slots.len(), 4);
    assert!(result.slots.iter().all(|slot| slot.name.is_some()));

    let best_plat: Vec<&str> = result
        .slots
        .iter()
        .filter(|slot| slot.best_platinum)
        .map(|slot| slot.name.as_deref().unwrap())
        .collect();
    assert_eq!(best_plat, vec!["Wisp Prime Systems Blueprint"]);

    // Ducat tie (100/100) is the policy, not a miss: both are marked.
    let best_duc: Vec<&str> = result
        .slots
        .iter()
        .filter(|slot| slot.best_ducats)
        .map(|slot| slot.name.as_deref().unwrap())
        .collect();
    assert_eq!(best_duc.len(), 2);
    assert!(best_duc.contains(&"Wisp Prime Systems Blueprint"));
    assert!(best_duc.contains(&"Cedo Prime Barrel"));

    // Untradeable: recognized with no facts, never recommended.
    let forma = result
        .slots
        .iter()
        .find(|slot| slot.name.as_deref() == Some("Forma Blueprint"))
        .unwrap();
    assert!(forma.slug.is_none());
    assert!(forma.cached_platinum.is_none());
    assert!(!forma.best_platinum && !forma.best_ducats);
}

#[test]
fn corpus_garbled_expensive_reward_is_shown_but_never_recommended() {
    // OCR reads "Wlsp Prime Systern Bluepint" -> fuzzy match ~0.875, below
    // RECOMMENDATION_CONFIDENCE. It still resolves to the 60p item, but
    // the best-pick mark must stay off even though it is the dearest.
    let names = [
        "Paris Prime Blueprint",
        "Wlsp Prime Systern Bluepint",
        "Cedo Prime Barrel",
        "Forma Blueprint",
    ];
    let frame = corpus_frame();
    let screen = corpus_screen(&names);
    let read = layout_from_lines(&frame, &screen, &corpus_catalog(), 4, None, |slot| {
        names[slot.index].to_string()
    })
    .expect("a four-reward screen must be detected");
    let (result, _) = assemble_result(
        &frame,
        read,
        &OverlaySettings::default(),
        4,
        corpus_facts(
            &[
                ("paris_prime_blueprint", 15),
                ("wisp_prime_systems_blueprint", 60),
                ("cedo_prime_barrel", 30),
            ],
            &[],
        ),
        None,
        "corpus-2".into(),
    );
    let wisp = result
        .slots
        .iter()
        .find(|slot| slot.name.as_deref() == Some("Wisp Prime Systems Blueprint"))
        .expect("the garbled reward must still resolve by name");
    assert!(wisp.confidence < RECOMMENDATION_CONFIDENCE);
    assert_eq!(wisp.cached_platinum, Some(60));
    assert!(
        !wisp.best_platinum,
        "a low-confidence match must never be recommended"
    );
    let best_plat: Vec<&str> = result
        .slots
        .iter()
        .filter(|slot| slot.best_platinum)
        .map(|slot| slot.name.as_deref().unwrap())
        .collect();
    // The dearest is the garbled 60p wisp; ruled out, the mark falls to
    // the next recommendable slot.
    assert_eq!(
        best_plat,
        vec!["Cedo Prime Barrel"],
        "wrong best mark: {best_plat:?}"
    );
}

#[test]
fn corpus_no_reward_screen_is_not_detected() {
    let frame = corpus_frame();
    let screen = vec![
        TsvLine {
            x: 300,
            y: 300,
            width: 400,
            height: 24,
            text: "Mission reward objective text".into(),
        },
        TsvLine {
            x: 500,
            y: 700,
            width: 300,
            height: 20,
            text: "Extraction complete".into(),
        },
    ];
    let read = layout_from_lines(&frame, &screen, &corpus_catalog(), 0, None, |_| {
        String::new()
    });
    assert!(
        read.is_none(),
        "no catalog match must mean no reward screen"
    );
}

#[test]
fn corpus_two_reward_screen_detects_both() {
    let names = ["Paris Prime Blueprint", "Wisp Prime Systems Blueprint"];
    let frame = corpus_frame();
    let screen = corpus_screen(&names);
    let read = layout_from_lines(&frame, &screen, &corpus_catalog(), 2, None, |slot| {
        names[slot.index].to_string()
    })
    .expect("a two-reward screen must be detected");
    assert_eq!(read.layout.slots.len(), 2);
    assert_eq!(read.matches.len(), 2);
}

#[test]
#[ignore = "requires the pinned release OCR model"]
fn real_three_reward_capture_survives_common_display_shapes() {
    let band = image::load_from_memory(include_bytes!(
        "../../../../tests/fixtures/relic-ocr/three-reward-band.png"
    ))
    .unwrap()
    .to_rgba8();
    let mut source = RgbaImage::new(2560, 1440);
    image::imageops::overlay(&mut source, &band, 760, 480);
    let scaled_1080 = image::imageops::resize(&source, 1920, 1080, FilterType::Triangle);
    let scaled_720 = image::imageops::resize(&source, 1280, 720, FilterType::Triangle);
    let mut ultrawide = RgbaImage::new(3440, 1440);
    image::imageops::overlay(&mut ultrawide, &source, 440, 0);
    let mut sixteen_ten = RgbaImage::new(1920, 1200);
    image::imageops::overlay(&mut sixteen_ten, &scaled_1080, 0, 60);
    let variants = [
        ("native-1440p", source),
        ("scaled-1080p", scaled_1080),
        ("scaled-720p", scaled_720),
        ("ultrawide-1440p", ultrawide),
        ("sixteen-ten-1200p", sixteen_ten),
    ];
    let candidates = vec![
        OverlayCatalogItem {
            name: "Paris Prime String".into(),
            slug: Some("paris_prime_string".into()),
        },
        OverlayCatalogItem {
            name: "Perigale Prime Blueprint".into(),
            slug: Some("perigale_prime_blueprint".into()),
        },
        OverlayCatalogItem {
            name: "Forma Blueprint".into(),
            slug: None,
        },
    ];
    let tessdata = std::env::var_os("TENNOWORTH_TESSDATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/tessdata"));
    assert!(
        tessdata.join("eng.traineddata").is_file(),
        "pinned eng.traineddata is missing from {}",
        tessdata.display()
    );
    let (ocr, ready) = OcrWorker::start(Ok(tessdata));
    ready.unwrap();
    for (label, image) in variants {
        let frame = CapturedFrame {
            width: image.width(),
            height: image.height(),
            image,
            x: 0,
            y: 0,
        };
        let mut timings = OverlayStageTimings::default();
        let started = std::time::Instant::now();
        let expected = read_expected_layout(&ocr, &frame, &candidates, 3, None, &mut timings)
            .unwrap_or_else(|error| panic!("{label} expected path after {:?}: {error}; timings={timings:?}", started.elapsed()));
        assert!(
            layout_read_is_complete(&expected, 3),
            "{label} expected path"
        );

        let centered = read_centered_reward_layout(&ocr, &frame, &candidates, None, &mut timings)
            .unwrap_or_else(|error| panic!("{label} centered path: {error}"));
        let mut consensus = RecognitionConsensus::default();
        consensus.observe(centered);
        assert!(
            consensus.resolve(3, true).is_some(),
            "{label} centered path"
        );
        eprintln!("{label}: elapsed={:?}; timings={timings:?}", started.elapsed());
    }
}
