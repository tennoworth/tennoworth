use super::*;
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct SetPart {
    pub slug: String,
    pub component_name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct RecipeIngredient {
    pub name: String,
    pub count: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub slug: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct RecipeEntry {
    #[serde(default)]
    #[ts(optional)]
    pub build_price: Option<f64>,
    #[serde(default)]
    #[ts(optional)]
    pub build_time: Option<f64>,
    #[serde(default)]
    #[ts(optional)]
    pub rush_price: Option<f64>,
    #[serde(default)]
    #[ts(optional)]
    pub ingredients: Option<Vec<RecipeIngredient>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum BuildPathKind {
    BuySet,
    BuyPartsBuild,
    BuyPartsRush,
    SellSpares,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BuildPath {
    pub kind: BuildPathKind,
    pub plat: f64,
    pub credits: f64,
    pub seconds: f64,
    pub unverified: Vec<RecipeIngredient>,
    pub saving_vs_set: f64,
    pub plat_known: bool,
    pub recipes_known: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct HavePart {
    pub slug: String,
    pub name: String,
    pub count: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct MissingPart {
    pub slug: String,
    pub name: String,
    pub price: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BuildPlan {
    pub set_slug: String,
    pub set_name: String,
    pub have: Vec<HavePart>,
    pub missing: Vec<MissingPart>,
    pub set_price: Option<f64>,
    pub paths: Vec<BuildPath>,
    pub incomplete: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BuildRequest {
    pub set_slug: String,
    pub set_name: String,
    pub parts: Vec<SetPart>,
    #[ts(type = "unknown")]
    pub market: Value,
    pub owned: Vec<PlannerOwned>,
    pub recipes: Option<std::collections::BTreeMap<String, RecipeEntry>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct BuildResult {
    pub plan: BuildPlan,
    pub cheapest: Option<BuildPath>,
}
pub fn build_plan(r: &BuildRequest) -> BuildResult {
    let mut have = Vec::new();
    let mut missing = Vec::new();
    for p in &r.parts {
        let held = count(&r.owned, &p.slug);
        if held > 0.0 {
            have.push(HavePart {
                slug: p.slug.clone(),
                name: p.component_name.clone(),
                count: held,
            });
        } else {
            missing.push(MissingPart {
                slug: p.slug.clone(),
                name: p.component_name.clone(),
                price: price(&r.market, &p.slug),
            });
        }
    }
    let set_price = price(&r.market, &r.set_slug);
    let incomplete = missing.iter().any(|m| m.price.is_none());
    let parts_cost: f64 = missing.iter().map(|m| m.price.unwrap_or(0.0)).sum();
    let (mut credits, mut seconds, mut rush, mut covered) = (0.0, 0.0, 0.0, 0);
    let mut unverified = Vec::new();
    for p in &r.parts {
        let Some(recipe) = r.recipes.as_ref().and_then(|recipes| recipes.get(&p.slug)) else {
            continue;
        };
        covered += 1;
        credits += recipe.build_price.unwrap_or(0.0);
        seconds += recipe.build_time.unwrap_or(0.0);
        rush += recipe.rush_price.unwrap_or(0.0);
        if let Some(ingredients) = &recipe.ingredients {
            for ing in ingredients {
                if ing.slug.as_deref().is_none_or(str::is_empty) {
                    unverified.push(ing.clone());
                }
            }
        }
    }
    let mut paths = Vec::new();
    if let Some(p) = set_price {
        paths.push(BuildPath {
            kind: BuildPathKind::BuySet,
            plat: p,
            credits: 0.0,
            seconds: 0.0,
            unverified: vec![],
            saving_vs_set: 0.0,
            plat_known: true,
            recipes_known: true,
        });
    }
    paths.push(BuildPath {
        kind: BuildPathKind::BuyPartsBuild,
        plat: parts_cost,
        credits,
        seconds,
        unverified: unverified.clone(),
        saving_vs_set: set_price.map_or(0.0, |p| p - parts_cost),
        plat_known: !incomplete,
        recipes_known: covered > 0,
    });
    if rush > 0.0 {
        paths.push(BuildPath {
            kind: BuildPathKind::BuyPartsRush,
            plat: parts_cost + rush,
            credits,
            seconds: 0.0,
            unverified,
            saving_vs_set: set_price.map_or(0.0, |p| p - (parts_cost + rush)),
            plat_known: !incomplete,
            recipes_known: covered > 0,
        });
    }
    let mut spare_value = 0.0;
    let mut spares_priced = true;
    for h in &have {
        let spares = (h.count - 1.0).max(0.0);
        if spares == 0.0 {
            continue;
        }
        match price(&r.market, &h.slug) {
            Some(p) => spare_value += p * spares,
            None => spares_priced = false,
        }
    }
    if spare_value > 0.0 {
        paths.push(BuildPath {
            kind: BuildPathKind::SellSpares,
            plat: -spare_value,
            credits: 0.0,
            seconds: 0.0,
            unverified: vec![],
            saving_vs_set: 0.0,
            plat_known: spares_priced,
            recipes_known: true,
        });
    }
    let cheapest = if incomplete {
        None
    } else {
        paths
            .iter()
            .filter(|p| !matches!(p.kind, BuildPathKind::SellSpares))
            .min_by(|a, b| a.plat.total_cmp(&b.plat))
            .cloned()
    };
    BuildResult {
        plan: BuildPlan {
            set_slug: r.set_slug.clone(),
            set_name: r.set_name.clone(),
            have,
            missing,
            set_price,
            paths,
            incomplete,
        },
        cheapest,
    }
}
