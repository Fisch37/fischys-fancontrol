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
        self.operation.reduce(chosen_sensors)
    }
}

#[derive(Debug, Clone, Copy)]
#[derive(Serialize, Deserialize)]
pub enum SensorMergeOperation {
    MIN,
    MAX,
    AVERAGE,
    MEDIAN,
    SUM,
    // I doubt anybody actually wants this
    PRODUCT
}
impl SensorMergeOperation {
    pub fn reduce(self, values: impl IntoIterator<Item = f64>) -> Option<f64> {
        let values = values.into_iter();
        match self {
            Self::MIN => values.reduce(f64::min),
            Self::MAX => values.reduce(f64::max),
            Self::AVERAGE => Some(f64_avg(values)),
            Self::MEDIAN => f64_median(values),
            Self::SUM => Some(values.sum()),
            Self::PRODUCT => Some(values.product())
        }
    }
}
