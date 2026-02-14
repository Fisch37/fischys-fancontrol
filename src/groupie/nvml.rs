#![cfg(feature = "nvml")]

use std::collections::HashMap;
use std::mem::discriminant;
use std::rc::Rc;

use log::warn;
use nvml_wrapper::{Device, Nvml};

use crate::controllers::nvml::NvmlExtensions;
use nvml_wrapper::{enum_wrappers::device::TemperatureSensor, error::NvmlError};

use crate::eg_push_and_continue;
use crate::groupie::{
    Adapter, OwnedKey, PluginStorage, Sensor, SensorKey, SensorKind, SensorPlugin, SensorState,
};
use crate::utils::ErrorGroup;

// Result<u32> is a dirty hack, because we really don't care about the type
// and our calls (so far) happen to return u32
type NvmlQueryFn<'a> = &'a dyn Fn(&Device) -> Result<f64, NvmlError>;
type OwnedNvmlQueryFn = Box<dyn Fn(&Device) -> Result<f64, NvmlError>>;

const fn nvml_0(_: &Device) -> Result<f64, NvmlError> {
    Ok(0.0)
}

const fn nvml_inf(_: &Device) -> Result<f64, NvmlError> {
    Ok(f64::INFINITY)
}

fn nvml_core_temp(device: &Device) -> Result<f64, NvmlError> {
    device.temperature(TemperatureSensor::Gpu).map(Into::into)
}

const POWER_SCALER: f64 = 1e-3;
fn nvml_power_usage(device: &Device) -> Result<f64, NvmlError> {
    device.power_usage().map(|p| p as f64 * POWER_SCALER)
}

fn nvml_power_usage_max(device: &Device) -> Result<f64, NvmlError> {
    device
        .enforced_power_limit()
        .map(|p| p as f64 * POWER_SCALER)
}

pub struct NvmlPlugin<'nvml> {
    nvml: &'nvml Nvml,
    registered_sensors: Vec<NvmlSensorInfo>,
    used_devices: HashMap<String, Device<'nvml>>,
}
impl<'nvml> NvmlPlugin<'nvml> {
    pub fn new(nvml: &'nvml Nvml) -> Self {
        Self {
            nvml,
            registered_sensors: Vec::new(),
            used_devices: HashMap::new(),
        }
    }

    #[inline]
    fn add_sensor(
        &mut self,
        callbacks: SensorCallbacks,
        name: String,
        adapter: &Rc<Adapter>,
        kind: SensorKind,
        add_fn: &mut dyn FnMut(Sensor) -> Option<Sensor>,
    ) {
        let sensor = Sensor {
            adapter: adapter.clone(),
            name,
            kind,
        };
        let key: OwnedKey = sensor.get_sensor_key().into();
        add_fn(sensor);
        self.registered_sensors
            .push(NvmlSensorInfo { key, callbacks });
    }
}
impl<'nvml> SensorPlugin for NvmlPlugin<'nvml> {
    fn discover_sensors(
        &mut self,
        add_fn: &mut dyn FnMut(Sensor) -> Option<Sensor>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut error_group = ErrorGroup::new();

        let mut device_it = self.nvml.get_devices()?;
        while let Some(device) = error_group.push_until_ok(&mut device_it) {
            let uuid = eg_push_and_continue!(error_group, device.uuid());
            let adapter = eg_push_and_continue!(
                error_group,
                device.name().map(|name| {
                    Rc::new(Adapter {
                        key: uuid.clone(),
                        name,
                    })
                })
            );

            let fan_count = eg_push_and_continue!(
                error_group,
                device.num_fans().or_else(|e| match e {
                    NvmlError::NotSupported => Ok(0),
                    x => Err(x),
                })
            );
            for fan_idx in 0..fan_count {
                self.add_sensor(
                    OwnedSensorCallbacks {
                        input: Box::new(move |device: &Device<'_>| {
                            device.fan_speed_rpm(fan_idx).map(Into::into)
                        }),
                        // Boxing two static functions here is less than beautiful
                        // fixme?
                        min: Box::new(nvml_0),
                        max: Box::new(nvml_inf),
                    }
                    .into(),
                    format!("fan{fan_idx}"),
                    &adapter,
                    SensorKind::Fan,
                    add_fn,
                );
            }

            if is_supported(nvml_power_usage(&device)) {
                self.add_sensor(
                    BorrowedSensorCallbacks {
                        input: &nvml_power_usage,
                        max: &nvml_power_usage_max,
                        min: &nvml_0,
                    }
                    .into(),
                    "power".to_string(),
                    &adapter,
                    SensorKind::Power,
                    add_fn,
                );
            }
            if is_supported(nvml_core_temp(&device)) {
                self.add_sensor(
                    BorrowedSensorCallbacks {
                        input: &nvml_core_temp,
                        max: &nvml_inf,
                        min: &nvml_0,
                    }
                    .into(),
                    "gpu_temperature".to_string(),
                    &adapter,
                    SensorKind::Temperature,
                    add_fn,
                )
            }
            self.used_devices.insert(uuid, device);
        }

