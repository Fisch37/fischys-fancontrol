use core::f64;
use std::{process::Command, rc::Rc};

use log::warn;

use crate::utils::{ErrorGroup, ExitStatusError, SimpleError};
use super::{QueryResult, SensorData, SensorKind, Adapter};

pub fn query_sensors(state: &mut QueryResult) -> Result<(), Box<dyn std::error::Error>> {
    let result = Command::new("nvidia-smi")
        .arg("--query-gpu=uuid,name,temperature.gpu,temperature.gpu.tlimit,temperature.memory")
        .arg("--format=csv")
        .output()?;
    if !result.status.success() {
        return Err(Box::new(ExitStatusError {
            code: result.status.code(),
            stderr: result.stderr
        }))
    }
    let mut reader = csv::Reader::from_reader(result.stdout.as_slice());
    let temperature_sensors = state.get_of_kind_mut(super::SensorKind::Temperature);
    let mut error = ErrorGroup::new();
    for result in reader.records() {
        match result {
            Err(e) => error.push(e),
            Ok(record) => {
                if record.len() < 5 {
                    error.push(SimpleError::new(format!("Unexpected record length. Expected 5, found {}", record.len())));
                    continue;
                }
                let adapter = Rc::new(Adapter {
                    key: record[0].trim().to_string(),
                    name: record[1].trim().to_string()
                });
                let core_tlimit = match record[3].trim().parse() {
                    Err(_) => f64::INFINITY,
                    Ok(x) => x
                };
                match record[2].trim().parse() {
                    Err(_) => { },
                    Ok(core_temperature) => {
                        match temperature_sensors.add(SensorData {
                            kind: SensorKind::Temperature,
                            name: "core_temperature".to_string(),
                            adapter: adapter.clone(),
                            input: core_temperature,
                            min: f64::NEG_INFINITY,
                            max: core_tlimit
                        }) {
                            Ok(_) => { },
                            Err(_) => warn!("Tried to add duplicate sensor core_temperature for {:?}", adapter)
                        }
                    }
                };
                match record[4].trim().parse() {
                    Err(_) => { },
                    Ok(memory_temperature) => {
                        match temperature_sensors.add(SensorData {
                            kind: SensorKind::Temperature,
                            name: "memory_temperature".to_string(),
                            adapter: adapter.clone(),
                            input: memory_temperature,
                            min: f64::NEG_INFINITY,
                            max: f64::INFINITY
                        }) {
                            Ok(_) => { },
                            Err(_) => warn!("Tried to add duplicate sensor memory_temperature for {:?}", adapter)
                        }
                    }
                }
            }
        }
    }
    
    if error.is_empty() { Ok(()) }
    else { Err(Box::new(error)) }
}