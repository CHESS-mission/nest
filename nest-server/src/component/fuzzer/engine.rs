use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::Deserialize;

/// Which direction a message is travelling through the fuzzer.
#[derive(Clone, Copy, Debug)]
pub enum Direction {
    CompToSyst,
    SystToComp,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct FuzzProfile {
    pub enabled: bool,
    pub drop_prob: f64,
}

impl Default for FuzzProfile {
    fn default() -> Self {
        Self {
            enabled: false,
            drop_prob: 0.1,
        }
    }
}

pub struct FuzzEngine {
    pub name: String,
    pub comp_to_syst: FuzzProfile,
    pub syst_to_comp: FuzzProfile,
    rng: StdRng,
}

impl FuzzEngine {
    pub fn new(
        name: String,
        comp_to_syst: FuzzProfile,
        syst_to_comp: FuzzProfile,
        seed: Option<u64>,
    ) -> Self {
        Self {
            name,
            comp_to_syst,
            syst_to_comp,
            rng: match seed {
                Some(s) => StdRng::seed_from_u64(s),
                None => StdRng::from_entropy(),
            },
        }
    }

    pub fn profile(&self, dir: Direction) -> &FuzzProfile {
        match dir {
            Direction::CompToSyst => &self.comp_to_syst,
            Direction::SystToComp => &self.syst_to_comp,
        }
    }

    pub fn profile_mut(&mut self, dir: Direction) -> &mut FuzzProfile {
        match dir {
            Direction::CompToSyst => &mut self.comp_to_syst,
            Direction::SystToComp => &mut self.syst_to_comp,
        }
    }

    pub fn process<M>(&mut self, msg: M, dir: Direction) -> Option<M> {
        let profile = self.profile(dir);

        if !profile.enabled {
            return Some(msg);
        }

        if profile.drop_prob > 0.0 && self.rng.gen_bool(profile.drop_prob.clamp(0.0, 1.0)) {
            return None;
        }

        Some(msg)
    }
}
