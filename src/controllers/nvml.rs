#![cfg(feature = "nvml")]

use nvml_wrapper::{Device, Nvml, enums::device::FanControlPolicy, error::NvmlError};

use crate::controllers::FanControlError;

use super::FanController;

pub struct DeviceIterator<'a> {
    nvml: &'a Nvml,
    index: u32,
    device_count: u32
}
impl<'a> Iterator for DeviceIterator<'a> {
    type Item = Result<Device<'a>, NvmlError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.device_count {
            return None;
        }
        match self.nvml.device_by_index(self.index) {
            Err(NvmlError::InvalidArg) => None,
            x => {
                self.index += 1;
                Some(x)
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.device_count as usize, Some(self.device_count as usize))
    }
}

pub fn get_devices(nvml: &'_ Nvml) -> Result<DeviceIterator<'_>, NvmlError> {
    nvml.device_count().map(|device_count| {
        DeviceIterator { nvml, index: 0, device_count }
    })
}

// This is not a beautiful function, but it does allow us to "clone" a Device struct.
// Unfortunately it is fallible, which I assume is why it's not part of nvml-wrapper
fn clone_device<'nvml>(device: &Device<'nvml>) -> Result<Device<'nvml>, NvmlError> {
    device.nvml().device_by_index(device.index()?)
}

/// NVML-controlled GPU fans.
pub struct NVIDIAFanController<'nvml> {
    device: Device<'nvml>,
    fan_idx: u32,
    key: String
}
impl<'nvml> NVIDIAFanController<'nvml> {
    // Scans all NVML devices for their controllable fans
    pub fn scan(nvml: &'nvml Nvml) -> Result<Vec<Self>, NvmlError> {
        let device_iterator = get_devices(nvml)?;
        // most GPUs have at least two fans. (Breathtaking estimation, but better than nothing)
        let mut output = Vec::with_capacity(2 * device_iterator.size_hint().0);
        for dev in device_iterator {
            let device = dev?;
            let fan_count = match device.num_fans() {
                // Device has no fans, so yeet it
                Err(NvmlError::NotSupported) => continue,
                res => res?
            };
            for fan_idx in 0..fan_count {
                output.push(NVIDIAFanController {
                    device: clone_device(&device)?,
                    fan_idx,
                    key: format!("{}/{fan_idx}", device.uuid()?),
                })
            }
        }
        Ok(output)
    }
}
impl<'nvml> FanController for NVIDIAFanController<'nvml> {
    fn get_key(&self) -> &str {
        &self.key
    }

    fn read_value(&self) -> Result<f64, FanControlError> {
        self.device.fan_speed(self.fan_idx).map(Into::into)
            .map_err(Into::into)
    }
    fn write_value(&mut self, value: f64) -> Result<(), FanControlError> {
        self.device.set_fan_speed(self.fan_idx, value.round() as u32)
            .map_err(Into::into)
    }

    fn get_min_value(&self) -> f64 {
        self.get_min_max_value().0
    }
    fn get_max_value(&self) -> f64 {
        self.get_min_max_value().1
    }
    fn get_min_max_value(&self) -> (f64, f64) {
        // FIXME: This will crash if NVMLError::GpuLost occurs
        //  We should instead propagate the error further up
        let (min, max) = self.device.min_max_fan_speed()
            // GPU should have fans, because we check for that ahead of time.
            // (I don't think a GPU will just lose its fans at runtime)
            // Device may drop off the bus suddenly, but I don't think we can do anything about it?
            .expect("GPU can't give us its min-max speed. Help!");
        (min as f64, max as f64)
    }

    fn is_auto(&self) -> Result<bool, FanControlError> {
        self.device.fan_control_policy(self.fan_idx)
            .map(|policy| policy != FanControlPolicy::Manual)
            .map_err(Into::into)
    }
    fn set_auto(&mut self, auto: bool) -> Result<(), FanControlError> {
        self.device.set_fan_control_policy(
            self.fan_idx,
            if auto { FanControlPolicy::TemperatureContinousSw }
            else { FanControlPolicy::Manual }
        ).map_err(Into::into)
    }
}