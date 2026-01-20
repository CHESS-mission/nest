use std::{mem, sync::{Arc, RwLock}, time::{Instant, Duration}};

use nalgebra::{UnitQuaternion, Vector2, Vector3};
use ndarray::ArrayD;
use anyhow::{anyhow, Result};
use nest_api::SetEnvAction;

use crate::any_env_var_enum;

#[derive(thiserror::Error, Debug)]
pub enum EnvVarError {
    #[error("incorrect type for environment variable {0}: expected {1}, received {2}")]
    ApiTypeMismatch(String, String, String),
}


/// Environment variable types.
/// Available environment variable types are:
///
/// - [IntVar]
/// - [FloatVar]
/// - [BoolVar]
/// - [Vec2Var]
/// - [Vec3Var]
/// - [QuaternionVar]
/// - [TensorVar]

pub type IntVar = i32;
pub type FloatVar = f32;
pub type BoolVar = bool;
pub type Vec2Var = Vector2<f32>;
pub type Vec3Var = Vector3<f32>;
pub type QuaternionVar = UnitQuaternion<f32>;
pub type TensorVar = ArrayD<f32>;

pub trait Lerp: Clone {
    fn lerp(&self, other: &Self, t: f32) -> Self;
}

impl Lerp for IntVar {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        // Do not interpolate for integers
        if t < 1.0 { *self }
        else { *other }
    }
}

impl Lerp for FloatVar {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        self + t * (other - self)
    }
}

impl Lerp for BoolVar {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        // Do not interpolate for booleans
        if t < 1.0 { *self }
        else { *other }
    }
}

impl Lerp for Vec2Var {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        self + t * (other - self)
    }
}

impl Lerp for Vec3Var {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        self + t * (other - self)
    }
}

impl Lerp for QuaternionVar {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        // Interpolate only if the angle between the two quaternions is < 180 degrees
        if let Some(q) = self.try_slerp(other, t, 1e-6) {
            q
        } else if t < 1.0 {
            *self
        } else {
            *other
        }
    }
}

impl Lerp for TensorVar {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        // Interpolate only if the shapes are equal
        if self.shape() == other.shape() {
            self + t * (other - self)
        } else if t < 1.0 {
            self.clone()
        } else {
            other.clone()
        }
    }
}

#[derive(Debug, Clone)]
pub struct EnvironmentVar<V: Lerp + Default> {
    prev_value: Option<V>,
    lerp_start: Instant,
    curr_value: V,
    lerp_end: Instant,
}

impl<V: Lerp + Default> EnvironmentVar<V> {
    pub fn new(initial_value: V) -> Self {
        let now = Instant::now();
        Self {
            prev_value: None,
            lerp_start: now,
            curr_value: initial_value,
            lerp_end: now,
        }
    }

    pub fn set(&mut self, new_value: V) {
        let _ = mem::replace(&mut self.curr_value, new_value);
        self.prev_value = None;

        let now = Instant::now();
        self.lerp_start = now;
        self.lerp_end = now;
    }

    pub fn set_next(&mut self, next_value: V, at: Instant) {
        let old_curr = mem::replace(&mut self.curr_value, next_value);
        self.prev_value = Some(old_curr);

        self.lerp_start = Instant::now();
        self.lerp_end = at;
    }

    pub fn get(&self) -> V {
        let now = Instant::now();

        let Some(prev_value) = self.prev_value.as_ref() else {
            return self.curr_value.clone();
        };

        // If times are equal, avoid division by zero
        let span = self.lerp_end.duration_since(self.lerp_start);
        if span.is_zero() { return self.curr_value.clone(); }

        let elapsed = now.saturating_duration_since(self.lerp_start);

        // Compute interpolant and clamp to [0, 1] to avoid overshoot
        let t = elapsed.as_secs_f32() / span.as_secs_f32();
        let t = t.clamp(0.0, 1.0);

        prev_value.lerp(&self.curr_value, t)
    }
}

impl<V: Lerp + Default> Default for EnvironmentVar<V> {
    fn default() -> Self {
        Self {
            prev_value: None,
            lerp_start: Instant::now(),
            curr_value: V::default(),
            lerp_end: Instant::now()
        }
    }
}

pub type SharedEnvironmentVar<V> = Arc<RwLock<EnvironmentVar<V>>>;

any_env_var_enum!(AnyEnvironmentVar, EnvironmentVar);
any_env_var_enum!(AnySharedEnvironmentVar, SharedEnvironmentVar);

impl AnySharedEnvironmentVar {
    pub fn set_from_api(&self, set_env: SetEnvAction) -> Result<(), EnvVarError> {
        macro_rules! convert {
            ($var:expr, $expected:literal, $variant:ident) => {
                if let nest_api::EnvVarValue::$variant(val) = set_env.value {
                    let mut var = $var.write().unwrap();
                    if let Some(duration_s) = set_env.lerp_over_s {
                        var.set_next(val, Instant::now() + Duration::from_secs_f32(duration_s));
                    } else {
                        var.set(val);
                    }
                    Ok(())
                } else {
                    Err(EnvVarError::ApiTypeMismatch(
                        set_env.variable,
                        $expected.into(),
                        set_env.value.type_str().into(),
                    ))
                }
            };
        }

        match self {
            AnySharedEnvironmentVar::Int(var) => convert!(var, "int", Int),
            AnySharedEnvironmentVar::Float(var) => convert!(var, "float", Float),
            AnySharedEnvironmentVar::Bool(var) => convert!(var, "bool", Bool),
            AnySharedEnvironmentVar::Vec2(var) => convert!(var, "vec2", Vec2),
            AnySharedEnvironmentVar::Vec3(var) => convert!(var, "vec3", Vec3),
            AnySharedEnvironmentVar::Quaternion(var) => convert!(var, "quaternion", Quaternion),
            AnySharedEnvironmentVar::Tensor(var) => convert!(var, "tensor", Tensor),
        }
    }
}

