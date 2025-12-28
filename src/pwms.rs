use std::{fmt::Debug, fs::{read_to_string, OpenOptions}, io::Write, path::{Path, PathBuf}, sync::LazyLock};

use log::warn;
use regex::Regex;

const HWMON_PATH: &str = "/sys/class/hwmon";
// const HWMON_PATTERN: &str = r"^hwmon[0-9]+$";
// const PWM_PATTERN: &str = r"^pwm[1-9][0-9]*$";
const HWMON_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^hwmon[0-9]+$").unwrap());
const PWM_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^pwm[1-9][0-9]*$").unwrap());

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct Pwm {
    base_path: PathBuf
}

impl Pwm {
    pub fn scan() -> Result<Vec<Pwm>, std::io::Error> {
        // TODO: Make this code not suck
        let mut pwms = vec![];
        for monitor_dir in Path::new(HWMON_PATH).read_dir()?.filter_map(|dir_result| {
            dir_result.inspect_err(|e| warn!("I/O Error while iterating over hwmons directory: {}", e))
                .ok()
        }) {
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

    fn get_name_raw<'a>(&'a self) -> &'a std::ffi::OsStr {
        self.base_path.file_name().unwrap()
    }

    pub fn get_name(self: &Pwm) -> &str {
        self.get_name_raw().to_str().unwrap()
    }

    pub fn get_name_string(&self) -> String {
        self.get_name().to_owned()
    }

    pub fn read_value(&self) -> Result<u8, Box<dyn std::error::Error>> {
        let buf = read_to_string(&self.base_path)?;
        Ok(buf.trim().parse()?)
    }

    pub fn is_auto(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let buf = read_to_string(self.special_file("enable"))?;
        Ok(buf.trim().parse::<u8>()? > 1)
    }

    pub fn set_auto(&self, auto: bool) -> Result<(), std::io::Error> {
        let mut file = OpenOptions::new()
            .write(true)
            .open(self.special_file("enable"))?;
        file.write_all(if auto { b"5" } else { b"1" })?;
        Ok(())
    }

    pub fn set_value(&self, value: u8) -> Result<(), std::io::Error> {
        let mut file = OpenOptions::new()
            .write(true)
            .open(&self.base_path)?;
        file.write_all(value.to_string().as_bytes())?;
        Ok(())
    }

    fn special_file(&self, extension: &str) -> PathBuf {
        let mut filename = self.base_path.file_name().unwrap().to_os_string();
        filename.push("_");
        filename.push(extension);
        self.base_path.parent().unwrap()
            .join(filename)
    }
}