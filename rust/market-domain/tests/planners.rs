use market_domain::planners::*;
use serde_json::Value;
fn compare(actual: &Value, expected: &Value, path: &str) {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => {
            let a = a.as_f64().unwrap_or(f64::NAN);
            let b = b.as_f64().unwrap_or(f64::NAN);
            assert!(
                (a - b).abs() <= 1e-10_f64.max(b.abs() * 1e-12),
                "{path}: {a} != {b}"
            );
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{path}");
            for (i, (a, b)) in a.iter().zip(b).enumerate() {
                compare(a, b, &format!("{path}[{i}]"));
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len(), "{path}: {actual} != {expected}");
            for (k, v) in b {
                compare(a.get(k).unwrap_or(&Value::Null), v, &format!("{path}.{k}"));
            }
        }
        _ => assert_eq!(actual, expected, "{path}"),
    }
}
#[test]
fn planner_shared_contracts() {
    let fixtures: Vec<Value> =
        serde_json::from_str(include_str!("../../../tests/fixtures/planners/cases.json")).unwrap();
    for case in fixtures {
        let input = case["input"].clone();
        let actual = match case["operation"].as_str().unwrap() {
            "sets" => {
                serde_json::to_value(set_recos(&serde_json::from_value(input).unwrap())).unwrap()
            }
            "relic" => {
                serde_json::to_value(relic_plan(&serde_json::from_value(input).unwrap())).unwrap()
            }
            "ducat" => {
                serde_json::to_value(ducat_plan(&serde_json::from_value(input).unwrap())).unwrap()
            }
            "build" => {
                serde_json::to_value(build_plan(&serde_json::from_value(input).unwrap())).unwrap()
            }
            _ => panic!("unknown fixture"),
        };
        compare(&actual, &case["expected"], case["name"].as_str().unwrap());
    }
}

#[test]
fn malformed_relic_rewards_are_rejected_before_planning() {
    for chance in [
        serde_json::json!(null),
        serde_json::json!("5"),
        serde_json::json!("invalid"),
    ] {
        let request = PlannerRequest {
            owned: vec![PlannerOwned {
                slug: "r".into(),
                name: "Relic".into(),
                count: 1.0,
                subtype: Some("intact".into()),
            }],
            market: serde_json::json!({"relic_rewards":{"r":[{"reward_slug":"a","reward_name":"A","rarity":"rare","chance":chance}]}}),
            limit: None,
        };
        assert!(validate_relic_request(&request).is_err());
    }
}

#[test]
fn owned_boundary_preserves_zero_and_rejects_inexact_quantities() {
    let row = PlannerOwned {
        slug: "a".into(),
        name: "A".into(),
        count: 0.0,
        subtype: None,
    };
    assert!(validate_owned(&[]).is_ok());
    assert!(validate_owned(std::slice::from_ref(&row)).is_ok());
    for count in [-1.0, 0.5, f64::NAN, f64::INFINITY, 9_007_199_254_740_992.0] {
        assert!(validate_owned(&[PlannerOwned {
            count,
            ..row.clone()
        }])
        .is_err());
    }
    let max = PlannerOwned {
        count: 9_007_199_254_740_991.0,
        ..row.clone()
    };
    assert!(validate_owned(std::slice::from_ref(&max)).is_ok());
    assert!(validate_owned(&[max, PlannerOwned { count: 1.0, ..row }]).is_err());
}

#[test]
fn market_numeric_strings_cannot_overflow_planner_outputs() {
    for market in [
        serde_json::json!({"items":{"a":{"low_sell":"1e308"}}}),
        serde_json::json!({"items":{"a":{"median_now":"inf"}}}),
        serde_json::json!({"set_to_parts":{"s":{"parts":[{"quantity":"1e308"}]}}}),
        serde_json::json!({"relic_rewards":{"r":[{"item_count":"1e308"}]}}),
    ] {
        assert!(validate_planner_market(&market).is_err());
    }
    assert!(validate_planner_market(
        &serde_json::json!({"items":{"a":{"low_sell":"12.5","avg":null}}})
    )
    .is_ok());
}

#[test]
fn subnormal_prices_cannot_turn_a_priced_ducat_ratio_into_null() {
    let mut request = DucatRequest {
        owned: vec![PlannerOwned {
            slug: "a".into(),
            name: "A".into(),
            count: 3.0,
            subtype: None,
        }],
        market: serde_json::json!({"items":{"a":{"low_sell":5e-324,"ducats":100}}}),
        target: 45.0,
        keep_above: Some(15.0),
        quantities_are_available: None,
    };
    assert!(validate_ducat_request(&request).is_err());
    request.market = serde_json::json!({"items":{"a":{"low_sell":0,"ducats":100}}});
    assert!(validate_ducat_request(&request).is_ok());
    let plan = ducat_plan(&request);
    assert!(plan
        .candidates
        .first()
        .is_some_and(|candidate| candidate.ducats_per_plat.is_none()));
    request.target = -1.0;
    assert!(validate_ducat_request(&request).is_ok());
    assert_eq!(ducat_plan(&request).plan.ducats, 0.0);
}
