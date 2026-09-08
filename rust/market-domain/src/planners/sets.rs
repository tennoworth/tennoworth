use super::*;
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum SetRecoKind {
    NearComplete,
    CompleteWithExtras,
    Extras,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct SetRecoPart {
    pub slug: String,
    pub name: String,
    pub count: f64,
    pub required: f64,
    pub low_sell: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct MissingSetPart {
    pub slug: String,
    pub name: String,
    pub quantity: f64,
    pub low_sell: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct SetReco {
    pub kind: SetRecoKind,
    pub set_slug: String,
    pub set_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub set_low_sell: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub set_top_buy: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub set_vol: Option<f64>,
    pub parts: Vec<SetRecoPart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub parts_low_sell: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub missing: Option<Vec<MissingSetPart>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub missing_cost: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub instant_uplift: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub extras: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub extras_plat: Option<f64>,
    pub net_plat: f64,
}
pub fn set_recos(r: &PlannerRequest) -> Vec<SetReco> {
    let Some(sets) = r.market.get("set_to_parts").and_then(Value::as_object) else {
        return vec![];
    };
    let mut out = Vec::new();
    for (slug, info) in sets {
        let Some(parts) = info["parts"].as_array().filter(|p| !p.is_empty()) else {
            continue;
        };
        let (mut owned_units, mut required_units, mut extra_copies, mut extras_plat, mut parts_sum) =
            (0.0, 0.0, 0.0, 0.0, 0.0);
        let mut missing = Vec::new();
        let mut rows = Vec::new();
        for p in parts {
            let part_slug = name(p, "slug");
            let part_name = name(p, "component_name");
            let cnt: f64 = r
                .owned
                .iter()
                .filter(|rec| {
                    rec.subtype.as_deref().is_none_or(str::is_empty) && rec.slug == part_slug
                })
                .map(|rec| rec.count)
                .sum();
            let required = field(p, "quantity").floor().max(1.0);
            let low_sell = field(entry(&r.market, &part_slug), "low_sell");
            let owned_for_set = cnt.min(required);
            let shortfall = (required - cnt).max(0.0);
            required_units += required;
            owned_units += owned_for_set;
            parts_sum += owned_for_set * low_sell;
            rows.push(SetRecoPart {
                slug: part_slug.clone(),
                name: part_name.clone(),
                count: cnt,
                required,
                low_sell,
            });
            if cnt > required {
                extra_copies += cnt - required;
                extras_plat += (cnt - required) * low_sell;
            }
            if shortfall > 0.0 {
                missing.push(MissingSetPart {
                    slug: part_slug,
                    name: part_name,
                    quantity: shortfall,
                    low_sell,
                });
            }
        }
        if owned_units == 0.0 {
            continue;
        }
        let set_entry = entry(&r.market, slug);
        let set_price = field(set_entry, "low_sell");
        let set_buy = field(set_entry, "top_buy");
        let vol = field(set_entry, "vol");
        let mut reco = SetReco {
            kind: SetRecoKind::Extras,
            set_slug: slug.clone(),
            set_name: name(info, "name"),
            set_low_sell: None,
            set_top_buy: None,
            set_vol: None,
            parts: rows,
            parts_low_sell: None,
            missing: None,
            missing_cost: None,
            instant_uplift: None,
            extras: None,
            extras_plat: None,
            net_plat: 0.0,
        };
        if owned_units == required_units {
            if extra_copies <= 0.0 {
                continue;
            }
            reco.kind = SetRecoKind::CompleteWithExtras;
            reco.set_low_sell = Some(set_price);
            reco.set_vol = Some(vol);
            reco.extras = Some(extra_copies);
            reco.extras_plat = Some(extras_plat);
            reco.net_plat = extras_plat;
        } else if required_units - owned_units <= 2.0
            && set_price > 0.0
            && missing.iter().all(|p| p.low_sell > 0.0)
        {
            let cost: f64 = missing.iter().map(|p| p.quantity * p.low_sell).sum();
            let uplift = set_price - cost - parts_sum;
            if uplift <= 0.0 {
                continue;
            }
            reco.kind = SetRecoKind::NearComplete;
            reco.set_low_sell = Some(set_price);
            reco.set_top_buy = (set_buy != 0.0).then_some(set_buy);
            reco.set_vol = Some(vol);
            reco.parts_low_sell = Some(parts_sum);
            reco.missing = Some(missing);
            reco.missing_cost = Some(cost);
            reco.instant_uplift = (set_buy > 0.0).then_some(set_buy - cost - parts_sum);
            reco.net_plat = uplift;
        } else if extra_copies > 0.0 {
            reco.extras = Some(extra_copies);
            reco.extras_plat = Some(extras_plat);
            reco.net_plat = extras_plat;
        } else {
            continue;
        }
        out.push(reco);
    }
    out.sort_by(|a, b| b.net_plat.total_cmp(&a.net_plat));
    cap(out, r.limit.unwrap_or(24))
}
