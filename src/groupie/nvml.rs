#[cfg(feature = "nvml")]
use std::rc::Rc;

use log::warn;
#[cfg(feature = "nvml")]
use nvml_wrapper::{error::NvmlError, enum_wrappers::device::TemperatureSensor};

#[cfg(feature = "nvml")]
use crate::groupie::{Adapter, SensorKind};
use crate::{GlobalContext, groupie::{QueryResult, SensorData, SensorKey as _}};

#[cfg(feature = "nvml")]
mod nvml_internal {
    pub use crate::controllers::nvml::get_devices;
}

#[inline] #[allow(unused)]  // unused if nvml is disabled
fn post_dup_warn(sensor: SensorData) {
    let key = sensor.get_sensor_key();
    warn!("Tried to add sensor {}/{}, but it already exists. Skipping it.", key.0, key.1)
}

#[cfg(feature = "nvml")]
#[inline]
pub fn add_optional_sensor<Name: ToString + ?Sized>(
    state: &mut QueryResult,
    sensor_name: &Name,
    sensor_kind: SensorKind,
    adapter: &Rc<Adapter>,
    sensor_value: Result<f64, NvmlError>,
    max: f64
) -> Result<(), NvmlError> {
    match sensor_value {
        Ok(x) => {
            if let Err(sensor) = state.add(SensorData {
                kind: sensor_kind,
                name: sensor_name.to_string(),
                input: x,
                min: 0.0,
                max: max,
                adapter: adapter.clone()
            }) {
                post_dup_warn(sensor);
            }
            Ok(())
        },
        Err(NvmlError::NotSupported) => Ok(()),
        Err(e) => Err(e)
    }
}

#[cfg(feature = "nvml")]
pub fn query_sensors(state: &mut QueryResult, context: &GlobalContext) -> Result<(), Box<dyn std::error::Error>> {
    use std::rc::Rc;
    use crate::groupie::{Adapter, SensorData, SensorKind};

    for device in nvml_internal::get_devices(context.get_nvml())? {
        use core::f64;

        use nvml_wrapper::error::NvmlError;

        let device = device?;
        let adapter = Rc::new(Adapter {
            key: device.uuid()?,
            name: device.name()?
        });
        let fan_count = match device.num_fans() {
            Err(NvmlError::NotSupported) => 0,
            x => x?
        };
        for fan_idx in 0..fan_count {
            if let Err(sensor) = state.add(SensorData {
                kind: SensorKind::Fan,
                name: format!("fan{fan_idx}"),
                input: device.fan_speed_rpm(fan_idx)? as f64,
                min: 0.0,
                max: f64::INFINITY,
                adapter: adapter.clone()
            }) {
                post_dup_warn(sensor);
            };
        }

        add_optional_sensor(
            state,
            "power",
            SensorKind::Power,
            &adapter,
            // NVML gives power in milliwatts, but we use watts
            device.power_usage().map(|x| (x as f64)/1_000.0),
            device.enforced_power_limit().map(|x| (x as f64)/1_000.0)
                .inspect_err(|e| warn!("Failed to fetch power limit for {}: {e}", adapter.name))
                .unwrap_or(f64::INFINITY)
        )?;
        add_optional_sensor(
            state,
            "gpu_temperature",
            SensorKind::Temperature,
            &adapter,
            device.temperature(TemperatureSensor::Gpu).map(Into::into),
            f64::INFINITY
        )?;
    }
    Ok(())
}

#[cfg(not(feature = "nvml"))]
#[allow(unused)]  // suppress unused parameter messages
pub fn query_sensors(state: &mut QueryResult, context: &GlobalContext) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}