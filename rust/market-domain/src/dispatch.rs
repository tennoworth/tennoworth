//! A single registry couples native operations with their input and output types.
use crate::{advisor::*, inventory::*, planners::*, scoring::*, trade_session::*};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

macro_rules! operations {
    ($($name:ident: $input:ty => $output:ty = $run:expr),+ $(,)?) => {
        #[derive(Deserialize, Serialize, TS)]
        #[serde(tag = "operation", content = "input", rename_all = "snake_case")]
        pub enum DomainRequest { $($name($input)),+ }

        #[derive(Serialize, TS)]
        #[serde(tag = "operation", content = "result", rename_all = "snake_case")]
        pub enum DomainResponse { $($name($output)),+ }

        impl DomainRequest {
            pub fn execute(self) -> Result<DomainResponse, String> {
                self.validate()?;
                match self { $(Self::$name(input) => ($run)(input).map(DomainResponse::$name)),+ }
            }
        }
    };
}

operations! {
    NormalizeInventory: InventoryRequest => NormalizedInventory = normalize_inventory,
    ScoreInventory: ScoreInventoryRequest => Vec<ScoredInventoryFact> = score_inventory,
    TradeSession: SessionRequest => SessionPlan = |input| Ok::<_, String>(select_session(input)),
    Advisor: AdvisorRequest => BTreeMap<String, Verdict> = |input| Ok::<_, String>(advise_owned(input)),
    History: HistoryRequest => HistoryAnalysis = |input| Ok::<_, String>(analyze_history(input)),
    RelicPlan: PlannerRequest => Vec<RelicPlanEntry> = |input| Ok::<_, String>(relic_plan(&input)),
    SetRecos: PlannerRequest => Vec<SetReco> = |input| Ok::<_, String>(set_recos(&input)),
    DucatPlan: DucatRequest => DucatResult = |input| Ok::<_, String>(ducat_plan(&input)),
    BuildPlan: BuildRequest => BuildResult = |input| Ok::<_, String>(build_plan(&input)),
}

impl DomainRequest {
    fn validate(&self) -> Result<(), String> {
        let json =
            serde_json::to_value(self).map_err(|_| "Invalid calculation request.".to_string())?;
        let mut pending = vec![(&json, 0_u8)];
        let mut nodes = 0_usize;
        while let Some((value, depth)) = pending.pop() {
            nodes += 1;
            if nodes > 4_000_000 || depth > 64 {
                return Err("The calculation request is too large.".into());
            }
            match value {
                serde_json::Value::Number(number)
                    if number
                        .as_f64()
                        .is_none_or(|v| !v.is_finite() || v.abs() > 9_007_199_254_740_991.0) =>
                {
                    return Err("The calculation contains an out-of-range number.".into())
                }
                serde_json::Value::Array(values) => {
                    pending.extend(values.iter().map(|value| (value, depth + 1)))
                }
                serde_json::Value::Object(values) => {
                    pending.extend(values.values().map(|value| (value, depth + 1)))
                }
                _ => (),
            }
        }
        let (owned, market) = match self {
            Self::RelicPlan(request) | Self::SetRecos(request) => {
                (Some(&request.owned), Some(&request.market))
            }
            Self::DucatPlan(request) => (Some(&request.owned), Some(&request.market)),
            Self::BuildPlan(request) => (Some(&request.owned), Some(&request.market)),
            _ => (None, None),
        };
        if let Some(owned) = owned {
            validate_owned(owned)?;
        }
        if let Some(market) = market {
            validate_planner_market(market)?;
        }
        match self {
            Self::Advisor(request) => validate_advisor_request(request)?,
            Self::History(request) => {
                validate_history_request(request)?;
                if request.buckets.unwrap_or(52) > 10_000 {
                    return Err("History bucket count is too large.".into());
                }
            }
            Self::TradeSession(request) => validate_session_request(request)?,
            Self::RelicPlan(request) => validate_relic_request(request)?,
            Self::DucatPlan(request) => validate_ducat_request(request)?,

            _ => (),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_public_history_scale_and_bounds_shape_before_series_work() {
        let items: serde_json::Map<String, serde_json::Value> = (0..3_000)
            .map(|i| {
                (
                    format!("item_{i}"),
                    serde_json::json!({"median": vec![10.0; 365]}),
                )
            })
            .collect();
        let request = DomainRequest::Advisor(AdvisorRequest {
            slugs: vec!["item_0".into()],
            market: serde_json::json!({"items": {}}),
            history: Some(serde_json::json!({"start":"2025-01-01", "items":items})),
            now_ms: 0.0,
        });
        assert!(request.validate().is_ok());
        let mut nested = serde_json::Value::Null;
        for _ in 0..65 {
            nested = serde_json::json!({"nested":nested});
        }
        let malformed = DomainRequest::Advisor(AdvisorRequest {
            slugs: vec!["item".into()],
            market: serde_json::json!({"items":{"item":{"median_now":"invalid"}},"extra":nested}),
            history: None,
            now_ms: 0.0,
        });
        assert_eq!(
            malformed.validate(),
            Err("The calculation request is too large.".into())
        );
    }

    #[test]
    fn runtime_probe_contracts_match_current_domain_code() {
        fn compare(actual: &serde_json::Value, expected: &serde_json::Value) {
            use serde_json::Value;
            match (actual, expected) {
                (Value::Number(a), Value::Number(b)) => {
                    let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
                    assert!((a - b).abs() <= 1e-9 * b.abs().max(1.0), "{a} != {b}");
                }
                (Value::Array(a), Value::Array(b)) => {
                    assert_eq!(a.len(), b.len());
                    for (a, b) in a.iter().zip(b) { compare(a, b); }
                }
                (Value::Object(a), Value::Object(b)) => {
                    assert_eq!(a.len(), b.len());
                    for (key, value) in b { compare(a.get(key).unwrap(), value); }
                }
                _ => assert_eq!(actual, expected),
            }
        }
        let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!("../../../tests/fixtures/domain-ipc/cases.json")).unwrap();
        for case in cases {
            let request: DomainRequest = serde_json::from_value(serde_json::json!({"operation":case["operation"],"input":case["input"]})).unwrap();
            let response = serde_json::to_value(request.execute().unwrap()).unwrap();
            compare(&response["result"], &case["expected"]);
        }
    }

    #[test]
    fn domain_bindings_match_rust() {
        let mut types = crate::bindings::TypeScript::default();
        types.add::<DomainRequest>();
        types.add::<DomainResponse>();
        let expected = types.finish().expect("unique domain types");
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../frontend/src/contracts/generated/domain.ts");
        if std::env::var_os("TENNOWORTH_UPDATE_BINDINGS").is_some() {
            std::fs::create_dir_all(file.parent().expect("parent")).expect("binding directory");
            std::fs::write(&file, &expected).expect("write domain bindings");
        }
        assert_eq!(std::fs::read_to_string(file).expect("run TENNOWORTH_UPDATE_BINDINGS=1 cargo test -p market-domain domain_bindings_match_rust"), expected);
    }

    #[test]
    fn command_response_carries_the_same_operation() {
        let request: DomainRequest = serde_json::from_value(serde_json::json!({"operation":"trade_session","input":{"candidates":[],"mode":"fast","budget":0,"target":null}})).expect("request");
        let result = serde_json::to_value(request.execute().expect("result")).expect("json");
        assert_eq!(result["operation"], "trade_session");
        assert_eq!(result["result"]["rows"], serde_json::json!([]));
        assert!(serde_json::from_value::<DomainRequest>(
            serde_json::json!({"operation":"place_order","input":{}})
        )
        .is_err());
    }
}
