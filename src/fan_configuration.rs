use std::{
    iter::{repeat_with, zip},
    thread::sleep,
    time::Duration,
};

use serde_derive::{Deserialize, Serialize};

use crate::{
    GlobalContext,
    controllers::{FanController, Pwm},
    groupie::{OwnedKey, SensorKey, SensorKind, SensorStorage},
    utils::SimpleError,
};

#[derive(Serialize, Deserialize)]
pub struct FanProperties {
    pub sensor: (String, String),
    pub rpm_curve: Vec<Point>,
    pub safe_start: f64,
}

#[derive(Serialize, Deserialize)]
pub struct Point {
    pwm: f64,
    rpm: f64,
}

const PRECISION: f64 = 15.0;
const ACTIVATION_BOUNDRY: f64 = 5.0;
const FAN_DETECTION_THRESHOLD: f64 = 0.7;

const FAN_SPEEDUP_DELAY: Duration = Duration::from_secs(5);
/// A step is basically a smaller jump in PWM values
const FAN_STEP_DELAY: Duration = Duration::from_secs(2);
const FAN_SLOWDOWN_DELAY: Duration = Duration::from_secs(10);

/// Determines the RPM curve of a fan depending on another PWM state.
/// This function assumes that the sensor pwm combo passed actually matches each other.
/// If this is not the case, the output will be nonsensical.
pub fn detect_fan_properties<K: SensorKey>(
    pwm: &mut dyn FanController,
    rpm_sensors: &[K],
    context: &GlobalContext,
) -> Result<Vec<FanProperties>, Box<dyn std::error::Error>> {
    eprintln!("Graphing {}", pwm.get_key());
    let previous_responsibility_state = pwm.is_auto()?;

    let mut state = SensorStorage::new(context);
    pwm.set_auto(false)?;

    let mut rpm_curves: Vec<Vec<Point>> =
        repeat_with(|| Vec::with_capacity((pwm.get_max_value() / PRECISION) as usize + 1))
            .take(rpm_sensors.len())
            .collect();

    let mut value: f64 = pwm.get_max_value();
    pwm.write_value(value)?;
    // Fan needs to get up to speed
    sleep(FAN_SPEEDUP_DELAY);
    loop {
        pwm.write_value(value)?;
        sleep(FAN_STEP_DELAY);
        state.update()?;

        eprint!("{value} -> ");
        for (key, points) in zip(rpm_sensors, rpm_curves.iter_mut()) {
            let key_parts = key.get_sensor_key();
            let sensor = state.get_sensor_data(key).ok_or_else(|| {
                SimpleError::new(format!(
                    "Couldn't find sensor {}/{}",
                    key_parts.0, key_parts.1
                ))
            })?;
            points.push(Point {
                pwm: value,
                rpm: sensor.input,
            });
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
    Ok(zip(rpm_sensors, zip(rpm_curves, start_values))
        .map(|(key, (points, start_value))| {
            let (adapter_key, sensor_name) = key.get_sensor_key();
            FanProperties {
                sensor: (adapter_key.to_string(), sensor_name.to_string()),
                rpm_curve: points,
                safe_start: start_value,
            }
        })
        .collect())
}

fn find_start_values<Key: SensorKey>(
    pwm: &mut dyn FanController,
    rpm_sensors: &[Key],
    state: &mut SensorStorage,
) -> Result<Vec<f64>, Box<dyn std::error::Error>> {
    let pwm_max = pwm.get_max_value();
    pwm.write_value(pwm.get_min_value())?;
    sleep(FAN_SLOWDOWN_DELAY - FAN_STEP_DELAY);
    // using u8::MAX here is fine, because the loop below breaks before u8::MAX is called
    let mut start_values = vec![pwm_max; rpm_sensors.len()];
    let mut value = pwm.get_min_value();
    // not checking u8::MAX here is fine since it would be the last value and is used as the placeholder above.
    // therefore, anything that would activate at u8::MAX, will have u8::MAX even though we never actually checked against it.
    // NOTE: there is a flaw here, in that if a fan were to never activate, it would also get u8::MAX, therefore that value is inherently unreliable.
    //  This is an acceptable tradeoff to me. If a fan hasn't started yet on 240, I doubt it will start on 255. (Most fans won't reach this value anyway)
    while start_values.contains(&pwm_max) && value < pwm_max {
        pwm.write_value(value)?;
        sleep(FAN_STEP_DELAY);
        state.update()?;

        for (key, start_value) in
            zip(rpm_sensors, start_values.iter_mut()).filter(|(_, b)| **b == pwm_max)
        {
            let sensor = state.get_sensor_data(key).ok_or_else(|| {
                SimpleError::new(format!(
                    "Sensor disappeared during fan-start analysis: {}/{}",
                    key.get_adapter_key(),
                    key.get_sensor_name()
                ))
            })?;
            if sensor.input >= ACTIVATION_BOUNDRY {
                *start_value = value;
                eprintln!(
                    "Fan {}/{} started at value {value} ({} RPM)",
                    key.get_adapter_key(),
                    key.get_sensor_name(),
                    sensor.input
                );
            }
        }
        // Naive adding is fine, because inf !< inf in Rust, so the loop will still exit
        value += PRECISION;
    }
    Ok(start_values)
}

/// Finds all rpm sensors tied to this PWM control.
/// At the end of this function the PWM will be set to manual mode!
pub fn find_controlled_fans(
    pwm: &mut Pwm,
    context: &GlobalContext,
) -> Result<Vec<OwnedKey>, Box<dyn std::error::Error>> {
    let mut state = SensorStorage::new(context);

    // Phase 1: Set PWM HIGH
    pwm.set_auto(false)?;
    pwm.write_value(pwm.get_max_value())?;
    sleep(FAN_SPEEDUP_DELAY);
    state.update()?;
    let phase1_states: Vec<_> = state
        .iter_data()
        .filter(|d| d.kind == SensorKind::Fan)
        // Hack to circumvent simultaneous borrow issue
        // SensorData borrows Sensor.name (to avoid continuous allocations),
        // but that means SensorData also can't be stored across state.update boundaries
        // (because update borrows state mutably, and SensorData borrows it immutably).
        // Fixing this is complicated.
        .map(|d| (d.get_owned_key(), d.input))
        .collect();

    // Phase 2: Set PWM low
    pwm.write_value(pwm.get_min_value())?;
    sleep(FAN_SLOWDOWN_DELAY);
    state.update()?;

    // Find sensors with notable rpm drop
    // capacity=1: it's rare for a PWM to target multiple fans
    let mut affected_sensors = Vec::with_capacity(1);
    for phase1 in phase1_states.into_iter() {
        match state.get_sensor_data(&phase1.0) {
            None => eprintln!("{} disappeared after first check!", phase1.0.display()),
            Some(phase2) => {
                if (phase1.1 - phase2.input) / phase1.1 > FAN_DETECTION_THRESHOLD {
                    affected_sensors.push(phase1.0);
                }
            }
        }
    }
    Ok(affected_sensors)
}
