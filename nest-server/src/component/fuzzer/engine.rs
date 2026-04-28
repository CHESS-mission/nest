use rand::rngs::StdRng;
use rand::SeedableRng;
use serde::Deserialize;
use serde_json::Value;

use crate::ports::{BytesMessage, I2cTransaction};

use super::strategy::{strategy_from_config, FuzzStrategy, MutationResult};

/// Which direction a message is travelling through the fuzzer.
#[derive(Clone, Copy, Debug)]
pub enum Direction {
    CompToSyst,
    SystToComp,
}

/// A directional fuzz profile containing an ordered pipeline of strategies.
pub struct FuzzProfile {
    pub enabled: bool,
    pub strategies: Vec<Box<dyn FuzzStrategy>>,
}

impl Default for FuzzProfile {
    fn default() -> Self {
        Self {
            enabled: false,
            strategies: Vec::new(),
        }
    }
}

/// Deserializable config for a fuzz profile (one direction).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct FuzzProfileConfig {
    pub enabled: bool,
    /// Ordered list of strategy configs.
    pub strategies: Vec<Value>,
}

impl FuzzProfileConfig {
    /// Build a `FuzzProfile` with instantiated strategy objects.
    pub fn build(&self) -> anyhow::Result<FuzzProfile> {
        let mut strategies: Vec<Box<dyn FuzzStrategy>> = Vec::new();

        for config in &self.strategies {
            strategies.push(strategy_from_config(config)?);
        }

        Ok(FuzzProfile {
            enabled: self.enabled,
            strategies,
        })
    }
}

pub struct FuzzEngine {
    pub name: String,
    pub comp_to_syst: FuzzProfile,
    pub syst_to_comp: FuzzProfile,
    pub rng: StdRng,
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

    /// Process a BytesMessage through the strategy pipeline.
    pub fn process_bytes(&mut self, mut msg: BytesMessage, dir: Direction) -> (Option<BytesMessage>, Vec<String>) {
        let profile = match dir {
            Direction::CompToSyst => &mut self.comp_to_syst,
            Direction::SystToComp => &mut self.syst_to_comp,
        };
        let mut log_entries = Vec::new();

        if !profile.enabled {
            return (Some(msg), log_entries);
        }

        for strategy in &mut profile.strategies {
            if !strategy.enabled() {
                continue;
            }
            match strategy.apply_bytes(&mut msg, &mut self.rng) {
                MutationResult::Passed => {}
                MutationResult::Mutated(desc) => {
                    log_entries.push(format!("[{}] {}", strategy.name(), desc));
                }
                MutationResult::Buffered(desc) => {
                    log_entries.push(format!("[{}] buffered: {}", strategy.name(), desc));
                    return (None, log_entries);
                }
                MutationResult::Dropped => {
                    log_entries.push(format!("[{}] dropped", strategy.name()));
                    return (None, log_entries);
                }
            }
        }

        (Some(msg), log_entries)
    }

    /// Process an I2cTransaction through the strategy pipeline.
    pub fn process_i2c(&mut self, mut msg: I2cTransaction, dir: Direction) -> (Option<I2cTransaction>, Vec<String>) {
        let profile = match dir {
            Direction::CompToSyst => &mut self.comp_to_syst,
            Direction::SystToComp => &mut self.syst_to_comp,
        };
        let mut log_entries = Vec::new();

        if !profile.enabled {
            return (Some(msg), log_entries);
        }

        for strategy in &mut profile.strategies {
            if !strategy.enabled() {
                continue;
            }
            match strategy.apply_i2c(&mut msg, &mut self.rng) {
                MutationResult::Passed => {}
                MutationResult::Mutated(desc) => {
                    log_entries.push(format!("[{}] {}", strategy.name(), desc));
                }
                MutationResult::Buffered(desc) => {
                    log_entries.push(format!("[{}] buffered: {}", strategy.name(), desc));
                    return (None, log_entries);
                }
                MutationResult::Dropped => {
                    log_entries.push(format!("[{}] dropped", strategy.name()));
                    return (None, log_entries);
                }
            }
        }

        (Some(msg), log_entries)
    }

    /// Generic passthrough for message types without specific mutations (Bool, Int, Float, Text).
    /// Only drop strategies are applied.
    pub fn process_generic<M>(&mut self, msg: M, dir: Direction) -> (Option<M>, Vec<String>) {
        let profile = match dir {
            Direction::CompToSyst => &mut self.comp_to_syst,
            Direction::SystToComp => &mut self.syst_to_comp,
        };
        let mut log_entries = Vec::new();

        if !profile.enabled {
            return (Some(msg), log_entries);
        }

        for strategy in &mut profile.strategies {
            if !strategy.enabled() || !strategy.can_drop() {
                continue;
            }
            if strategy.should_drop(&mut self.rng) {
                log_entries.push(format!("[{}] dropped", strategy.name()));
                return (None, log_entries);
            }
        }

        (Some(msg), log_entries)
    }
}
