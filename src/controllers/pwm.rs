use std::{
    ffi::OsStr,
    fmt::Debug,
    fs::{OpenOptions, read_to_string},
    io::{Error as IOError, ErrorKind, Write},
    path::{Path, PathBuf},
};

use lazy_static::lazy_static;
use log::warn;
use regex::Regex;

use crate::controllers::FanControlError;

use super::FanController;

const HWMON_PATH: &str = "/sys/class/hwmon";
const PWM_IS_AUTO_THRESHOLD: u8 = 2;
lazy_static! {
    static ref HWMON_PATTERN: Regex = Regex::new(r"^hwmon[0-9]+$").unwrap();
    static ref PWM_PATTERN: Regex = Regex::new(r"^pwm[1-9][0-9]*$").unwrap();
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct Pwm {
    base_path: PathBuf,
}

impl Pwm {
    pub fn scan() -> Result<Vec<Pwm>, IOError> {
        // TODO: Make this code not suck
        // TODO: Make use of iterators to save on allocating a vec for this in in controllers::scan_all
        let mut pwms = vec![];
        for monitor_dir in Path::new(HWMON_PATH)
            .read_dir()?
            // Skip unsuccessful read_dir results
            .filter_map(|dir_result| {
                dir_result
                    .inspect_err(|e| {
                        warn!("I/O Error while iterating over hwmons directory: {}", e)
                    })
                    .ok()
            })
        {
            if !monitor_dir
                .file_name()
                .to_str()
                .is_some_and(|filename| HWMON_PATTERN.is_match(filename))
            {
                continue;
            }
            let entry = match monitor_dir.path().read_dir() {
                Ok(x) => x,
                Err(e) => {
                    warn!(
                        "I/O Error while iterating over directory {:?}: {}",
                        monitor_dir.path(),
                        e
                    );
                    continue;
                }
            };
            for file in entry.filter_map(|file_result| {
                file_result
                    .inspect_err(|e| {
                        warn!(
                            "I/O Error while iterating over hwmon {:?}: {}",
                            monitor_dir.path(),
                            e
                        )
                    })
                    .ok()
            }) {
                if !file
                    .file_name()
                    .to_str()
                    .is_some_and(|filename| PWM_PATTERN.is_match(filename))
                {
                    continue;
                }
                pwms.push(Pwm {
                    base_path: file.path(),
                });
            }
        }
        pwms.sort();
        Ok(pwms)
    }

    fn get_name_raw(&self) -> &OsStr {
        self.base_path.file_name().unwrap()
    }

    fn special_file(&self, extension: &str) -> PathBuf {
        let mut filename = self.base_path.file_name().unwrap().to_os_string();
        filename.push("_");
        filename.push(extension);
        self.base_path.parent().unwrap().join(filename)
    }
}
impl FanController for Pwm {
    fn get_key(&self) -> &str {
        self.get_name_raw().to_str().unwrap()
    }

    fn read_value(&self) -> Result<f64, FanControlError> {
        let buf = read_to_string(&self.base_path)?;
        buf.trim()
            .parse()
            // ParseFloatError has exactly two error types: empty and invalid.
            // Both are invalid data.
            .map_err(|_| ErrorKind::InvalidData.into())
            .map_err(|e: IOError| e.into())
    }
    fn write_value(&mut self, value: f64) -> Result<(), FanControlError> {
        let mut file = OpenOptions::new().write(true).open(&self.base_path)?;
        let byte_out = value.clamp(u8::MIN as f64, u8::MAX as f64).round() as u8;
        file.write_all(byte_out.to_string().as_bytes())?;
        Ok(())
    }

    fn is_auto(&self) -> Result<bool, FanControlError> {
        let buf = read_to_string(self.special_file("enable"))?;
        buf.trim()
            .parse::<u8>()
            .map(|x| x >= PWM_IS_AUTO_THRESHOLD)
            .map_err(|_| ErrorKind::InvalidData.into())
            .map_err(|e: IOError| e.into())
    }
    fn set_auto(&mut self, auto: bool) -> Result<(), FanControlError> {
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
