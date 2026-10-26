use crate::render::Snapshot;
use serde_json::{Map, Value};
use std::path::Path;

pub(super) fn snapshot(snapshot: &Snapshot, json_out: &Path) -> Result<(), String> {
    let json_str = serde_json::to_string(&snapshot).map_err(|e| format!("serialize: {e}"))?;
    write(json_out, &json_str)
}

/// `market-public.json`: the part of the snapshot the site's prerendered
/// pages read to refresh their prices - about a third of `market.json`'s
/// transfer. The fields are listed in the fixture the frontend tests against.
pub(super) fn public_snapshot(snapshot: &Snapshot, out: &Path) -> Result<(), String> {
    let full = serde_json::to_value(snapshot).map_err(|e| format!("serialize: {e}"))?;
    let json_str =
        serde_json::to_string(&public_projection(&full)?).map_err(|e| format!("serialize: {e}"))?;
    write(out, &json_str)
}

fn public_projection(market: &Value) -> Result<Value, String> {
    let fields: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/public-market/fields.json"
    ))
    .map_err(|e| format!("public-market fields: {e}"))?;
    let names = |key: &str| -> Vec<&str> {
        fields
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect()
    };
    let item_fields = names("item");
    let mut out = Map::new();
    for key in names("snapshot") {
        if let Some(value) = market.get(key) {
            out.insert(key.to_string(), value.clone());
        }
    }
    let items = market
        .get("items")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    // The pages name only priced items, so the rest of the catalog is dead weight.
    if let Some(catalog) = out.get_mut("catalog").and_then(Value::as_object_mut) {
        catalog.retain(|_, slug| slug.as_str().is_some_and(|slug| items.contains_key(slug)));
    }
    let items = items
        .into_iter()
        .map(|(slug, item)| {
            let kept = item_fields
                .iter()
                .filter_map(|field| {
                    item.get(*field)
                        .map(|value| (field.to_string(), value.clone()))
                })
                .collect();
            (slug, Value::Object(kept))
        })
        .collect();
    out.insert("items".into(), Value::Object(items));
    Ok(Value::Object(out))
}

fn write(json_out: &Path, json_str: &str) -> Result<(), String> {
    let tmp = json_out.with_extension("json.tmp");
    let parent = json_out.parent().unwrap_or(std::path::Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    std::fs::write(&tmp, json_str).map_err(|e| format!("write tmp: {e}"))?;
    std::fs::rename(&tmp, json_out).map_err(|e| format!("rename: {e}"))?;
    let meta = std::fs::metadata(json_out).map_err(|e| format!("stat: {e}"))?;
    eprintln!("Wrote {} ({} bytes)", json_out.display(), meta.len());

    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    #[test]
    fn the_public_snapshot_keeps_exactly_the_listed_fields() {
        let fields: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/public-market/fields.json"
        ))
        .unwrap();
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../frontend/public/market.json"
        );
        let market: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let public = super::public_projection(&market).unwrap();
        let listed = |key: &str| -> Vec<String> {
            let mut names: Vec<String> = fields[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect();
            names.sort();
            names
        };
        let mut top: Vec<String> = public
            .as_object()
            .unwrap()
            .keys()
            .filter(|k| *k != "items")
            .cloned()
            .collect();
        top.sort();
        assert_eq!(top, listed("snapshot"));
        assert_eq!(
            public["items"].as_object().unwrap().len(),
            market["items"].as_object().unwrap().len()
        );
        let item_fields = listed("item");
        for (slug, item) in public["items"].as_object().unwrap() {
            for (field, value) in item.as_object().unwrap() {
                assert!(item_fields.contains(field), "{slug}.{field} is not listed");
                assert_eq!(value, &market["items"][slug][field], "{slug}.{field}");
            }
            for field in &item_fields {
                assert_eq!(
                    item.get(field).is_some(),
                    market["items"][slug].get(field).is_some(),
                    "{slug}.{field} dropped"
                );
            }
        }
        let catalog = public["catalog"].as_object().unwrap();
        assert!(!catalog.is_empty());
        for (name, slug) in market["catalog"].as_object().unwrap() {
            let priced = market["items"].get(slug.as_str().unwrap()).is_some();
            assert_eq!(catalog.get(name), priced.then_some(slug), "catalog {name}");
        }
    }

    #[test]
    fn a_null_auxiliary_map_stays_null() {
        let market = serde_json::json!({"updated_at": "2026-10-26T00:00:00Z", "platform": "pc", "catalog": null, "set_to_parts": null, "items": {"a": {"median_now": 1, "low_sell": 2}}});
        assert_eq!(
            super::public_projection(&market).unwrap(),
            serde_json::json!({"updated_at": "2026-10-26T00:00:00Z", "platform": "pc", "catalog": null, "set_to_parts": null, "items": {"a": {"median_now": 1}}})
        );
    }
}
