//! Static fitting view for the formats: the few ship/fighter values the formats need (slot and hardpoint counts
//! after subsystems, fighter squadron size), computed from the static data alone, without the engine.
//!
//! The only modifiers on these attributes are the subsystems' `slotModifier` / `hardPointModifierEffect` (modAdd of
//! hi/med/lowSlotModifier and turret/launcherHardPointModifier onto the ship), so base + sum is exact. The view
//! mirrors the engine's build: it is `None` when any fitted type is unknown (the engine refuses such a fit).
use eve_fit_model::*;
use eve_sde as d;

pub struct StaticFit<'a> {
    req: &'a FitRequest,
}

fn known(id: u32) -> bool {
    d::type_index(id).is_some()
}

fn modules_known(ms: &[ModuleReq]) -> bool {
    ms.iter().all(|m| known(m.type_id) && m.charge_type_id.map(known).unwrap_or(true))
}

/// Same acceptance as the engine's fit build: every ship/mode/module/charge/drone/fighter/implant/booster/
/// environment/projected type must exist (unknown skills are skipped, booster fits are optional).
fn build_ok(req: &FitRequest) -> bool {
    known(req.ship.type_id)
        && req.ship.mode_type_id.map(known).unwrap_or(true)
        && modules_known(&req.modules)
        && req.drones.iter().all(|x| known(x.type_id))
        && req.fighters.iter().all(|x| known(x.type_id))
        && req.implants.iter().all(|&x| known(x))
        && req.boosters.iter().all(|x| known(x.type_id))
        && req.environment.effect_type_ids.iter().all(|&x| known(x))
        && req.projected.iter().all(|p| match p.kind.as_str() {
            "module" => p.module.as_ref().map(|m| modules_known(std::slice::from_ref(m))).unwrap_or(true),
            "drone" => p.drone.as_ref().map(|x| known(x.type_id)).unwrap_or(true),
            "fighter" => p.fighter.as_ref().map(|x| known(x.type_id)).unwrap_or(true),
            "fit" => p.fit.as_ref().map(|f| {
                let mut sub = (**f).clone();
                sub.projected.clear();
                build_ok(&sub)
            }).unwrap_or(true),
            _ => true,
        })
}

impl<'a> StaticFit<'a> {
    pub fn new(req: &'a FitRequest) -> Option<Self> {
        if build_ok(req) { Some(StaticFit { req }) } else { None }
    }

    /// Base value of `attr` on an item of type `type_id`: request override (last wins), else the type's value,
    /// else the attribute default.
    fn base(&self, type_id: u32, attr: u16) -> f64 {
        if let Some(o) = self.req.overrides.iter().rev().find(|o| o.type_id == type_id && o.attribute_id == attr as u32) {
            return o.value;
        }
        d::type_index(type_id).and_then(|ix| d::type_attr(ix, attr)).unwrap_or_else(|| d::attr_default(attr))
    }

    fn has_effect(type_id: u32, eff: u32) -> bool {
        d::type_index(type_id).map(|ix| d::type_effects(ix).iter().any(|&x| d::EFF_IDS[(x >> 1) as usize] == eff)).unwrap_or(false)
    }

    /// Ship attribute with the subsystem slot / hardpoint modifiers applied (modules online or better).
    pub fn ship_attr(&self, attr: u16) -> f64 {
        let mut v = self.base(self.req.ship.type_id, attr);
        let pairs: [(u32, u16, u16); 5] = [
            (d::e::slotModifier, d::a::hiSlots, d::a::hiSlotModifier),
            (d::e::slotModifier, d::a::medSlots, d::a::medSlotModifier),
            (d::e::slotModifier, d::a::lowSlots, d::a::lowSlotModifier),
            (d::e::hardPointModifierEffect, d::a::turretSlotsLeft, d::a::turretHardPointModifier),
            (d::e::hardPointModifierEffect, d::a::launcherSlotsLeft, d::a::launcherHardPointModifier),
        ];
        for (eff, target, src) in pairs {
            if target != attr {
                continue;
            }
            for m in &self.req.modules {
                if m.state == Some(State::Offline) || !Self::has_effect(m.type_id, eff) {
                    continue;
                }
                v += self.base(m.type_id, src);
            }
        }
        v
    }

    /// Unmodified (nothing modifies it) attribute of request fighter `i`.
    pub fn fighter_attr(&self, i: usize, attr: u16) -> Option<f64> {
        self.req.fighters.get(i).map(|f| self.base(f.type_id, attr))
    }
}
