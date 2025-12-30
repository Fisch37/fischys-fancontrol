pub mod pwm;
#[cfg(feature = "nvml")]
pub mod nvml;

pub use pwm::Pwm;

pub trait FanController {
    type WriteError;
    type ReadError;

    /// Get the identifying key of this fan controller.
    /// This result must be unique to each logical controller on the system.
    /// This means that two FanController instances may share the same key,
    /// provided they point to the same controller hardware.
    fn get_key(&self) -> &str;

    /// Read the current setting of the FanController.
    fn read_value(&self) -> Result<f64, Self::ReadError>;
    /// Write a new setting to the FanController.
    /// Should fail if value is not within [`FanController::get_min_max_value`]
    fn write_value(&mut self, value: f64) -> Result<(), Self::WriteError>;
    
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
    fn is_auto(&self) -> Result<bool, Self::ReadError>;
    /// Set whether the program currently controls this fan controller.
    /// A value of false usually means that the controller will run automatically,
    /// however it may be possible for some fan controllers to run in parallel.
    fn set_auto(&mut self, auto: bool) -> Result<(), Self::WriteError>;
}