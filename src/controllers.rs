pub mod pwm;
#[cfg(feature = "nvml")]
pub mod nvml;

use std::fmt::Display;

pub use pwm::Pwm;

use crate::GlobalContext;

pub type Result<T> = std::result::Result<T, FanControlError>;

pub trait FanController {
    /// Get the identifying key of this fan controller.
    /// This result must be unique to each logical controller on the system.
    /// This means that two FanController instances may share the same key,
    /// provided they point to the same controller hardware.
    fn get_key(&self) -> &str;

    /// Read the current setting of the FanController.
    fn read_value(&self) -> Result<f64>;
    /// Write a new setting to the FanController.
    /// Should fail if value is not within [`FanController::get_min_max_value`]
    fn write_value(&mut self, value: f64) -> Result<()>;
    
    /// Get the minimum value acceptable for this controller.
    fn get_min_value(&self) -> f64;
    /// Get the maximum value acceptable for this controller.
    fn get_max_value(&self) -> f64;
    /// Get the minimum _and_ maximum value acceptable for this controller.
    /// 
    /// Note that if you need both min and max, it is always better to call this function
    /// instead of [`FanController::get_min_value`] and [`FanController::get_max_value`] separately.
    fn get_min_max_value(&self) -> (f64, f64) {
        (self.get_min_value(), self.get_max_value())
    }

    /// Whether the program currently controls this fan controller.
    fn is_auto(&self) -> Result<bool>;
    /// Set whether the program currently controls this fan controller.
    /// A value of false usually means that the controller will run automatically,
    /// however it may be possible for some fan controllers to run in parallel.
    fn set_auto(&mut self, auto: bool) -> Result<()>;
}

#[derive(Debug)]
pub enum FanControlError {
    InvalidData,
    CommunicationLost,
    PermissionDenied,
    /// Some other unexpected error
    Unexpected(Box<dyn std::error::Error>)
}
impl Display for FanControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use self::FanControlError::*;
        match self {
            InvalidData => write!(f, "Invalid data was read or written to the controller"),
            CommunicationLost => write!(f, "Communication with the controller has been lost"),
            PermissionDenied => write!(f, "Permission denied"),
            Unexpected(e) => write!(f, "An unexpected error occured: {e}"),
        }
    }
}
impl std::error::Error for FanControlError { }
impl From<std::io::Error> for FanControlError {
    fn from(value: std::io::Error) -> Self {
        use self::FanControlError::*;
        use std::io::ErrorKind;
        match value.kind() {
            ErrorKind::InvalidData => InvalidData,
            ErrorKind::NotFound
            | ErrorKind::ConnectionAborted
            | ErrorKind::ConnectionReset
            | ErrorKind::HostUnreachable
            | ErrorKind::NetworkUnreachable
            | ErrorKind::NetworkDown
             => CommunicationLost,
            ErrorKind::PermissionDenied | ErrorKind::ConnectionRefused => PermissionDenied,
            _ => Unexpected(Box::new(value))
        }
    }
}
#[cfg(feature = "nvml")]
impl From<nvml_wrapper::error::NvmlError> for FanControlError {
    fn from(value: nvml_wrapper::error::NvmlError) -> Self {
        use self::FanControlError::*;
        use nvml_wrapper::error::NvmlError;
        match value {
            NvmlError::GpuLost => CommunicationLost,
            NvmlError::NoPermission => PermissionDenied,
            e => Unexpected(Box::new(e))
        }
    }
}

#[cfg(not(feature = "nvml"))]
#[allow(unused_variables)]
pub fn scan_all(context: &GlobalContext) -> Result<Vec<Box<dyn FanController>>> {
    Pwm::scan().map(|vec| vec.into_iter().map(Box::new).map(trait_coerce).collect())
        .map_err(Into::into)
}

#[cfg(feature = "nvml")]
pub fn scan_all<'a>(context: &'a GlobalContext) -> Result<Vec<Box<dyn FanController + 'a>>> {
    use nvml::NVIDIAFanController;

    let pwms = Pwm::scan()?;
    let nvidia_fans: Vec<NVIDIAFanController> = NVIDIAFanController::scan(context.get_nvml())?;
    Ok(
        pwms.into_iter().map(Box::new).map(trait_coerce)
            .chain(nvidia_fans.into_iter().map(Box::new).map(trait_coerce))
            .collect()
    )
}

fn trait_coerce<'b, T: FanController + 'b>(b: Box<T>) -> Box<dyn FanController + 'b> { b }