use super::*;
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScrapCandidate {
    pub slug: String,
    pub name: String,
    pub spare: f64,
    pub ducats: f64,
    pub plat: f64,
    /// Null is an unpriced part: it sorts before finite ratios and crosses JSON without infinity.
    pub ducats_per_plat: Option<f64>,
    pub total_ducats: f64,
    pub total_plat: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DucatPlan {
    pub picks: Vec<ScrapCandidate>,
    pub ducats: f64,
    pub plat_given_up: f64,
    pub short: f64,
    pub held_back: Vec<ScrapCandidate>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DucatRequest {
    pub owned: Vec<PlannerOwned>,
    #[ts(type = "unknown")]
    pub market: Value,
    pub target: f64,
    pub keep_above: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct DucatResult {
    pub candidates: Vec<ScrapCandidate>,
    pub plan: DucatPlan,
}
pub fn ducat_plan(r: &DucatRequest) -> DucatResult {
    let mut candidates: Vec<ScrapCandidate> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for rec in &r.owned {
        if !seen.insert(&rec.slug) {
            continue;
        }
        let spare = (count(&r.owned, &rec.slug) - 1.0).max(0.0);
        let ducats = field(entry(&r.market, &rec.slug), "ducats");
        if spare == 0.0 || ducats <= 0.0 {
            continue;
        }
        let plat = price(&r.market, &rec.slug).unwrap_or(0.0);
        candidates.push(ScrapCandidate {
            slug: rec.slug.clone(),
            name: rec.name.clone(),
            spare,
            ducats,
            plat,
            ducats_per_plat: if plat > 0.0 {
                Some(ducats / plat)
            } else {
                None
            },
            total_ducats: ducats * spare,
            total_plat: plat * spare,
        });
    }
    candidates.sort_by(|a, b| {
        b.ducats_per_plat
            .unwrap_or(f64::INFINITY)
            .total_cmp(&a.ducats_per_plat.unwrap_or(f64::INFINITY))
            .then_with(|| b.ducats.total_cmp(&a.ducats))
    });
    let mut plan = DucatPlan {
        picks: vec![],
        ducats: 0.0,
        plat_given_up: 0.0,
        short: r.target.max(0.0),
        held_back: vec![],
    };
    for c in &candidates {
        if plan.ducats >= r.target {
            break;
        }
        if c.plat > r.keep_above.unwrap_or(15.0) {
            plan.held_back.push(c.clone());
            continue;
        }
        let copies = c
            .spare
            .min(((r.target - plan.ducats).max(0.0) / c.ducats).ceil());
        if copies <= 0.0 {
            continue;
        }
        let mut pick = c.clone();
        pick.spare = copies;
        pick.total_ducats = c.ducats * copies;
        pick.total_plat = c.plat * copies;
        plan.ducats += pick.total_ducats;
        plan.plat_given_up += pick.total_plat;
        plan.picks.push(pick);
    }
    plan.short = (r.target - plan.ducats).max(0.0);
    DucatResult { candidates, plan }
}
