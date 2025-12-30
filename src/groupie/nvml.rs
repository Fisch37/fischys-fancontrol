use crate::{GlobalContext, groupie::QueryResult};

#[cfg(feature = "nvml")]
mod nvml_internal {
    pub use crate::controllers::nvml::get_devices;
}

#[cfg(feature = "nvml")]
pub fn query_sensors(state: &mut QueryResult, context: &GlobalContext) -> Result<(), Box<dyn std::error::Error>> {
    use core::f64;
    use std::rc::Rc;
    use log::warn;
    use crate::groupie::{Adapter, SensorData, SensorKind, SensorKey};

    let fans = state.get_of_kind_mut(SensorKind::Fan);
    for device in nvml_internal::get_devices(context.get_nvml())? {
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
            match fans.add(SensorData {
                kind: SensorKind::Fan,
                name: format!("fan{fan_idx}"),
                input: device.fan_speed_rpm(fan_idx)? as f64,
                min: 0.0,
                max: f64::INFINITY,
                adapter: adapter.clone()
            }) {
                Ok(_) => { },
                Err(sensor) => {
                    let key = sensor.get_sensor_key();
                    warn!("Tried to add sensor {}/{}, but it already exists. Skipping it.", key.0, key.1)
                }
            };
        }
    }
    Ok(())
}

#[cfg(not(feature = "nvml"))]
#[allow(unused)]
pub fn query_sensors(state: &mut QueryResult, context: &GlobalContext) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}