use crate::groupie::{SensorKey, SensorStorage};
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, hash::Hash};

use super::{PwmControl, f64_1};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Derives a pwm value directly from a sensor, applying a conversion factor, if set.
/// This type is usually used within a [`super::MultiSensorControl`]
/// where multiple sensors can then be combined.
pub struct SingleSensorControl {
    pub adapter: String,
    pub sensor: String,
    #[serde(default = "f64_1")]
    pub factor: f64,
}
impl SensorKey for SingleSensorControl {
    fn get_sensor_key(&self) -> (&str, &str) {
        (&self.adapter, &self.sensor)
    }
}
impl Hash for SingleSensorControl {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.get_sensor_key().hash(state)
    }
}
impl PartialEq for SingleSensorControl {
    fn eq(&self, other: &Self) -> bool {
        self.cmp_by_sensor_key(other) == Ordering::Equal
    }
}
impl Eq for SingleSensorControl {}
#[typetag::serde(name = "single")]
impl PwmControl for SingleSensorControl {
    fn evaluate(&self, state: &SensorStorage) -> Option<f64> {
        state
            .get_sensor_data(self)
            .map(|sensor| self.factor * sensor.input)
    }
}
