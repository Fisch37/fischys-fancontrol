use std::{collections::HashMap, error::Error, hash::Hash};

use log::{info, log_enabled, warn};
use serde_derive::{Deserialize, Serialize};

use crate::{controllers::FanController, groupie::{QueryResult, SensorKind}, utils::SimpleError};

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
    let middle = values.len()/2;
    Some(
        if values.len() % 2 == 0 {
            (values[middle] + values[middle+1]) / 2.0
        } else {
            values[middle + 1]
        }
    )
}

const fn f64_1() -> f64 { 1.0 }
const fn default_sensor_kind() -> SensorKind { SensorKind::Temperature }
const fn default_curve_mode() -> CurveMode { CurveMode::LinearInterpolation }

trait PwmControl : std::fmt::Debug {
    fn evaluate(&self, state: &QueryResult) -> Option<f64>;
}

#[derive(Serialize, Deserialize)]
#[derive(Debug)]
pub struct SingleSensorControl {
    pub adapter: String,
    pub sensor: String,
    #[serde(default = "f64_1")]
    pub factor: f64,
    #[serde(default = "default_sensor_kind")]
    pub sensor_kind: SensorKind
}
impl Hash for SingleSensorControl {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.adapter.hash(state);
        self.sensor.hash(state);
    }
}
impl PartialEq for SingleSensorControl {
    fn eq(&self, other: &Self) -> bool {
        // Not a perfect .eq method, but it works... enough
        self.adapter == other.adapter && self.sensor == other.sensor
    }
}
impl Eq for SingleSensorControl { }
impl PwmControl for SingleSensorControl {
    fn evaluate(&self, state: &QueryResult) -> Option<f64> {
        let sensors = state.get_of_kind(self.sensor_kind);
        sensors.get_from_parts(&self.adapter, &self.sensor)
            .map(|sensor| self.factor*sensor.input)
    }
}

#[derive(Serialize, Deserialize)]
#[derive(Debug)]
pub struct MultiSensorControl {
    pub sensors: Vec<SensorControl>,
    pub operation: SensorMergeOperation
}
#[derive(Serialize, Deserialize)]
#[derive(Debug)]
pub enum SensorMergeOperation {
    MIN, MAX, AVERAGE, MEDIAN
}
impl PwmControl for MultiSensorControl {
    fn evaluate(&self, state: &QueryResult) -> Option<f64> {
        let chosen_sensors = self.sensors.iter()
            // FIXME: Replace this with fail-fast semantics.
            //  (Should return None if any of the control.evaluate calls returned None)
            .filter_map(|control| control.evaluate(state));
        match self.operation {
            SensorMergeOperation::MIN => chosen_sensors.reduce(f64::min),
            SensorMergeOperation::MAX => chosen_sensors.reduce(f64::max),
            SensorMergeOperation::AVERAGE => Some(f64_avg(chosen_sensors)),
            SensorMergeOperation::MEDIAN => f64_median(chosen_sensors)
        }
    }
}

#[derive(Serialize, Deserialize)]
#[derive(Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SensorControl {
    Single(SingleSensorControl),
    Multi(MultiSensorControl),
    Literal { value: f64 }
}
impl PwmControl for SensorControl {
    fn evaluate(&self,state: &QueryResult) -> Option<f64>  {
        match self {
            Self::Single(x) => x.evaluate(state),
            Self::Multi(x) => x.evaluate(state),
            Self::Literal { value } => Some(*value)
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PwmCurve {
    input_sensors: SensorControl,
    #[serde(default = "default_curve_mode")]
    mode: CurveMode,
    points: Vec<(f64, f64)>,
    min: f64,
    max: f64
}
#[derive(Clone, Copy, Debug)]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CurveMode {
    LinearInterpolation,
    SnapLow,
    SnapHigh
}
impl CurveMode {
    pub fn interpolate(self, x: f64, low: (f64, f64), high: (f64, f64)) -> f64 {
        match self {
            Self::LinearInterpolation => low.1 + (x - low.0)*(high.1 - low.1)/(high.0 - low.0),
            Self::SnapLow => low.1,
            Self::SnapHigh => high.1
        }
    }
}

pub fn update_pwms<'a, U, C: AsMut<dyn FanController + 'a>>(
    state: &QueryResult,
    pwms: &mut [C],
    curves: &HashMap<String, PwmCurve>,
    for_each_update: &mut U
) -> Result<(), Vec<Box<dyn Error>>>
    where U: FnMut((&str, f64, f64))
{
    let mut errors = vec![]; // don't want to set a capacity here. normally empty
    fn push_err<E: Error + 'static>(errors: &mut Vec<Box<dyn Error>>, e: E) {
        errors.push(Box::new(e));
    }

    for pwm in pwms.iter_mut().map(AsMut::as_mut) {
        let pwm_name = pwm.get_key();
        let curve = match curves.get(pwm_name) {
            None => continue,
            Some(x) => x
        };
        let combined_temp = match curve.input_sensors.evaluate(state) {
            None => {
                warn!("Failed to get sensor data for pwm {}. Sensor not found", pwm_name);
                continue
            },
            Some(x) => x
        };
        // Find lower end interpolation point (or None if combined_temp < the lowest point)
        let low_index: Option<usize> = curve.points.iter().enumerate()
            .rfind(|(_, (point, _))| *point <= combined_temp)
            .map(|(i, _)| i);
        // for (i, (point, _)) in curve.points.iter().enumerate() {
        //     if combined_temp < *point {
        //         break;
        //     }
        //     low_index = Some(i);
        // }
        // If low_index.is_none(), then combined_temp < the lowest interpolation point => high_index = 0
        let high_index: Option<usize> = low_index.map_or(Some(0), |low_index| {
            if low_index < curve.points.len() - 1 {
                Some(low_index + 1)
            } else {
                None
            }
        });
        // let high_index: Option<usize> = match low_index {
        //     None => {
        //         // value < points[0]
        //         Some(0)
        //     },
        //     Some(low_index) => {
        //         if combined_temp >= curve.points[curve.points.len() - 1].0 {
        //             // value > points[-1]
        //             None
        //         } else {
        //             Some(low_index+1)
        //         }
        //     }
        // };
        let (pwm_min, pwm_max) = pwm.get_min_max_value();
        // TODO: This is syntactically bad and can be improved
        let pwm_value: f64 = f64::clamp(match low_index {
            None => curve.min,
            Some(low_index) => {
                match high_index {
                    None => curve.max,
                    Some(high_index) => {
                        let low = match curve.points.get(low_index) {
                            Some(i) => i,
                            None => {
                                push_err(&mut errors, SimpleError::new(format!("Out of bounds low index {low_index} for {pwm_name}")));
                                continue;
                            }
                        };
                        let high = match curve.points.get(high_index) {
                            Some(i) => i,
                            None => {
                                push_err(&mut errors, SimpleError::new(format!("Out of bounds high index {high_index} for {pwm_name}")));
                                continue;
                            }
                        };
                        curve.mode.interpolate(combined_temp, *low, *high)
                    }
                }
            }
        }, pwm_min, pwm_max);
        for_each_update((pwm_name, combined_temp, pwm_value));
        if log_enabled!(log::Level::Info) {
            info!("{}: {:.1}°C (li {:?}; hi {:?}) -> {}", pwm_name, combined_temp, low_index, high_index, pwm_value);
        }
        if let Err(e) = pwm.write_value(pwm_value) {
            warn!("Failed to set pwm for {}: {}", pwm.get_key(), e);
            push_err(&mut errors, e);
        }
    }

    if errors.is_empty() { Ok(()) }
    else { Err(errors) }
}