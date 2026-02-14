mod composite;
mod interpolated;
mod single;

pub use self::{
    single::SingleSensorControl,
    composite::MultiSensorControl,
    interpolated::{InterpolatedSensorControl, InterpolationMode, InterpolationData}
};

use std::{collections::HashMap, error::Error};

use log::{info, log_enabled, warn};
use serde_derive::{Deserialize, Serialize};

use crate::{
    controllers::FanController,
    groupie::SensorStorage,
    utils::SimpleError
};

fn f64_avg<I: IntoIterator<Item = f64>>(iterator: I) -> f64 {
    let mut sum: f64 = 0.0;
    let mut count: i64 = 0;
    for v in iterator {
        sum += v;
        count += 1;
    }
    // count is not an f64 initially as that may cause it to get stuck during incrementation
    // due to float imprecision in large numbers.
    // Rounding to the next float at the end ensures an accurate(ish) value
    sum / (count as f64)
}

fn f64_median<I: IntoIterator<Item = f64>>(iterator: I) -> Option<f64> {
    let mut values: Vec<f64> = iterator.into_iter().collect();
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    Some(if values.len() % 2 == 0 {
        (values[middle] + values[middle + 1]) / 2.0
    } else {
        values[middle + 1]
    })
}

const fn f64_1() -> f64 {
    1.0
}

pub trait PwmControl: std::fmt::Debug {
    fn evaluate(&self, state: &SensorStorage) -> Option<f64>;
}

#[derive(Clone)]
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SensorControl {
    Single(SingleSensorControl),
    Multi(MultiSensorControl),
    // Interpolated(InterpolatedSensorControl),
    Literal { value: f64 },
}
impl PwmControl for SensorControl {
    fn evaluate(&self, state: &SensorStorage) -> Option<f64> {
        match self {
            Self::Single(x) => x.evaluate(state),
            Self::Multi(x) => x.evaluate(state),
            // Self::Interpolated(x) => x.evaluate(state),
            Self::Literal { value } => Some(*value),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PwmCurve {
    #[serde(flatten)]
    interpolate_control: InterpolatedSensorControl
}

pub fn update_pwms<'a, C: AsMut<dyn FanController + 'a>>(
    state: &SensorStorage,
    pwms: &mut [C],
    curves: &HashMap<String, PwmCurve>,
    for_each_update: &mut impl FnMut((&str, f64, f64)),
) -> Result<(), Vec<Box<dyn Error>>> {
    let mut errors = vec![]; // don't want to set a capacity here. normally empty
    fn push_err<E: Error + 'static>(errors: &mut Vec<Box<dyn Error>>, e: E) {
        errors.push(Box::new(e));
    }

    for pwm in pwms.iter_mut().map(AsMut::as_mut) {
        let pwm_name = pwm.get_key();
        let curve = match curves.get(pwm_name) {
            None => continue,
            Some(x) => x,
        };
        let InterpolationData {
            input_value,
            value: target_pwm,
            low_index,
            high_index
        } = match curve.interpolate_control.evaluate_meta(state) {
            Some(x) => x,
            None => {
                push_err(&mut errors, SimpleError::from(format!(
                    "Error evaluating control for {pwm_name}. See the log for details.",
                )));
                continue;
            }
        };
        for_each_update((pwm_name, input_value, target_pwm));
        if log_enabled!(log::Level::Info) {
            info!(
                "{pwm_name}: {input_value:.1}°C (li {low_index:?}; hi {high_index:?}) -> {target_pwm}"
            );
        }
        if let Err(e) = pwm.write_value(target_pwm) {
            warn!("Failed to set pwm for {}: {}", pwm.get_key(), e);
            push_err(&mut errors, e);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