        Ok(())
    }

    fn update(&mut self, storage: &mut PluginStorage) -> Result<(), Box<dyn std::error::Error>> {
        let mut error_group = ErrorGroup::new();
        for sensor_info in self.registered_sensors.iter() {
            let device = match self.used_devices.get(&sensor_info.key.adapter) {
                Some(x) => x,
                None => {
                    warn!("Could not find device for sensor {}", sensor_info.key);
                    continue;
                }
            };
            let callbacks = sensor_info.callbacks.borrowed();
            let input = eg_push_and_continue!(error_group, (*callbacks.input)(device));
            let min = eg_push_and_continue!(error_group, (*callbacks.min)(device));
            let max = eg_push_and_continue!(error_group, (*callbacks.max)(device));

            match storage.put_state(&sensor_info.key, SensorState::new(input, min, max)) {
                Ok(_) => {}
                Err(_) => warn!(
                    "Could not find sensor {} in plugin storage, but it exists in NvmlPlugin registry",
                    &sensor_info.key
                ),
            }
        }
        Ok(error_group.ok()?)
    }
}

fn is_supported<T>(r: Result<T, NvmlError>) -> bool {
    !r.is_err_and(|e| discriminant(&e) == discriminant(&NvmlError::NotSupported))
}

struct NvmlSensorInfo {
    key: OwnedKey,
    callbacks: SensorCallbacks,
}
enum SensorCallbacks {
    Static(BorrowedSensorCallbacks<'static>),
    Dynamic(OwnedSensorCallbacks),
}
impl SensorCallbacks {
    pub fn borrowed(&self) -> BorrowedSensorCallbacks<'_> {
        match self {
            Self::Static(c) => c.clone(),
            Self::Dynamic(c) => c.into(),
        }
    }
}
impl From<BorrowedSensorCallbacks<'static>> for SensorCallbacks {
    fn from(value: BorrowedSensorCallbacks<'static>) -> Self {
        Self::Static(value)
    }
}
impl From<OwnedSensorCallbacks> for SensorCallbacks {
    fn from(value: OwnedSensorCallbacks) -> Self {
        Self::Dynamic(value)
    }
}

#[derive(Clone)]
struct BorrowedSensorCallbacks<'a> {
    // We're going to be in real trouble if Nvidia decides to make anything not u32
    input: NvmlQueryFn<'a>,
    max: NvmlQueryFn<'a>,
    min: NvmlQueryFn<'a>,
}

struct OwnedSensorCallbacks {
    input: OwnedNvmlQueryFn,
    max: OwnedNvmlQueryFn,
    min: OwnedNvmlQueryFn,
}
impl<'a> From<&'a OwnedSensorCallbacks> for BorrowedSensorCallbacks<'a> {
    fn from(value: &'a OwnedSensorCallbacks) -> Self {
        BorrowedSensorCallbacks {
            input: &value.input,
            max: &value.max,
            min: &value.min,
        }
    }
}
