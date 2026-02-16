#![cfg(feature = "nvml")]

use nvml_wrapper::{Device, Nvml, error::NvmlError};

pub struct DeviceIterator<'a> {
    nvml: &'a Nvml,
    index: u32,
    device_count: u32,
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

pub trait NvmlExtensions {
    fn get_devices(&self) -> Result<DeviceIterator<'_>, NvmlError>;
}
impl NvmlExtensions for Nvml {
    fn get_devices(&self) -> Result<DeviceIterator<'_>, NvmlError> {
        self.device_count().map(|device_count| DeviceIterator {
            nvml: self,
            index: 0,
            device_count,
        })
    }
}