#![cfg(feature = "nvml")]

use std::collections::HashMap;
use std::mem::discriminant;
use std::rc::Rc;

use log::warn;
use nvml_wrapper::{Device, Nvml};

use nvml_wrapper::{error::NvmlError, enum_wrappers::device::TemperatureSensor};
use crate::controllers::nvml::NvmlExtensions;


use crate::eg_push_and_continue;
use crate::groupie::{Adapter, OwnedKey, PluginStorage, Sensor, SensorKey, SensorKind, SensorPlugin, SensorState};
use crate::utils::ErrorGroup;

type NvmlQueryFn = &'static dyn Fn(&Device) -> Result<u32, NvmlError>;

fn nvml_always<const X: u32>(_: &Device) -> Result<u32, NvmlError> {
    Ok(X)
}

fn nvml_core_temp(device: &Device) -> Result<u32, NvmlError> {
    device.temperature(TemperatureSensor::Gpu)
}

fn nvml_power_usage(device: &Device) -> Result<u32, NvmlError> {
    device.power_usage()
}

fn nvml_power_usage_max(device: &Device) -> Result<u32, NvmlError> {
    device.enforced_power_limit()
}


pub struct NvmlPlugin<'nvml> {
    nvml: &'nvml Nvml,
    registered_sensors: Vec<SensorCallbacks>,
    used_devices: HashMap<String, Device<'nvml>>
}
impl<'nvml> NvmlPlugin<'nvml> {
    pub fn new(nvml: &'nvml Nvml) -> Self {
        Self { nvml, registered_sensors: Vec::new(), used_devices: HashMap::new() }
    }
}
impl<'nvml> SensorPlugin for NvmlPlugin<'nvml> {
    fn discover_sensors(&mut self, add_fn: fn(Sensor) -> Option<Sensor>) -> Result<(), Box<dyn std::error::Error>> {
        let mut error_group = ErrorGroup::new();
        
        let mut device_it = self.nvml.get_devices()?;
        while let Some(device) = error_group.push_until_ok(&mut device_it) {
            let uuid = eg_push_and_continue!(error_group, device.uuid());
            let adapter = eg_push_and_continue!(
                error_group,
                device.name()
                    .map(|name| {
                        Rc::new(Adapter {
                            key: uuid.clone(),
                            name: name
                        })
                    })
            );
            // Result<u32> is a dirty hack, because we really don't care about the type
            // and our calls (so far) happen to return u32
            let mut add_if_supported = |
                input_fn: NvmlQueryFn,
                max_fn: NvmlQueryFn,
                min_fn: NvmlQueryFn,
                name: &str,
                kind: SensorKind
            | {
                if is_supported(input_fn(&device)) {
                    let sensor = Sensor {
                        adapter: adapter.clone(),
                        name: name.to_string(),
                        kind
                    };
                    let key: OwnedKey = sensor.get_sensor_key().into();
                    add_fn(sensor);
                    self.registered_sensors.push(SensorCallbacks {
                        key,
                        input: input_fn,
                        max: max_fn,
                        min: min_fn
                    });
                }
            };

            let fan_count = eg_push_and_continue!(
                error_group,
                device.num_fans()
                    .or_else(|e| match e {
                        NvmlError::NotSupported => Ok(0),
                        x => Err(x)
                    })
            );
            for fan_idx in 0..fan_count {
                add_fn(Sensor {
                    adapter: adapter.clone(),
                    name: format!("fan{fan_idx}"),
                    kind: SensorKind::Fan
                });
            }

            add_if_supported(
                &nvml_power_usage,
                &nvml_power_usage_max,
                &nvml_always::<0>,
                "power",
                SensorKind::Power
            );
            add_if_supported(
                &nvml_core_temp,
                &nvml_always::<0>,
                &nvml_always::<{ u32::MAX }>,
                "gpu_temperature",
                SensorKind::Temperature
            );
            self.used_devices.insert(uuid, device);
        }

        Ok(())
    }

    fn update(&mut self, storage: &mut PluginStorage) -> Result<(), Box<dyn std::error::Error>> {
        let mut error_group = ErrorGroup::new();
        for callbacks in self.registered_sensors.iter() {
            let device = match self.used_devices.get(&callbacks.key.adapter) {
                Some(x) => x,
                None => {
                    warn!("Could not find device for sensor {}", callbacks.key);
                    continue;
                }
            };
            let input = eg_push_and_continue!(error_group, (*callbacks.input)(device));
            let min = eg_push_and_continue!(error_group, (*callbacks.min)(device));
            let max = eg_push_and_continue!(error_group, (*callbacks.max)(device));
            
            // Dirty hack to scale power ratings from milliwatts to watts
            let scaler = match storage.get_sensor(&callbacks.key) {
                Some(x) if x.kind == SensorKind::Power => 1e-3,
                _ => 1.0
            };
            storage.put_state(
                &callbacks.key,
                SensorState::new(
                    input as f64 * scaler,
                    min as f64 * scaler,
                    max as f64 * scaler
                )
            ).inspect_err(|_| {
                warn!("Could not find sensor {} in plugin storage, but it exists in NvmlPlugin registry", &callbacks.key)
            });
        }
        Ok(error_group.ok()?)
    }
}


fn is_supported<T>(r: Result<T, NvmlError>) -> bool {
    !r.is_err_and(|e| discriminant(&e) == discriminant(&NvmlError::NotSupported))
}

struct SensorCallbacks {
    key: OwnedKey,
    // We're going to be in real trouble if Nvidia decides to make anything not u32
    input: NvmlQueryFn,
    max: NvmlQueryFn,
    min: NvmlQueryFn
}