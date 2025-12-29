use std::{fmt::Display, iter::{repeat_with, zip}, thread::sleep, time::Duration};

use serde_derive::{Deserialize, Serialize};

use crate::{groupie::{QueryResult, SensorData, SensorKey, SensorKind, query_sensors}, pwms::Pwm};


#[derive(Serialize, Deserialize)]
pub struct FanProperties {
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
const ACTIVATION_BOUNDRY: f64 = 5.0;
const FAN_DETECTION_THRESHOLD: f64 = 0.7;

const FAN_SPEEDUP_DELAY: Duration = Duration::from_secs(5);
/// A step is basically a smaller jump in PWM values
const FAN_STEP_DELAY: Duration = Duration::from_secs(2);
const FAN_SLOWDOWN_DELAY: Duration = Duration::from_secs(10);

/// Determines the RPM curve of a fan depending on another PWM state.
/// This function assumes that the sensor pwm combo passed actually matches each other.
/// If this is not the case, the output will be nonsensical.
pub fn detect_fan_properties<Key: SensorKey>(pwm: &Pwm, rpm_sensors: &[Key]) -> Result<Vec<FanProperties>, Box<dyn std::error::Error>> {
    eprintln!("Graphing {}", pwm.get_name());
    let previous_responsibility_state = pwm.is_auto()?;

    let mut state = QueryResult::new();
    pwm.set_auto(false)?;
    
    let mut rpm_curves: Vec<Vec<Point>> = repeat_with(|| Vec::with_capacity((u8::MAX/PRECISION) as usize + 1))
        .take(rpm_sensors.len())
        .collect();
    
    let mut value: u8 = u8::MAX;
    pwm.set_value(value)?;
    // Fan needs to get up to speed
    sleep(FAN_SPEEDUP_DELAY);
    loop {
        pwm.set_value(value)?;
        sleep(FAN_STEP_DELAY);
        query_sensors(&mut state)?;
        
        let fans = state.get_of_kind(SensorKind::Fan);
        eprint!("{value} -> ");
        for (key, points) in zip(rpm_sensors, rpm_curves.iter_mut()) {
            let key_parts = key.get_sensor_key();
            let sensor = fans.get(key)
                .ok_or_else(|| Error { message: format!("Couldn't find sensor {}/{}", key_parts.0, key_parts.1) })?;
            points.push(Point { pwm: value, rpm: sensor.input });
            eprint!("{} ", sensor.input);
        }
        eprintln!();

        if value < PRECISION {
            break;
        } else {
            value -= PRECISION;
        }
    }


    let start_values = find_start_values(pwm, rpm_sensors, &mut state)?;
    eprint!("Start values: ");
    for start in &start_values {
        eprint!("{start} ");
    }
    eprintln!();

    pwm.set_auto(previous_responsibility_state)?;
    Ok(
        zip(rpm_sensors, zip(rpm_curves, start_values))
        .map(|(key, (points, start_value))| {
            let (adapter_key, sensor_name) = key.get_sensor_key();
            FanProperties {
                sensor: (adapter_key.to_string(), sensor_name.to_string()),
                rpm_curve: points,
                safe_start: start_value
            }
        })
        .collect()
    )
}

fn find_start_values<Key: SensorKey>(pwm: &Pwm, rpm_sensors: &[Key], state: &mut QueryResult) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    pwm.set_value(0)?;
    sleep(FAN_SLOWDOWN_DELAY - FAN_STEP_DELAY);
    // using u8::MAX here is fine, because the loop below breaks before u8::MAX is called
    let mut start_values = vec![u8::MAX; rpm_sensors.len()];
    let mut value = 0;
    // not checking u8::MAX here is fine since it would be the last value and is used as the placeholder above.
    // therefore, anything that would activate at u8::MAX, will have u8::MAX even though we never actually checked against it.
    // NOTE: there is a flaw here, in that if a fan were to never activate, it would also get u8::MAX, therefore that value is inherently unreliable.
    //  This is an acceptable tradeoff to me. If a fan hasn't started yet on 240, I doubt it will start on 255. (Most fans won't reach this value anyway)
    while start_values.iter().any(|x| *x == u8::MAX) && value < u8::MAX {
        pwm.set_value(value)?;
        sleep(FAN_STEP_DELAY);
        query_sensors(state)?;

        for (key, start_value) in zip(rpm_sensors, start_values.iter_mut())
            .filter(|(_, b)| **b == u8::MAX)
        {
            let sensor = state.get_of_kind(SensorKind::Fan).get(key)
                .ok_or_else(|| Error { message: format!("Sensor disappeared during fan-start analysis: {}/{}", key.get_adapter_key(), key.get_sensor_name()) })?;
            if sensor.input >= ACTIVATION_BOUNDRY {
                *start_value = value;
                eprintln!("Fan {}/{} started at value {value} ({} RPM)", key.get_adapter_key(), key.get_sensor_name(), sensor.input);
            }
        }

        value = value.saturating_add(PRECISION);
    }
    Ok(start_values)
}

/// Finds all rpm sensors tied to this PWM control.
/// At the end of this function the PWM will be set to manual mode!
pub fn find_controlled_fans(pwm: &Pwm) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let mut query_result = QueryResult::new();

    // Phase 1: Set PWM HIGH
    pwm.set_auto(false)?;
    pwm.set_value(u8::MAX)?;
    sleep(FAN_SPEEDUP_DELAY);
    query_sensors(&mut query_result)?;
    let phase1_states = query_result.get_of_kind(SensorKind::Fan).clone();

    // Phase 2: Set PWM low
    pwm.set_value(u8::MIN)?;
    sleep(FAN_SLOWDOWN_DELAY);
    query_sensors(&mut query_result)?;

    // Find sensors with notable rpm drop
    // it's rare for a PWM to target multiple fans
    let mut affected_sensors = Vec::with_capacity(1);
    for phase1 in phase1_states.unpack().into_iter() {
        match get_matching_sensor(&query_result, &phase1) {
            Err(e) => eprintln!("{:?} disappeared after first check! {e}", phase1.get_sensor_key()),
            Ok(phase2) => {
                if (phase1.input - phase2.input)/phase1.input > FAN_DETECTION_THRESHOLD {
                    affected_sensors.push((phase1.adapter.key.clone(), phase1.name));
                }
            }
        }
    }
    Ok(affected_sensors)
}

fn get_matching_sensor<'a>(state: &'a QueryResult, sensor: &SensorData) -> Result<&'a SensorData, Error> {
    match state.get_of_kind(sensor.kind).get_from_parts(&sensor.adapter.key, &sensor.name) {
        None => Err(Error::new(format!("Could not find a sensor {}/{}", sensor.adapter.key, sensor.name))),
        Some(x) => Ok(x)
    }
}