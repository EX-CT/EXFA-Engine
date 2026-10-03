//! Property tests for the optimizer (docs/21 §5): results are valid fits, respect skills / floors / keep,
//! are monotone in skills and deterministic.
use serde_json::{json, Value};

fn base(level: u8) -> Value {
    json!({"schema_version": 1, "ship": {"type_id": 587},
           "character": {"skills": {"default_level": level}},
           "modules": [
               {"type_id": 484, "state": "active", "charge_type_id": 12625},
               {"type_id": 484, "state": "active", "charge_type_id": 12625},
               {"type_id": 484, "state": "active", "charge_type_id": 12625},
               {"type_id": 10190, "state": "online"}]})
}

fn run(req: Value) -> Value {
    let r = eve_optimizer::optimize_value(&req);
    assert!(r.get("results").is_some(), "optimizer error: {r}");
    r
}

fn results(r: &Value) -> &Vec<Value> {
    r["results"].as_array().unwrap()
}

#[test]
fn feasible_results_have_no_violations_and_respect_keep_and_floors() {
    let r = run(json!({"base": base(5), "objective": {"metric": "dps"},
                       "constraints": {"min": {"ehp": 2500.0}},
                       "search": {"keep": [3], "candidates": "group"},
                       "limits": {"max_evaluations": 2500, "results": 3}}));
    let rs = results(&r);
    assert!(!rs.is_empty());
    for row in rs {
        assert_eq!(row["feasible"], json!(true), "{row}");
        let stats: Value = serde_json::from_str(&eve_dogma::calc_json(&row["fit"].to_string())).unwrap();
        let v = stats["violations"].as_array().map(|a| a.len()).unwrap_or(0);
        assert_eq!(v, 0, "violations in result: {}", stats["violations"]);
        assert!(row["metrics"]["ehp"].as_f64().unwrap() >= 2500.0);
        let kept = &row["fit"]["modules"].as_array().unwrap().iter().filter(|m| m["type_id"] == json!(10190)).count();
        assert_eq!(*kept, 1, "keep[3] (10190) must stay fitted");
    }
    // ranks are 1..n and objectives non-increasing
    let objs: Vec<f64> = rs.iter().map(|x| x["objective"].as_f64().unwrap()).collect();
    assert!(objs.windows(2).all(|w| w[0] >= w[1] - 1e-9), "{objs:?}");
}

#[test]
fn skills_constraint_holds_and_is_monotone() {
    let mk = |lvl| json!({"base": base(lvl), "objective": {"metric": "dps"},
                         "search": {"candidates": "group"},
                         "limits": {"max_evaluations": 2500, "results": 1}});
    let r0 = run(mk(0));
    let r5 = run(mk(5));
    let top0 = &results(&r0)[0];
    let top5 = &results(&r5)[0];
    // feasible = no MISSING_SKILL beyond the fixed hull's own requirements (reported as a warning)
    assert_eq!(top0["feasible"], json!(true), "{top0}");
    assert_eq!(top5["missing_skills"], json!([]));
    let hull = [3329, 3327]; // Minmatar Frigate, Spaceship Command (Rifter)
    for s in top0["missing_skills"].as_array().unwrap() {
        assert!(hull.contains(&s.as_u64().unwrap()), "module skill missing at All 0: {s}");
    }
    assert!(r0["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("fixed part")));
    let (a, b) = (top0["objective"].as_f64().unwrap(), top5["objective"].as_f64().unwrap());
    assert!(a <= b + 1e-9, "All 0 ({a}) must not beat All 5 ({b})");
}

#[test]
fn deterministic() {
    let req = json!({"base": base(5), "objective": [{"metric": "dps", "weight": 1.0, "scale": 100.0},
                                                   {"metric": "ehp", "weight": 0.5, "scale": 2000.0}],
                     "limits": {"max_evaluations": 1500, "results": 3, "seed": 7}});
    let a = run(req.clone());
    let b = run(req);
    assert_eq!(a["results"], b["results"]);
    assert_eq!(a["evaluated"], b["evaluated"]);
}

#[test]
fn errors() {
    let r = eve_optimizer::optimize_value(&json!({"base": base(5), "objective": {"metric": "nope"}}));
    assert_eq!(r["error"]["code"], json!("OPT_BAD_METRIC"), "{r}");
    let mut b = base(5);
    b["character"]["clone"] = json!("alpha");
    let r = eve_optimizer::optimize_value(&json!({"base": b}));
    assert_eq!(r["error"]["code"], json!("UNSUPPORTED"));
    let r = eve_optimizer::optimize_value(&json!({"base": base(5), "constraints": {"min": {"ehp": 1e12}},
                                                  "limits": {"max_evaluations": 300}}));
    assert_eq!(r["error"]["code"], json!("OPT_NO_FEASIBLE"), "{r}");
    let r = eve_optimizer::optimize_value(&json!({"base": base(5), "constraints": {"price": {"max_isk": 1e6, "prices": {"484": 1000.0}}},
                                                  "limits": {"max_evaluations": 300}}));
    assert_eq!(r["error"]["code"], json!("OPT_MISSING_PRICE"), "{r}");
}
