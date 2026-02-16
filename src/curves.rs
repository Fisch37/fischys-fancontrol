//! This module is the core of the entire project.
//! It features the [`PwmControl`] trait and all of its implementors,
//! which produce a target value for a [`FanController`], given the current [`SensorStorage`].
//!
//! For more detail, see the object descriptions.

mod composite;
mod interpolated;
mod single;

pub use self::{
    composite::MultiSensorControl,
    interpolated::{InterpolatedSensorControl, InterpolationData, InterpolationMode},
    single::SingleSensorControl,
};
pub type SensorControl = Box<dyn PwmControl>;

use std::collections::HashMap;

use log::{info, log_enabled, warn};
use serde::{Deserialize, Serialize};

use crate::{
    controllers::FanController,
    groupie::SensorStorage,
    utils::{ErrorGroup, SimpleError},
};

#[doc(hidden)]
/// Returns the average of a set of floats,
/// or NaN if the iterator is empty.
fn f64_avg<I: IntoIterator<Item = f64>>(iterator: I) -> Option<f64> {
    let mut sum: f64 = 0.0;
    let mut count: i64 = 0;
    for v in iterator {
        sum += v;
        count += 1;
    }
    // count is not an f64 initially as that may cause it to get stuck during incrementation
    // due to float imprecision in large numbers.
    // Rounding to the next float at the end ensures an accurate(ish) value
    // (many months later) wait, why am I worrying about precision errors that occur only at 9007199254740992?
    Some(sum / (count as f64)).filter(|x| x.is_nan())
}

#[doc(hidden)]
fn f64_median<I: IntoIterator<Item = f64>>(iterator: I) -> Option<f64> {
    let mut values: Vec<f64> = iterator.into_iter().collect();
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        (values[middle] + values[middle + 1]) / 2.0
    } else {
        values[middle + 1]
    })
}

#[doc(hidden)]
const fn f64_1() -> f64 {
    1.0
}

#[typetag::serde(tag = "type")]
/// This is a simple trait for types that hold information to calculate a PWM value
/// from a set of sensor inputs.
pub trait PwmControl: std::fmt::Debug {
    /// Determine the target PWM value from a [`SensorStorage`].
    ///
    /// Returns [`None`] if the current state does not allow for a value to be determined,
    /// else [`Some`] holding any valid floating point value.
    /// Callers should not make any assumption as to the bounds or realness of this value
    /// and should be prepared to handle special values (such as -inf, inf, NaN) correctly.
    fn evaluate(&self, state: &SensorStorage) -> Option<f64>;
}
#[typetag::serde(name = "literal")]
/// A pwm literal value
impl PwmControl for f64 {
    fn evaluate(&self, _: &SensorStorage) -> Option<f64> {
        Some(*self)
    }
}

#[derive(Serialize, Deserialize, Debug)]
/// The root for a particular PWM curve configuration.
///
/// Now only a wrapper around [`InterpolatedSensorControl`].
/// See that for details.
pub struct PwmCurve {
    #[serde(flatten)]
    interpolate_control: InterpolatedSensorControl,
}

/// Adjusts a given set of [`FanController`]s by a [`PwmCurve`],
/// if a curve for that controller is defined, calling `for_each_update`
/// for every controller that was updated with `(pwm_key, sensor_value, pwm_value)`.
///
/// Handles errors permissively, always running to the end and collecting any errors
/// that might occur, before returning an [`Err`] if any errors did occur.
pub fn update_pwms<'a, C: AsMut<dyn FanController + 'a>>(
    state: &SensorStorage,
    pwms: &mut [C],
    curves: &HashMap<String, PwmCurve>,
    for_each_update: &mut impl FnMut((&str, f64, f64)),
) -> Result<(), ErrorGroup> {
    // don't want to set a capacity here. normally empty
    let mut errors = ErrorGroup::new();

    for pwm in pwms.iter_mut().map(AsMut::as_mut) {
        let pwm_name = pwm.get_key();
        let curve = match curves.get(pwm_name) {
            None => continue,
            Some(x) => x,
        };
        let InterpolationData {
            input_value,
            value: mut target_pwm,
            low_index,
            high_index,
        } = match curve.interpolate_control.evaluate_meta(state) {
            Some(x) => x,
            None => {
                errors.push(SimpleError::from(format!(
                    "Error evaluating control for {pwm_name}. See the log for details.",
                )));
                continue;
            }
        };
        let (pwm_min, pwm_max) = pwm.get_min_max_value();
        // Receiving NaN is acceptable, as FanController::write_value will simply error
        target_pwm = f64::clamp(target_pwm, pwm_min, pwm_max);
        for_each_update((pwm_name, input_value, target_pwm));
        if log_enabled!(log::Level::Info) {
            info!(
                "{pwm_name}: {input_value:.1}°C (li {low_index:?}; hi {high_index:?}) -> {target_pwm}"
            );
        }
        if let Err(e) = pwm.write_value(target_pwm) {
            warn!("Failed to set pwm for {}: {}", pwm.get_key(), e);
            errors.push(e);
        }
    }

    errors.ok()
}
