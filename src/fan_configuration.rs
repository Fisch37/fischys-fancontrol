use std::{fmt::Display, thread::sleep, time::Duration};

use serde_derive::{Deserialize, Serialize};

use crate::{groupie::{query_sensors, QueryResult, SensorData}, pwms::Pwm};


#[derive(Serialize, Deserialize)]
pub struct FanProperties {
    pub pwm: String,
    pub sensor: (String, String),
    pub rpm_curve: Vec<Point>,
    pub safe_start: u8
}

#[derive(Serialize, Deserialize)]
pub struct Point {
    pwm: u8,
    rpm: f64
}

#[derive(Debug)]
struct Error {
    message: String
}
impl Error {
    pub fn new(message: String) -> Error {
        Error { message: message }
    }
}
impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Error(FanConfiguration: {})", self.message)
    }
}
impl std::error::Error for Error { }

const PRECISION: u8 = 15;
const ACTIVATION_BOUNDRY: f64 = 0.0;
pub fn detect_fan_properties(sensor: &SensorData, pwm: &Pwm) -> Result<FanProperties, Box<dyn std::error::Error>> {
    println!("Graphing {}", pwm.get_name());
    let previous_responsibility_state = pwm.is_auto()?;

    let mut state = QueryResult::new();
    pwm.set_auto(false)?;
    let mut value: u8 = u8::MAX;
    // This will use 1 more slot than necessary if PRECISION is one of 3,5,15,17,51,85 or 255
    // The memory waste seriously isn't that big
    let mut rpm_curve = Vec::with_capacity((value/PRECISION) as usize + 1);
    pwm.set_value(value)?; // Fan needs to get up to speed
    sleep(Duration::from_secs(5));
    loop {
        pwm.set_value(value)?;
        sleep(Duration::from_secs(3));
        query_sensors(&mut state)?;
        let current_sensor_data = get_matching_sensor(&state, sensor)?;
        rpm_curve.push(Point { pwm: value, rpm: current_sensor_data.input });
        println!("{} -> {}", value, current_sensor_data.input);

        if value < PRECISION {
            break;
        } else {
            value -= PRECISION;
        }
    }

    pwm.set_value(0)?;
    sleep(Duration::from_secs(10));
    query_sensors(&mut state)?;
    let mut value = 0;
    while get_matching_sensor(&state, sensor)?.input <= ACTIVATION_BOUNDRY && value <= u8::MAX - PRECISION {
        pwm.set_value(value)?;
        sleep(Duration::from_secs(2));
        query_sensors(&mut state)?;

        value += PRECISION;
    }
    if value > u8::MAX - PRECISION {
        value = u8::MAX;
    }
    println!("Start value {}", value);

    pwm.set_auto(previous_responsibility_state)?;
    Ok(FanProperties {
        pwm: pwm.get_name().to_owned(),
        sensor: (sensor.adapter.key.clone(), sensor.name.clone()),
        rpm_curve,
        safe_start: value
    })
}

fn get_matching_sensor<'a>(state: &'a QueryResult, sensor: &SensorData) -> Result<&'a SensorData, Error> {
    match state.get_of_kind(sensor.kind).get(&sensor.adapter.key, &sensor.name) {
        None => Err(Error::new(format!("Could not find a sensor {}/{}", sensor.adapter.key, sensor.name))),
        Some(x) => Ok(x)
    }
}