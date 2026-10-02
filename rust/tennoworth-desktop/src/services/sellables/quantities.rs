use std::collections::BTreeMap;

use market_math::sell_priority;

use crate::persistence::Db;

use super::MarketData;

impl MarketData {
    fn traded_set_parts(
        &self,
        traded: &std::collections::BTreeSet<String>,
    ) -> Result<std::collections::BTreeSet<String>, String> {
        let mut parts = std::collections::BTreeSet::new();
        for (slug, set) in &self.set_to_parts {
            if traded.contains(&set.name.to_lowercase())
                || traded.contains(&format!("{} set", set.name.to_lowercase()))
                || traded
                    .iter()
                    .any(|name| self.catalog.get(name) == Some(slug))
            {
                parts.extend(self.set_recipe(slug)?.into_keys());
            }
        }
        Ok(parts)
    }
    pub fn session_recipes(&self) -> BTreeMap<String, BTreeMap<String, u32>> {
        self.set_to_parts
            .keys()
            .filter_map(|slug| {
                let recipe = self.set_recipe(slug).ok()?;
                (recipe.values().map(|n| u64::from(*n)).sum::<u64>() <= 6)
                    .then(|| (slug.clone(), recipe))
            })
            .collect()
    }
    pub fn has_item(&self, slug: &str) -> bool {
        self.items.contains_key(slug)
    }

    pub fn set_recipe(&self, slug: &str) -> Result<BTreeMap<String, u32>, String> {
        let recipe = self
            .set_to_parts
            .get(slug)
            .filter(|s| !s.parts.is_empty())
            .ok_or_else(|| {
                format!("Recipe for {slug} is unresolved; keep its protection until reviewed.")
            })?;
        let mut parts = BTreeMap::<String, u32>::new();
        for part in &recipe.parts {
            let quantity = part
                .quantity
                .filter(|q| *q > 0)
                .ok_or_else(|| format!("Recipe quantities for {slug} are unresolved."))?;
            if !self.has_item(&part.slug) || part.slug.ends_with("_set") {
                return Err(format!("Recipe identity for {slug} is unresolved."));
            }
            let count = parts.entry(part.slug.clone()).or_default();
            *count = count
                .checked_add(quantity)
                .ok_or("Recipe quantities exceed supported limits.")?;
        }
        Ok(parts)
    }

    pub fn owned_quantities(&self, db: &Db) -> Result<BTreeMap<String, (u32, u32)>, String> {
        let traded = db.traded_names_since_scan().map_err(|e| e.to_string())?;
        let traded_parts = self.traded_set_parts(&traded)?;
        let mut owned = BTreeMap::<String, (u32, u32)>::new();
        for item in db.latest_snapshot_items().map_err(|e| e.to_string())? {
            let Some((name, slug)) = self.resolve(&item.slug) else {
                continue;
            };
            if !self.has_item(&slug) || slug.ends_with("_set") || slug.ends_with("_relic") {
                continue;
            }
            let traded_part = traded_parts.contains(&slug);
            let row = owned.entry(slug).or_default();
            let count = u32::try_from(item.count).map_err(|_| "Inventory quantity is invalid.")?;
            row.0 = row
                .0
                .checked_add(count)
                .ok_or("Inventory quantity exceeds supported limits.")?;
            let unavailable = if traded_part || traded.contains(&name.to_lowercase()) {
                count
            } else {
                u32::try_from(item.leveled).map_err(|_| "Untradeable quantity is invalid.")?
            };
            row.1 = row
                .1
                .checked_add(unavailable)
                .ok_or("Inventory quantity exceeds supported limits.")?;
        }
        Ok(owned)
    }
    pub fn session_quantities(&self, db: &Db) -> Result<BTreeMap<String, u32>, String> {
        self.sellable_inventory(db).map(|rows| {
            let mut quantities: BTreeMap<String, u32> = rows
                .into_iter()
                .map(|(slug, (_, count))| (slug, count))
                .collect();
            for (slug, parts) in self.session_recipes() {
                let count = parts
                    .iter()
                    .map(|(part, required)| quantities.get(part).copied().unwrap_or(0) / required)
                    .min()
                    .unwrap_or(0);
                quantities.insert(slug, count);
            }
            quantities
        })
    }

    pub(super) fn sellable_inventory(
        &self,
        db: &Db,
    ) -> Result<BTreeMap<String, (String, u32)>, String> {
        let protected =
            crate::services::protection::ProtectionPlan::load(db)?.requirements(self)?;
        let reserve = reserve_copies(db);
        let traded = db.traded_names_since_scan().map_err(|e| e.to_string())?;
        let traded_parts = self.traded_set_parts(&traded)?;
        let reserves: BTreeMap<_, _> = db
            .get_reserves()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|r| (r.slug, r.keep.max(0)))
            .collect();
        let mut owned = BTreeMap::<String, (String, i64, i64)>::new();
        for item in db.latest_snapshot_items().map_err(|e| e.to_string())? {
            let Some((name, slug)) = self.resolve(&item.slug) else {
                continue;
            };
            // The log has no rank identity. Require another scan for an item
            // given away since this snapshot instead of guessing which tier left.
            if traded.contains(&name.to_lowercase()) || traded_parts.contains(&slug) {
                continue;
            }
            let row = owned.entry(slug).or_insert((name, 0, 0));
            row.1 = row.1.saturating_add(item.count.max(0));
            row.2 = row.2.saturating_add(item.leveled.max(0));
        }
        Ok(owned
            .into_iter()
            .map(|(slug, (name, count, leveled))| {
                let keep = reserve.max(reserves.get(&slug).copied().unwrap_or(0));
                let keep = keep.max(
                    leveled.saturating_add(i64::from(protected.get(&slug).copied().unwrap_or(0))),
                );
                let safe = sell_priority::sellable_qty(count, keep, leveled);
                (slug, (name, u32::try_from(safe).unwrap_or(0)))
            })
            .collect())
    }
}

/// Read the user's global "keep N copies" reserve from settings (the SPA's
/// `reserve-copies`), clamped ≥ 0. A missing/garbage value is 0 (no reserve),
/// matching the SPA's parse-or-zero.
fn reserve_copies(db: &Db) -> i64 {
    db.get_setting("reserve-copies")
        .ok()
        .flatten()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .filter(|&n| n >= 0)
        .unwrap_or(0)
}
