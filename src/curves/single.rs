use std::hash::Hash;
use serde::{Deserialize, Serialize};
use crate::groupie::{SensorKey, SensorStorage};

use super::{PwmControl, f64_1};


#[derive(Debug, Clone)]
#[derive(Serialize, Deserialize)]
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
impl Eq for SingleSensorControl {}
impl PwmControl for SingleSensorControl {
    fn evaluate(&self, state: &SensorStorage) -> Option<f64> {
        state
            .get_sensor_data(self)
            .map(|sensor| self.factor * sensor.input)
    }
}
