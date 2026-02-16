use super::{PwmControl, SensorControl};
use crate::groupie::SensorStorage;
use log::error;
use serde::{Deserialize, Serialize};
use tap::prelude::*;

#[doc(hidden)]
const fn default_interpolation_mode() -> InterpolationMode {
    InterpolationMode::LinearInterpolation
}

/// Metadata about the interpolation process.
pub struct InterpolationData {
    pub input_value: f64,
    /// The resulting PWM-value
    pub value: f64,
    /// The index of the highest value less than input_value
    /// (or None if input_value is less than the first point.0)
    pub low_index: Option<usize>,
    /// The index of lowest value greater than input_value
    /// (or None if input_value is greater than the last point.0)
    pub high_index: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
/// Interpolates between points on a spline using the contained sensor as the x-value.
/// See [`InterpolationMode`] for different options for interpolating.
///
/// Also requires a `min` and `max` value for when the sensor value goes outside the bounds
/// of the curve.
///
/// **Note:** `points` is assumed to be sorted. Weirdness will happen if it is not.
pub struct InterpolatedSensorControl {
    input: SensorControl,
    #[serde(default = "default_interpolation_mode")]
    mode: InterpolationMode,
    min: f64,
    max: f64,
    points: Vec<(f64, f64)>,
}
impl InterpolatedSensorControl {
    /// Helper function for [`Self::evaluate`].
    /// Interpolates the given `input_value` between the `low_index` and `high_index`.
    ///
    /// This function assumes that `low_index` and `high_index` are the correct indices
    /// and the result will likely be nonsensical if this is not the case.
    ///
    /// Returns None, if the interpolation is impossible (usually due to some error).
    /// If `low_index` is [`None`], returns `-f64::INFINITY`;
    /// if `high_index` is [`None`], likewise returns `f64::INFINITY`.
    fn interpolate_between_indices(
        &self,
        low_index: Option<usize>,
        high_index: Option<usize>,
        input_value: f64,
    ) -> Option<f64> {
        let low_index = match low_index {
            Some(x) => x,
            None => return Some(-f64::INFINITY),
        };
        let high_index = match high_index {
            Some(x) => x,
            None => return Some(f64::INFINITY),
        };

        let low = self
            .points
            .get(low_index)
            .tap_none(|| error!("Out of bounds low index {low_index} on {self:?}"))?;
        let high = self
            .points
            .get(high_index)
            .tap_none(|| error!("Out of bounds high index {high_index} on {self:?}"))?;
        Some(self.mode.interpolate(input_value, *low, *high))
    }

    /// Like [`Self::evaluate`], except also returns some metadata about the interpolation process,
    /// if Some is returned.
    pub fn evaluate_meta(&self, state: &SensorStorage) -> Option<InterpolationData> {
        let input_value = self.input.evaluate(state)?;
        // Find lower end interpolation point (or None if combined_temp < the lowest point)
        let low_index: Option<usize> = self
            .points
            .iter()
            .enumerate()
            // Find the last point (in the list) whose x value is <= the input value
            .rfind(|(_, (point, _))| *point <= input_value)
            .map(|(i, _)| i);
        // If low_index.is_none(), then combined_temp < the lowest interpolation point
        // => high_index = 0
        let high_index: Option<usize> = low_index.map_or(Some(0), |low_index| {
            if low_index < self.points.len() - 1 {
                Some(low_index + 1)
            } else {
                None
            }
        });
        self.interpolate_between_indices(low_index, high_index, input_value)
            .map(|x| InterpolationData {
                input_value,
                value: x,
                low_index,
                high_index,
            })
    }
}
#[typetag::serde(name = "interpolated")]
impl PwmControl for InterpolatedSensorControl {
    fn evaluate(&self, state: &SensorStorage) -> Option<f64> {
        self.evaluate_meta(state).map(|data| data.value)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// How to interpolate between values.
pub enum InterpolationMode {
    /// Interpolate linearly along subsequent pairs of points.
    /// This is the type of interpolation you will typically see in control programs.
    LinearInterpolation,
    /// Snap to the highest value lower than the sensor input.
    SnapLow,
    /// Snap to the lowest value higher than the sensor input.
    SnapHigh,
}
impl InterpolationMode {
    pub fn interpolate(self, x: f64, low: (f64, f64), high: (f64, f64)) -> f64 {
        match self {
            Self::LinearInterpolation => low.1 + (x - low.0) * (high.1 - low.1) / (high.0 - low.0),
            Self::SnapLow => low.1,
            Self::SnapHigh => high.1,
        }
    }
}
