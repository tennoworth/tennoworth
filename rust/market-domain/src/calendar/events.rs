//! Every dated thing the month and lane calendar shows. Confirmed dates come
//! from DE's worldState (through the snapshot) and from the curated feed.
//! Estimates (Baro's later visits, Prime vaulting) are projections and say so:
//! they carry `estimate` and never advice, because presenting a guess as a
//! date is how a calendar loses trust.
//!
//! Rows come in source order; the page orders them, since its tie-break
//! compares ids with `localeCompare`.

use super::date::{js_trim, parse_ms};
use super::{
    at, entries, field, filled, ms, positive, printed, reward_rows, rows, set_parts, text,
    truthy_number, CalendarHolding, BARO_PERIOD, BARO_PROJECTIONS, DAY, PAST, VAULT_HORIZON,
};
use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum DatedKind {
    Official,
    Event,
    Baro,
    Resurgence,
    Vault,
    Notice,
    Deal,
    Nightwave,
}

/// Who published the date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum DatedSource {
    Worldstate,
    Estimate,
    Notice,
    Official,
}

/// What a game-wide booster raises; DE's codes the calendar does not know are `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum Boost {
    ResourceAmount,
    ResourceChance,
    Affinity,
    Credits,
    Other,
}

fn boost(upgrade: &str) -> Boost {
    match upgrade {
        "GAMEPLAY_PICKUP_AMOUNT" => Boost::ResourceAmount,
        "GAMEPLAY_PICKUP_RATE" => Boost::ResourceChance,
        "GAMEPLAY_KILL_XP_AMOUNT" => Boost::Affinity,
        "GAMEPLAY_MONEY_PICKUP_AMOUNT" | "GAMEPLAY_MONEY_REWARD_AMOUNT" => Boost::Credits,
        _ => Boost::Other,
    }
}

