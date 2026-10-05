//! exfa-formats computes slot/hardpoint counts after subsystems from static data (no engine). Check it against
//! the engine for every strategic-cruiser subsystem, alone and one full set per hull, online and offline.
use exfa_core::data as d;
use exfa_core::request::*;
use exfa_formats::fitting::StaticFit;

fn req(ship: u32, subs: &[(u32, State)]) -> FitRequest {
    let mut r: FitRequest = serde_json::from_str(&format!("{{\"ship\":{{\"type_id\":{ship}}}}}")).unwrap();
    for &(t, st) in subs {
        r.modules.push(serde_json::from_value(serde_json::json!({"type_id": t, "slot": "subsystem", "state": st})).unwrap());
    }
    r
}

#[test]
fn subsystem_slots_match_engine() {
    let attrs = [d::a::hiSlots, d::a::medSlots, d::a::lowSlots, d::a::turretSlotsLeft, d::a::launcherSlotsLeft, d::a::rigSlots, d::a::maxSubSystems];
    let mut checked = 0;
    for (hull, name) in [(29986u32, "Legion"), (29984, "Tengu"), (29988, "Proteus"), (29990, "Loki")] {
        let subs: Vec<u32> = (0..d::type_count())
            .filter(|&ix| d::ty(ix).category == 32 && d::type_name(ix).starts_with(name) && d::type_published(ix))
            .map(d::type_id_at)
            .collect();
        assert!(subs.len() >= 12, "{name}: {} subsystems", subs.len());
        let mut cases: Vec<Vec<(u32, State)>> = subs.iter().map(|&s| vec![(s, State::Online)]).collect();
        cases.push(subs.iter().take(4).map(|&s| (s, State::Online)).collect());
        cases.push(subs.iter().take(4).map(|&s| (s, State::Offline)).collect());
        for c in cases {
            let r = req(hull, &c);
            let f = exfa_core::engine::Fit::build(&r).expect("build");
            let sf = StaticFit::new(&r).expect("static");
            for &a in &attrs {
                assert_eq!(sf.ship_attr(a), f.get(f.ship, a), "{name} {c:?} attr {a}");
                checked += 1;
            }
        }
    }
    assert!(checked > 300);
}
