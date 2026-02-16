use crate::groupie::SensorStorage;
use serde::{Deserialize, Serialize};

use super::{PwmControl, SensorControl, f64_avg, f64_median};

#[derive(Debug, Serialize, Deserialize)]
/// Combines multiply sensor values into a single value via a [`SensorMergeOperation`].
///
/// **Note:** This currently ignores any [`None`] values from its child sensors.
/// This behaviour is intended to be changed to be short-circuiting
/// (returning [`None`] if any children returned [`None`]) instead.
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[allow(clippy::upper_case_acronyms)]
pub enum SensorMergeOperation {
    /// The minimum sensor value.
    ///
    /// Defaults to [`None`], if no values exist.
    MIN,
    /// The maximum sensor value.
    ///
    /// Defaults to [`None`], if no values exist.
    MAX,
    /// The average sensor value.
    ///
    /// Defaults to [`None`], if no values exist.
    AVERAGE,
    /// The median sensor value.
    /// Note that this method is relatively expensive,
    /// requiring us to collect and sort all values passed.
    ///
    /// Defaults to [`None`], if no values exist.
    MEDIAN,
    /// The sum of all sensor values.
    /// Defaults to the additive identity (`-0.0`) if no values exist.
    SUM,
    // I doubt anybody actually wants this
    /// The product of all sensor values.
    /// Defaults to the multiplicative identity (`1.0`) if no values exist.
    PRODUCT,
}
impl SensorMergeOperation {
    /// Applies this operation over an iterator of values.
    /// The exact return values depend on the variant and are described at said variants.
    ///
    /// Returns [`None`] if `values` is empty, except if an obvious default value exists.
    pub fn reduce(self, values: impl IntoIterator<Item = f64>) -> Option<f64> {
        let values = values.into_iter();
        match self {
            Self::MIN => values.reduce(f64::min),
            Self::MAX => values.reduce(f64::max),
            Self::AVERAGE => f64_avg(values),
            Self::MEDIAN => f64_median(values),
            Self::SUM => Some(values.sum()),
            Self::PRODUCT => Some(values.product()),
        }
    }
}