/// The facts behind an event's title and description. Rows DE or the feed
/// name themselves carry that text as published (`Given`); the view words
/// the rest. Resurgence and vault rows carry set slugs, which the view names.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct AlertReward {
    pub item: String,
    pub count: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum DatedText {
    Baro {
        r#where: Option<String>,
    },
    BaroEstimate,
    Resurgence {
        sets: Vec<String>,
    },
    Vault {
        sets: Vec<String>,
    },
    Deal {
        item: String,
        discount: Option<f64>,
        price: Option<f64>,
    },
    Event {
        title: String,
        rewards: Vec<String>,
    },
    Alert {
        rewards: Vec<AlertReward>,
        credits: Option<f64>,
    },
    /// `factor` is the multiplier, or none when DE's operation is not a plain multiply.
    Booster {
        boost: Boost,
        factor: Option<f64>,
    },
    Nightwave {
        intermission: bool,
    },
    Official {
        title: String,
        note: Option<String>,
        r#type: String,
    },
    Given {
        title: String,
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct DatedEvent {
    /// Stable across refreshes of the same snapshot, so a selection survives.
    pub id: String,
    pub kind: DatedKind,
    /// Epoch ms.
    pub from: i64,
    /// Epoch ms; none for a single moment such as a deadline.
    pub to: Option<i64>,
    pub text: DatedText,
    pub estimate: bool,
    pub source: DatedSource,
    #[serde(rename = "sourceUrl", skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source_url: Option<String>,
    /// Held slugs this event touches; absent when that cannot be known.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub affects: Option<Vec<String>>,
}

impl DatedEvent {
    fn confirmed(id: String, kind: DatedKind, from: i64, to: Option<i64>, text: DatedText) -> Self {
        DatedEvent {
            id,
            kind,
            from,
            to,
            text,
            estimate: false,
            source: DatedSource::Worldstate,
            source_url: None,
            affects: None,
        }
    }
}

/// Held slugs with a copy left; a short list, so a scan is cheaper in the module than a set.
type Held<'a> = Option<Vec<&'a str>>;

/// A window still worth showing: both ends readable and not long finished.
fn window(value: &Value, from: &str, to: &str, now: f64) -> Option<(i64, i64)> {
    let (from, to) = (at(value, from)?, at(value, to)?);
    (ms(to) >= now - PAST).then_some((from, to))
}

/// The held sets and parts among `sets`, each once.
fn held_parts(market: &Value, sets: &[&str], held: &Held) -> Option<Vec<String>> {
    let held = held.as_ref()?;
    let mut hits: Vec<String> = Vec::new();
    let mut hit = |slug: &str| {
        if held.contains(&slug) && !hits.iter().any(|h| h == slug) {
            hits.push(slug.to_owned());
        }
    };
    for set in sets {
        hit(set);
        for part in set_parts(market, set) {
            if let Some(slug) = text(part, "slug") {
                hit(slug);
            }
        }
    }
    Some(hits)
}

fn baro(market: &Value, now: f64, out: &mut Vec<DatedEvent>) {
    let Some(visit) = field(market, "baro") else {
        return;
    };
    let Some((from, to)) = window(visit, "activation", "expiry", now) else {
        return;
    };
    out.push(DatedEvent::confirmed(
        format!("baro-{from}"),
        DatedKind::Baro,
        from,
        Some(to),
        DatedText::Baro {
            r#where: filled(visit, "location").map(str::to_owned),
        },
    ));
    for n in 1..=BARO_PROJECTIONS {
        let start = from + n * BARO_PERIOD;
        if ms(start + (to - from)) < now {
            continue;
        }
        out.push(DatedEvent {
            id: format!("baro-est-{start}"),
            kind: DatedKind::Baro,
            from: start,
            to: Some(start + (to - from)),
            text: DatedText::BaroEstimate,
            estimate: true,
            source: DatedSource::Estimate,
            source_url: None,
            affects: None,
        });
    }
}

fn resurgence(market: &Value, now: f64, held: &Held, out: &mut Vec<DatedEvent>) {
    let calendar = field(market, "calendar");
    let current = calendar.and_then(|c| field(c, "resurgence_current"));
    let mut seen = Vec::new();
    for rotation in rows(calendar, "resurgence").iter().chain(current) {
        let frames: Vec<&str> = rows(Some(rotation), "frames")
            .iter()
            .map(|f| f.as_str().unwrap_or_default())
            .collect();
        let Some((from, to)) = window(rotation, "from", "to", now) else {
            continue;
        };
        if frames.is_empty() || seen.contains(&from) {
            continue;
        }
        seen.push(from);
        let mut event = DatedEvent::confirmed(
            format!("resurgence-{from}"),
            DatedKind::Resurgence,
            from,
            Some(to),
            DatedText::Resurgence {
                sets: frames.iter().map(|f| (*f).to_owned()).collect(),
            },
        );
        event.affects = held_parts(market, &frames, held);
        out.push(event);
    }
}

fn vaulting(market: &Value, now: f64, held: &Held, out: &mut Vec<DatedEvent>) {
    let mut by_date: Vec<(i64, Vec<&str>)> = Vec::new();
    for (slug, prime) in entries(field(market, "calendar"), "primes") {
        if prime.is_null()
            || prime.get("vaulted") == Some(&Value::Bool(true))
            || filled(prime, "vault_date").is_some()
        {
            continue;
        }
        let Some(when) = at(prime, "est_vault_date") else {
            continue;
        };
        if ms(when) <= now || ms(when) > now + VAULT_HORIZON {
            continue;
        }
        match by_date.iter_mut().find(|(date, _)| *date == when) {
            Some((_, sets)) => sets.push(slug),
            None => by_date.push((when, vec![slug])),
        }
    }
    out.extend(by_date.into_iter().map(|(when, sets)| DatedEvent {
        id: format!("vault-est-{when}"),
        kind: DatedKind::Vault,
        from: when,
        to: None,
        affects: held_parts(market, &sets, held),
        text: DatedText::Vault {
            sets: sets.into_iter().map(str::to_owned).collect(),
        },
        estimate: true,
        source: DatedSource::Estimate,
        source_url: None,
    }));
}

fn deals(market: &Value, now: f64, out: &mut Vec<DatedEvent>) {
    for deal in rows(field(market, "de"), "deals") {
        let Some(to) = at(deal, "expiry").filter(|to| ms(*to) >= now) else {
            continue;
        };
        let (discount, price) = (
            truthy_number(deal, "discount"),
            truthy_number(deal, "sale_price"),
        );
        let priced = discount.is_some() && price.is_some();
        out.push(DatedEvent::confirmed(
            format!("deal-{to}-{}", printed(deal, "item")),
            DatedKind::Deal,
            to - DAY,
            Some(to),
            DatedText::Deal {
                item: text(deal, "item").unwrap_or_default().to_owned(),
                discount: discount.filter(|_| priced),
                price: price.filter(|_| priced),
            },
        ));
    }
}

fn game_events(market: &Value, now: f64, out: &mut Vec<DatedEvent>) {
    for entry in reward_rows(market) {
        // DE publishes some goals with no name; an unnamed row tells the reader nothing.
        let Some(title) = text(entry, "title").filter(|t| !js_trim(t).is_empty()) else {
            continue;
        };
        let Some((from, to)) = window(entry, "starts_at", "ends_at", now) else {
            continue;
        };
        let mut rewards: Vec<String> = Vec::new();
        let groups = rows(Some(entry), "groups");
        for name in groups
            .iter()
            .flat_map(|g| rows(Some(g), "rewards"))
            .filter_map(|r| text(r, "name"))
        {
            if !rewards.iter().any(|r| r == name) {
                rewards.push(name.to_owned());
            }
        }
        out.push(DatedEvent::confirmed(
            format!("event-{}", printed(entry, "id")),
            DatedKind::Event,
            from,
            Some(to),
            DatedText::Event {
                title: title.to_owned(),
                rewards,
            },
        ));
    }
}

// Alerts and boosters are short in-game happenings, filed with the in-game
// events rather than given a filter each.
fn alerts(market: &Value, now: f64, out: &mut Vec<DatedEvent>) {
    for alert in rows(field(market, "de"), "alerts") {
        let credits = positive(alert, "credits");
        let rewards: Vec<AlertReward> = rows(Some(alert), "rewards")
            .iter()
            .map(|r| AlertReward {
                item: text(r, "item").unwrap_or_default().to_owned(),
                count: r.get("count").and_then(Value::as_f64).unwrap_or_default(),
            })
            .collect();
        let Some((from, to)) = window(alert, "activation", "expiry", now) else {
            continue;
        };
        if rewards.is_empty() && credits.is_none() {
            continue;
        }
        out.push(DatedEvent::confirmed(
            format!("alert-{}", printed(alert, "id")),
            DatedKind::Event,
            from,
            Some(to),
            DatedText::Alert { rewards, credits },
        ));
    }
}

fn boosters(market: &Value, now: f64, out: &mut Vec<DatedEvent>) {
    for booster in rows(field(market, "de"), "boosters") {
        let Some((from, to)) = window(booster, "activation", "expiry", now) else {
            continue;
        };
        let multiplies =
            field(booster, "operation").map_or(Some("MULTIPLY"), Value::as_str) == Some("MULTIPLY");
        let upgrade = text(booster, "upgrade").unwrap_or_default();
        out.push(DatedEvent::confirmed(
            format!("booster-{from}-{}", printed(booster, "upgrade")),
            DatedKind::Event,
            from,
            Some(to),
            DatedText::Booster {
                boost: boost(upgrade),
                factor: booster
                    .get("value")
                    .and_then(Value::as_f64)
                    .filter(|v| multiplies && *v > 1.0),
            },
        ));
    }
}

fn nightwave(market: &Value, now: f64, out: &mut Vec<DatedEvent>) {
    for season in rows(field(market, "de"), "nightwave") {
        let Some((from, to)) = window(season, "activation", "expiry", now) else {
            continue;
        };
        out.push(DatedEvent::confirmed(
            format!("nightwave-{from}"),
            DatedKind::Nightwave,
            from,
            Some(to),
            DatedText::Nightwave {
                intermission: season.get("intermission") == Some(&Value::Bool(true)),
            },
        ));
    }
}

fn notices(feed: &Value, now: f64, out: &mut Vec<DatedEvent>) {
    for message in rows(Some(feed), "messages") {
        let Some((from, to)) = window(message, "starts_at", "ends_at", now) else {
            continue;
        };
        out.push(DatedEvent {
            id: format!("notice-{}", printed(message, "id")),
            kind: DatedKind::Notice,
            from,
            to: Some(to),
            text: DatedText::Given {
                title: text(message, "title").unwrap_or_default().to_owned(),
                detail: text(message, "body").unwrap_or_default().to_owned(),
            },
            estimate: false,
            source: DatedSource::Notice,
            source_url: text(message, "source_url").map(str::to_owned),
            affects: None,
        });
    }
}

fn official(feed: &Value, now: f64, out: &mut Vec<DatedEvent>) {
    for event in rows(Some(feed), "events") {
        let Some(from) = at(event, "starts_at") else {
            continue;
        };
        // A date with no end is a single moment, such as a stream start.
        let to = match filled(event, "ends_at") {
            Some(end) => match parse_ms(end) {
                Some(to) => Some(to),
                None => continue,
            },
            None => None,
        };
        if ms(to.unwrap_or(from)) < now - PAST {
            continue;
        }
        out.push(DatedEvent {
            id: format!("official-{}", printed(event, "id")),
            kind: DatedKind::Official,
            from,
            to,
            text: DatedText::Official {
                title: text(event, "title").unwrap_or_default().to_owned(),
                note: text(event, "note")
                    .map(js_trim)
                    .filter(|n| !n.is_empty())
                    .map(str::to_owned),
                r#type: text(event, "kind").unwrap_or_default().to_owned(),
            },
            estimate: false,
            source: DatedSource::Official,
            source_url: text(event, "source_url").map(str::to_owned),
            affects: None,
        });
    }
}

/// The calendar's rows from the snapshot and the curated feed, in source order.
pub fn build(
    market: &Value,
    feed: &Value,
    owned: Option<&[CalendarHolding]>,
    now: f64,
) -> Vec<DatedEvent> {
    let held: Held = owned.map(|owned| {
        owned
            .iter()
            .filter(|h| h.count.is_some_and(|c| c > 0.0))
            .map(|h| h.slug.as_str())
            .collect()
    });
    let mut out = Vec::new();
    if !market.is_null() {
        game_events(market, now, &mut out);
        alerts(market, now, &mut out);
        boosters(market, now, &mut out);
        nightwave(market, now, &mut out);
        baro(market, now, &mut out);
        resurgence(market, now, &held, &mut out);
        vaulting(market, now, &held, &mut out);
        deals(market, now, &mut out);
    }
    notices(feed, now, &mut out);
    official(feed, now, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::super::CalendarRequest;
    use super::*;
    use serde_json::{json, Value};

    const NOW: f64 = 1_791_882_000_000.0; // 2026-10-13T09:00:00Z

    fn events(
        market: Option<Value>,
        feed: Option<Value>,
        owned: Option<&[&str]>,
    ) -> Vec<DatedEvent> {
        let owned: Option<Vec<_>> =
            owned.map(|o| o.iter().map(|s| json!({ "slug": s, "count": 1 })).collect());
        let request: CalendarRequest = serde_json::from_value(
            json!({ "market": market, "feed": feed, "owned": owned, "now": NOW }),
        )
        .unwrap();
        build(
            &request.market,
            &request.feed,
            request.owned.as_deref(),
            request.now,
        )
    }

    fn rotation() -> Value {
        json!({ "from": "2026-10-01T18:00:00.000Z", "to": "2026-10-29T18:00:00.000Z", "frames": ["protea_prime_set", "ivara_prime_set"] })
    }

    #[test]
    fn projects_baro_as_estimates_beside_the_confirmed_visit() {
        let rows = events(
            Some(
                json!({ "baro": { "activation": "2026-10-16T13:00:00Z", "expiry": "2026-10-18T13:00:00Z", "location": "Pluto Relay" } }),
            ),
            None,
            None,
        );
        let summary: Vec<_> = rows.iter().map(|e| (&e.text, e.estimate, e.from)).collect();
        assert_eq!(
            summary,
            [
                (
                    &DatedText::Baro {
                        r#where: Some("Pluto Relay".into())
                    },
                    false,
                    1_792_155_600_000
                ),
                (&DatedText::BaroEstimate, true, 1_793_365_200_000),
                (&DatedText::BaroEstimate, true, 1_794_574_800_000),
            ]
        );
    }

    #[test]
    fn lists_a_rotation_once_and_knows_the_held_parts_it_touches() {
        let rows = events(
            Some(
                json!({ "calendar": { "resurgence": [rotation()], "resurgence_current": rotation() } }),
            ),
            None,
            None,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].text,
            DatedText::Resurgence {
                sets: vec!["protea_prime_set".into(), "ivara_prime_set".into()]
            }
        );
        let data = json!({
            "calendar": { "resurgence_current": rotation() },
            "set_to_parts": { "ivara_prime_set": { "name": "Ivara Prime Set", "parts": [{ "slug": "ivara_prime_chassis_blueprint", "component_name": "Chassis" }] } },
        });
        let held = events(
            Some(data.clone()),
            None,
            Some(&["ivara_prime_chassis_blueprint", "lavos_prime_set"]),
        );
        assert_eq!(
            held[0].affects,
            Some(vec!["ivara_prime_chassis_blueprint".into()])
        );
        assert_eq!(events(Some(data), None, None)[0].affects, None);
    }

    #[test]
    fn groups_estimated_vaulting_by_date() {
        let rows = events(
            Some(json!({ "calendar": { "primes": {
            "lavos_prime_set": { "name": "Lavos Prime", "vaulted": false, "est_vault_date": "2026-11-12" },
            "cedo_prime_set": { "name": "Cedo Prime", "vaulted": false, "est_vault_date": "2026-11-12" },
            "braton_prime_set": { "name": "Braton Prime", "vaulted": false, "est_vault_date": "2015-04-13" },
            "yareli_prime_set": { "name": "Yareli Prime", "vaulted": false, "est_vault_date": "2027-02-21" },
            "acceltra_prime_set": { "name": "Acceltra Prime", "vaulted": true, "est_vault_date": "2026-11-01", "vault_date": "2026-11-01" },
        } } })),
            None,
            None,
        );
        let summary: Vec<_> = rows.iter().map(|e| (&e.text, e.estimate, e.to)).collect();
        assert_eq!(
            summary,
            [(
                &DatedText::Vault {
                    sets: vec!["lavos_prime_set".into(), "cedo_prime_set".into()]
                },
                true,
                None
            )]
        );
    }

    #[test]
    fn keeps_curated_notices_and_official_dates_with_their_source() {
        let feed = json!({ "version": 1, "updated_at": "2026-10-11T19:00:00Z", "messages": [
            { "id": "bronco", "title": "Bronco Prime Barrel", "body": "Exchange spare barrels.", "source_url": "https://forums.warframe.com/", "starts_at": "2026-10-11T00:00:00Z", "ends_at": "2026-10-15T15:00:00Z" },
            { "id": "old", "title": "Old", "body": "Old.", "source_url": "https://forums.warframe.com/", "starts_at": "2026-07-01T00:00:00Z", "ends_at": "2026-07-02T00:00:00Z" },
        ], "events": [
            { "id": "devstream-200", "kind": "devstream", "title": "Devstream 200", "starts_at": "2026-10-23T19:00:00Z", "source_url": "https://forums.warframe.com/topic/devstream-200/" },
            { "id": "update-41", "kind": "update", "title": "Update 41", "starts_at": "2026-11-18T15:00:00Z", "ends_at": "2026-11-18T18:00:00Z", "source_url": "https://www.warframe.com/news/update-41", "note": " Patch notes follow at release.\u{feff}" },
            { "id": "old", "kind": "other", "title": "Old", "starts_at": "2026-07-01T00:00:00Z", "source_url": "https://www.warframe.com/news" },
        ] });
        let rows = events(None, Some(feed), None);
        let summary: Vec<_> = rows
            .iter()
            .map(|e| {
                (
                    e.id.as_str(),
                    e.kind,
                    e.to.is_none(),
                    e.source_url.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                (
                    "notice-bronco",
                    DatedKind::Notice,
                    false,
                    Some("https://forums.warframe.com/")
                ),
                (
                    "official-devstream-200",
                    DatedKind::Official,
                    true,
                    Some("https://forums.warframe.com/topic/devstream-200/")
                ),
                (
                    "official-update-41",
                    DatedKind::Official,
                    false,
                    Some("https://www.warframe.com/news/update-41")
                ),
            ]
        );
        assert_eq!(
            rows[1].text,
            DatedText::Official {
                title: "Devstream 200".into(),
                note: None,
                r#type: "devstream".into()
            }
        );
        assert_eq!(
            rows[2].text,
            DatedText::Official {
                title: "Update 41".into(),
                note: Some("Patch notes follow at release.".into()),
                r#type: "update".into()
            }
        );
    }

    #[test]
    fn survives_missing_and_malformed_surfaces() {
        assert!(build(&Value::Null, &Value::Null, None, NOW).is_empty());
        let mut empty = rotation();
        empty["frames"] = json!([]);
        let rows = events(
            Some(json!({
                "baro": { "activation": "soon", "expiry": "", "location": "" },
                "calendar": { "resurgence_current": empty },
                "de": { "deals": [{ "item": "X", "expiry": "never" }] },
            })),
            None,
            None,
        );
        assert!(rows.is_empty());
    }

    #[test]
    fn lists_named_events_with_their_rewards_once() {
        let goal = |id: &str, title: &str, rewards: &[&str]| {
            json!({
                "id": id, "source": "goal", "title": title, "starts_at": "2026-10-07T16:00:00Z", "ends_at": "2026-10-21T16:00:00Z", "completeness": "complete",
                "groups": [{ "kind": "final", "rewards": rewards.iter().map(|n| json!({ "unique": format!("/Lotus/{n}"), "name": n, "quantity": 1 })).collect::<Vec<_>>() }],
            })
        };
        let rows = events(
            Some(json!({ "event_rewards": { "goals": {
            "a": goal("a", "Heat Fissure", &["Kuva", "Ayatan Sculpture", "Kuva", "Forma"]),
            "b": goal("b", " \u{3000}", &["Kuva"]),
        } } })),
            None,
            None,
        );
        assert_eq!(
            rows.iter().map(|e| &e.text).collect::<Vec<_>>(),
            [&DatedText::Event {
                title: "Heat Fissure".into(),
                rewards: vec!["Kuva".into(), "Ayatan Sculpture".into(), "Forma".into()],
            }]
        );
    }

    #[test]
    fn files_alerts_and_boosters_with_events_and_nightwave_apart() {
        let rows = events(
            Some(json!({ "de": {
            "alerts": [
                { "id": "a1", "activation": "2026-10-12T19:00:00Z", "expiry": "2026-10-15T19:00:00Z", "credits": 10000, "rewards": [{ "item": "Kuva", "count": 200000 }] },
                { "id": "a2", "activation": "2026-10-12T19:00:00Z", "expiry": "2026-10-15T19:00:00Z", "rewards": [] },
            ],
            "boosters": [
                { "activation": "2026-10-12T19:00:00Z", "expiry": "2026-10-14T19:00:00Z", "upgrade": "GAMEPLAY_PICKUP_AMOUNT", "operation": "MULTIPLY", "value": 2 },
                { "activation": "2026-10-12T19:00:00Z", "expiry": "2026-10-14T19:00:00Z", "upgrade": "GAMEPLAY_SOMETHING_NEW", "operation": "ADD", "value": 5 },
            ],
            "nightwave": [{ "activation": "2026-08-12T15:30:00Z", "expiry": "2027-03-08T00:00:00Z", "intermission": true }],
        } })),
            None,
            None,
        );
        let summary: Vec<_> = rows
            .iter()
            .map(|e| (e.id.as_str(), e.kind, &e.text))
            .collect();
        assert_eq!(
            summary,
            [
                (
                    "alert-a1",
                    DatedKind::Event,
                    &DatedText::Alert {
                        rewards: vec![AlertReward {
                            item: "Kuva".into(),
                            count: 200_000.0
                        }],
                        credits: Some(10_000.0)
                    }
                ),
                (
                    "booster-1791831600000-GAMEPLAY_PICKUP_AMOUNT",
                    DatedKind::Event,
                    &DatedText::Booster {
                        boost: Boost::ResourceAmount,
                        factor: Some(2.0)
                    }
                ),
                (
                    "booster-1791831600000-GAMEPLAY_SOMETHING_NEW",
                    DatedKind::Event,
                    &DatedText::Booster {
                        boost: Boost::Other,
                        factor: None
                    }
                ),
                (
                    "nightwave-1786548600000",
                    DatedKind::Nightwave,
                    &DatedText::Nightwave { intermission: true }
                ),
            ]
        );
    }

    #[test]
    fn turns_a_live_darvo_deal_into_a_one_day_event() {
        let rows = events(
            Some(
                json!({ "de": { "deals": [{ "item": "Tenno Sword Shield", "expiry": "2026-10-13T15:00:00Z", "discount": 90, "sale_price": 15 }] } }),
            ),
            None,
            None,
        );
        assert_eq!(
            rows[0].text,
            DatedText::Deal {
                item: "Tenno Sword Shield".into(),
                discount: Some(90.0),
                price: Some(15.0)
            }
        );
        assert_eq!(rows[0].to.map(|to| to - rows[0].from), Some(86_400_000));
        assert_eq!(rows[0].id, "deal-1791903600000-Tenno Sword Shield");
    }

    #[test]
    fn held_parts_count_only_copies_still_held() {
        let request: CalendarRequest = serde_json::from_value(json!({
            "market": { "calendar": { "resurgence_current": rotation() } },
            "feed": null,
            "owned": [{ "slug": "protea_prime_set", "count": 0 }, { "slug": "ivara_prime_set", "count": 2 }],
            "now": NOW,
        })).unwrap();
        let rows = build(&request.market, &Value::Null, request.owned.as_deref(), NOW);
        assert_eq!(rows[0].affects, Some(vec!["ivara_prime_set".into()]));
    }
}
