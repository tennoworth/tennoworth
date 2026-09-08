use super::*;
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Refinement {
    Intact,
    Exceptional,
    Flawless,
    Radiant,
}
impl Refinement {
    fn key(self) -> &'static str {
        match self {
            Self::Intact => "intact",
            Self::Exceptional => "exceptional",
            Self::Flawless => "flawless",
            Self::Radiant => "radiant",
        }
    }
    fn traces(self) -> f64 {
        match self {
            Self::Intact => 0.0,
            Self::Exceptional => 25.0,
            Self::Flawless => 50.0,
            Self::Radiant => 100.0,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RelicEv {
    pub refinement: Refinement,
    pub ev: f64,
    pub traces: f64,
    pub gain_over_intact: f64,
    pub plat_per_trace: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum RelicVerdict {
    Crack,
    Refine,
    SellIntact,
    Thin,
    Unknown,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RelicDecision {
    pub ladder: Vec<RelicEv>,
    pub best: Option<RelicEv>,
    pub sell_now: f64,
    pub moving_count: usize,
    pub total_rewards: usize,
    pub verdict: RelicVerdict,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct RelicPlanReward {
    pub slug: String,
    pub name: String,
    pub rarity: String,
    pub chance: f64,
    pub low_sell: f64,
    pub vol_48h: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct RelicPlanEntry {
    pub relic_slug: String,
    pub relic_name: String,
    pub owned: f64,
    pub epp: f64,
    pub epp_owned: f64,
    pub sell_now: f64,
    pub moving_count: usize,
    pub total_rewards: usize,
    pub rewards: Vec<RelicPlanReward>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub decision: Option<RelicDecision>,
}
fn chance_at(reward: &Value, refinement: Refinement) -> Option<f64> {
    reward
        .get("chances")
        .and_then(|chances| chances.get(refinement.key()))
        .unwrap_or(&Value::Null)
        .as_f64()
        .filter(|c| c.is_finite())
        .or_else(|| {
            if refinement == Refinement::Intact {
                reward["chance"].as_f64().filter(|c| c.is_finite())
            } else {
                None
            }
        })
}
fn ev_at(rewards: &[Value], market: &Value, refinement: Refinement) -> Option<f64> {
    if rewards.is_empty() {
        return None;
    }
    let mut ev = 0.0;
    for r in rewards {
        let chance = chance_at(r, refinement)?;
        let count = if r["item_count"].is_null() {
            1.0
        } else {
            num(&r["item_count"])
        };
        ev += chance / 100.0 * price(market, &name(r, "reward_slug")).unwrap_or(0.0) * count;
    }
    (ev >= 0.0 && ev.is_finite()).then_some(ev)
}
pub fn decide_relic(rewards: &[Value], slug: &str, market: &Value) -> RelicDecision {
    let intact = ev_at(rewards, market, Refinement::Intact);
    let mut ladder = Vec::new();
    for refinement in [
        Refinement::Intact,
        Refinement::Exceptional,
        Refinement::Flawless,
        Refinement::Radiant,
    ] {
        if let Some(ev) = ev_at(rewards, market, refinement) {
            let traces = refinement.traces();
            let gain = intact.map_or(0.0, |n| ev - n);
            ladder.push(RelicEv {
                refinement,
                ev,
                traces,
                gain_over_intact: gain,
                plat_per_trace: if traces > 0.0 {
                    Some(gain / traces)
                } else {
                    None
                },
            });
        }
    }
    let mut best = ladder
        .iter()
        .find(|l| l.refinement == Refinement::Intact)
        .cloned();
    let mut refined: Option<RelicEv> = None;
    for rung in &ladder {
        if rung.plat_per_trace.is_some_and(|p| p >= 0.15)
            && refined.as_ref().is_none_or(|r| rung.ev > r.ev)
        {
            refined = Some(rung.clone());
        }
    }
    if refined.is_some() {
        best = refined;
    }
    let moving_count = rewards
        .iter()
        .filter(|r| field(entry(market, &name(r, "reward_slug")), "vol") >= 5.0)
        .count();
    let sell_now = price(market, slug).unwrap_or(0.0);
    let verdict = match &best {
        None => RelicVerdict::Unknown,
        Some(_) if moving_count == 0 => RelicVerdict::Thin,
        Some(b) if sell_now > 0.0 && sell_now >= b.ev => RelicVerdict::SellIntact,
        Some(b) if b.refinement != Refinement::Intact => RelicVerdict::Refine,
        Some(_) => RelicVerdict::Crack,
    };
    RelicDecision {
        ladder,
        best,
        sell_now,
        moving_count,
        total_rewards: rewards.len(),
        verdict,
    }
}
pub fn relic_plan(r: &PlannerRequest) -> Vec<RelicPlanEntry> {
    let mut out = Vec::new();
    for rec in &r.owned {
        if rec.subtype.as_deref() != Some("intact") {
            continue;
        }
        let Some(table) = r
            .market
            .get("relic_rewards")
            .and_then(|rewards| rewards.get(&rec.slug))
            .unwrap_or(&Value::Null)
            .as_array()
            .filter(|r| !r.is_empty())
        else {
            continue;
        };
        let mut epp = 0.0;
        let mut rewards = Vec::new();
        let mut moving_count = 0;
        for reward in table {
            let Some(chance) = reward["chance"].as_f64() else {
                epp = f64::NAN;
                break;
            };
            let slug = name(reward, "reward_slug");
            let p = price(&r.market, &slug).unwrap_or(0.0);
            let vol = field(entry(&r.market, &slug), "vol");
            let item_count = if reward["item_count"].is_null() {
                1.0
            } else {
                num(&reward["item_count"])
            };
            epp += chance / 100.0 * p * item_count;
            if vol >= 5.0 {
                moving_count += 1;
            }
            rewards.push(RelicPlanReward {
                slug,
                name: name(reward, "reward_name"),
                rarity: name(reward, "rarity"),
                chance,
                low_sell: p,
                vol_48h: vol,
            });
        }
        if !epp.is_finite() || epp <= 0.0 {
            continue;
        }
        rewards.sort_by(|a, b| b.chance.total_cmp(&a.chance));
        let decision = decide_relic(table, &rec.slug, &r.market);
        out.push(RelicPlanEntry {
            relic_slug: rec.slug.clone(),
            relic_name: rec.name.clone(),
            owned: rec.count,
            epp,
            epp_owned: epp * rec.count,
            sell_now: decision.sell_now,
            moving_count,
            total_rewards: table.len(),
            rewards,
            decision: if decision.ladder.len() > 1 {
                Some(decision)
            } else {
                None
            },
        });
    }
    out.sort_by(|a, b| b.epp.total_cmp(&a.epp));
    cap(out, r.limit.unwrap_or(3))
}

pub fn validate_relic_request(r: &PlannerRequest) -> Result<(), String> {
    for rec in r
        .owned
        .iter()
        .filter(|rec| rec.subtype.as_deref() == Some("intact"))
    {
        let Some(table) = r
            .market
            .get("relic_rewards")
            .and_then(|rewards| rewards.get(&rec.slug))
            .and_then(Value::as_array)
        else {
            continue;
        };
        for reward in table {
            if !reward
                .get("chance")
                .and_then(Value::as_f64)
                .is_some_and(f64::is_finite)
                || ["reward_slug", "reward_name", "rarity"]
                    .iter()
                    .any(|key| !reward.get(key).is_some_and(Value::is_string))
                || reward.get("item_count").is_some_and(|value| {
                    !value.is_null() && !value.as_f64().is_some_and(f64::is_finite)
                })
            {
                return Err(
                    "The relic reward table contains invalid chance, identity, or item-count data."
                        .to_owned(),
                );
            }
        }
    }
    Ok(())
}
