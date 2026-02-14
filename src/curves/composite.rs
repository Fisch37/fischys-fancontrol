use serde::{Deserialize, Serialize};
use crate::groupie::SensorStorage;

use super::{PwmControl, SensorControl, f64_avg, f64_median};

#[derive(Debug)]
#[derive(Serialize, Deserialize)]
pub struct MultiSensorControl {
    pub sensors: Vec<SensorControl>,
    pub operation: SensorMergeOperation,
}
#[typetag::serde(name = "multi")]
impl PwmControl for MultiSensorControl {
    fn evaluate(&self, state: &SensorStorage) -> Option<f64> {
        let chosen_sensors = self
            .sensors
            .iter()
            // FIXME: Replace this with fail-fast semantics.
            //  (Should return None if any of the control.evaluate calls returned None)
            .filter_map(|control| control.evaluate(state));
        match self.operation {
            SensorMergeOperation::MIN => chosen_sensors.reduce(f64::min),
            SensorMergeOperation::MAX => chosen_sensors.reduce(f64::max),
            SensorMergeOperation::AVERAGE => Some(f64_avg(chosen_sensors)),
            SensorMergeOperation::MEDIAN => f64_median(chosen_sensors),
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[derive(Serialize, Deserialize)]
pub enum SensorMergeOperation {
    MIN,
    MAX,
    AVERAGE,
    MEDIAN,
}
