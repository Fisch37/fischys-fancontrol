use std::{error::Error, process::Command, rc::Rc};
use log::{debug, log_enabled, warn};

use crate::{GlobalContext, utils::{ExitStatusError, MalformedDataError}};

use super::{QueryResult, SensorData, Adapter, SensorKind};

fn malformed_data_error<T>(message: &str) -> Result<T, Box<dyn Error>> {
    Err(Box::new(MalformedDataError::new(message)))
}

/// Determines the kind of a sensor from its name (name is similar to "temp1", "temp12", or "beep")
pub fn from_lm_sensors_name(name: &str) -> Option<SensorKind> {
    let kind = name.trim_end_matches(|c: char| !c.is_alphabetic());
    if kind == "temp" {
        Some(SensorKind::Temperature)
    } else if kind == "fan" {
        Some(SensorKind::Fan)
    } else if kind == "in" {
        Some(SensorKind::Voltmeter)
    } else if kind == "beep" {
        Some(SensorKind::Beep)
    } else {
        None
    }
}

// context param is necessary for function pointer array in groupie.rs
pub fn query_sensors(state: &mut QueryResult, #[allow(unused)] context: &GlobalContext) -> Result<(), Box<dyn Error>> {
    let result = Command::new("sensors")
        .arg("-j")
        .output()?;
    if !result.status.success() {
        return Err(Box::new(ExitStatusError {
            code: result.status.code(),
            stderr: result.stderr
        }));
    }

    let root: serde_json::Map<String, serde_json::Value> = serde_json::from_slice(result.stdout.as_slice())?;
    for (adapter_key, adapter_value) in root {
        if log_enabled!(log::Level::Debug) {
            debug!("{:?} -> {:#?}", adapter_key, adapter_value);
        }
        let adapter_raw = match adapter_value.as_object() {
            None => return Err(Box::new(MalformedDataError::new_with_string(format!("Expected object for adapter, not {}", adapter_value)))),
            Some(x) => x
        };
        let adapter = Rc::new(Adapter {
            key: adapter_key,
            name: match adapter_raw.get("Adapter").and_then(|val| val.as_str()) {
                None => return Err(Box::new(MalformedDataError::new("Key value Adapter for Adapter not found or of incorrect type"))),
                Some(x) => x.to_string()
            }
        });
        if log_enabled!(log::Level::Debug) {
            debug!("Adapter {:#?}", adapter);
        }

        for (sensor_key, sensor_value) in adapter_raw {
            let sensor_raw = match sensor_value.as_object() {
                None => continue,
                Some(x) => x
            };
            let mut sensor = SensorData { 
                kind: SensorKind::Fan,  // placeholder
                name: sensor_key.to_string(),
                input: f64::NAN, min: f64::NEG_INFINITY, max: f64::INFINITY,
                adapter: Rc::clone(&adapter)
            };
            let mut kind: Option<SensorKind> = Option::None;
            for (key, value) in sensor_raw {
                let value_float = match value.as_f64() {
                    None => return Err(Box::new(MalformedDataError::new("Expected f64 in sensor value"))),
                    Some(x) => x
                };

                let mut key_parts = key.split('_');
                match key_parts.next() {
                    Some(x) => kind = kind.or(from_lm_sensors_name(x)),
                    None => return malformed_data_error("Empty sensor key")
                }
                match key_parts.next() {
                    Some(x) => {
                        match x {
                            "input" => sensor.input = value_float,
                            "min" => sensor.min = value_float,
                            "max" => sensor.max = value_float,
                            _ => { }
                        }
                    },
                    None => return malformed_data_error("Sensor key does not contain an _ (should be <key>_<dtype>)")
                }
            }
            match kind {
                None => warn!("Skipping unknown sensor of unknown kind {}/{}", sensor.adapter.key, sensor.name),
                Some(k) => {
                    sensor.kind = k;
                    state.add(sensor)
                        .map_err(|sensor| Box::new(MalformedDataError::new_with_string(format!("Duplicate sensor {}/{}", sensor.adapter.key, sensor.name))))?
                }
            }
        }
    }

    Ok(())
}