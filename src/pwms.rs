use std::{ffi::OsStr, fmt::Debug, fs::{OpenOptions, read_to_string}, io::Write, path::{Path, PathBuf}};

use lazy_static::lazy_static;
use log::warn;
use regex::Regex;

const HWMON_PATH: &str = "/sys/class/hwmon";
// const HWMON_PATTERN: &str = r"^hwmon[0-9]+$";
// const PWM_PATTERN: &str = r"^pwm[1-9][0-9]*$";
// const HWMON_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^hwmon[0-9]+$").unwrap());
// const PWM_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^pwm[1-9][0-9]*$").unwrap());
lazy_static! {
    static ref HWMON_PATTERN: Regex = Regex::new(r"^hwmon[0-9]+$").unwrap();
    static ref PWM_PATTERN: Regex = Regex::new(r"^pwm[1-9][0-9]*$").unwrap();
}

pub trait FanController {
    type WriteError;
    type ReadError;

    /// Get the identifying key of this fan controller.
    /// This result must be unique to each logical controller on the system.
    /// This means that two FanController instances may share the same key,
    /// provided they point to the same controller hardware.
    fn get_key(&self) -> &str;

    /// Read the current setting of the FanController.
    fn read_value(&self) -> Result<u8, Self::ReadError>;
    /// Write a new setting to the FanController.
    /// Should fail if value is not within [`FanController::get_min_max_value`]
    fn write_value(&self, value: u8) -> Result<(), Self::WriteError>;
    
    /// Get the minimum value acceptable for this controller.
    fn get_min_value(&self) -> f64;
    /// Get the maximum value acceptable for this controller.
    fn get_max_value(&self) -> f64;
    /// Get the minimum _and_ maximum value acceptable for this controller.
    /// The default implementation calls [`FanController::get_min_value`] and [`FanController::get_max_value`].
    /// Custom implementations must ensure that the return value will be identical to the default implementation.
    fn get_min_max_value(&self) -> (f64, f64) {
        (self.get_min_value(), self.get_max_value())
    }

    /// Whether the program currently controls this fan controller.
    fn is_auto(&self) -> Result<bool, Self::ReadError>;
    /// Set whether the program currently controls this fan controller.
    /// A value of false usually means that the controller will run automatically,
    /// however it may be possible for some fan controllers to run in parallel.
    fn set_auto(&self, auto: bool) -> Result<(), Self::WriteError>;
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct Pwm {
    base_path: PathBuf
}

impl Pwm {
    pub fn scan() -> Result<Vec<Pwm>, std::io::Error> {
        // TODO: Make this code not suck
        let mut pwms = vec![];
        for monitor_dir in Path::new(HWMON_PATH).read_dir()?
            // Skip unsuccessful read_dir results
            .filter_map(|dir_result| {
                dir_result.inspect_err(|e| warn!("I/O Error while iterating over hwmons directory: {}", e))
                    .ok()
            })
        {
            if !monitor_dir.file_name().to_str()
                .map_or(false, |filename| HWMON_PATTERN.is_match(filename))
            { continue; }
            let entry = match monitor_dir.path().read_dir() {
                Ok(x) => x,
                Err(e) => {
                    warn!("I/O Error while iterating over directory {:?}: {}", monitor_dir.path(), e);
                    continue;
                }
            };
            for file in entry.filter_map(|file_result| {
                file_result
                    .inspect_err(|e| warn!("I/O Error while iterating over hwmon {:?}: {}", monitor_dir.path(), e))
                    .ok()
            }) {
                if !file.file_name().to_str().map_or(false, |filename|
                    PWM_PATTERN.is_match(filename)) { continue; }
                pwms.push(Pwm { base_path: file.path() });
            }
        }
        pwms.sort();
        return Ok(pwms);
    }

    fn get_name_raw(&self) -> &OsStr {
        self.base_path.file_name().unwrap()
    }

    fn special_file(&self, extension: &str) -> PathBuf {
        let mut filename = self.base_path.file_name().unwrap().to_os_string();
        filename.push("_");
        filename.push(extension);
        self.base_path.parent().unwrap()
            .join(filename)
    }
}
impl FanController for Pwm {
    type ReadError = Box<dyn std::error::Error>;
    type WriteError = std::io::Error;


    fn get_key(&self) -> &str {
        self.get_name_raw().to_str().unwrap()
    }

    fn read_value(&self) -> Result<u8, Self::ReadError> {
        let buf = read_to_string(&self.base_path)?;
        Ok(buf.trim().parse()?)
    }
    fn write_value(&self, value: u8) -> Result<(), Self::WriteError> {
        let mut file = OpenOptions::new()
            .write(true)
            .open(&self.base_path)?;
        file.write_all(value.to_string().as_bytes())?;
        Ok(())
    }

    fn is_auto(&self) -> Result<bool, Self::ReadError> {
        let buf = read_to_string(self.special_file("enable"))?;
        Ok(buf.trim().parse::<u8>()? > 1)
    }
    fn set_auto(&self, auto: bool) -> Result<(), Self::WriteError> {
        let mut file = OpenOptions::new()
            .write(true)
            .open(self.special_file("enable"))?;
        file.write_all(if auto { b"5" } else { b"1" })?;
        Ok(())
    }
    
    fn get_min_value(&self) -> f64 {
        u8::MIN as f64
    }
    fn get_max_value(&self) -> f64 {
        u8::MAX as f64
    }
    
}