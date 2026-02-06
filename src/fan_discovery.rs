use std::{iter::zip, thread::sleep, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{
    GlobalContext,
    controllers::scan_all,
    groupie::{OwnedKey, SensorData, SensorKey, SensorKind, SensorStorage},
};

#[derive(Serialize, Deserialize)]
pub struct Pwm2Fan {
    pub pwm: String,
    pub fans: Vec<OwnedKey>,
}

fn join_borrowed<'a, I: IntoIterator<Item = &'a str>>(vec: I, separator: char) -> String {
    let mut str = String::new();
    for e in vec {
        str += e;
        str.push(separator);
    }
    str
}

fn get_all_in<'a, K: SensorKey, I: Iterator<Item = K>>(
    it: I,
    state: &'a SensorStorage,
) -> impl Iterator<Item = SensorData<'a>> {
    it.filter_map(|s| state.get_sensor_data(s))
}

const SENSITIVITY: f64 = 300.0;
pub fn discover_pwm_fans(
    context: &GlobalContext,
) -> Result<Vec<Pwm2Fan>, Box<dyn std::error::Error>> {
    let mut state = SensorStorage::new(context);
    let mut controllers = scan_all(context)?;

    state.update()?;
    let fans: Vec<_> = state
        .iter_sensors()
        .filter(|s| s.kind == SensorKind::Fan)
        .cloned()
        .collect();

    println!(
        "Found {} fans: {}",
        fans.len(),
        join_borrowed(fans.iter().map(SensorKey::get_sensor_name), ' ')
    );
    println!(
        "Found {} pwms: {}",
        controllers.len(),
        join_borrowed(controllers.iter().map(|p| p.get_key()), ' ')
    );

    for p in &mut controllers {
        p.set_auto(false)?;
        p.write_value(p.get_max_value())?;
    }
    sleep(Duration::from_secs(5));
    println!("Spun up fans");
    state.update()?;
    for fan in get_all_in(fans.iter(), &state) {
        print!("{} {:.0} RPM ", fan.name, fan.input)
    }
    println!();

    let fan_speeds: Vec<f64> = get_all_in(fans.iter(), &state).map(|d| d.input).collect();
    let mut influence_list: Vec<Vec<usize>> = Vec::new();
    for p in &mut controllers {
        println!("Testing {}", p.get_key());
        p.write_value(p.get_min_value())?;
        sleep(Duration::from_secs(5));
        state.update()?;
        let mut affected_fans = vec![];
        //                                                             FIXME: This is vulnerable to state changes.
        //                                                              get_fan_states may return an it of different len, because of filter_map.
        //                                                              (Real world scenario: A fan doesn't receive any updates for a long time, so the old data gets discarded)
        for (i, (fan, original_speed)) in
            zip(get_all_in(fans.iter(), &state), &fan_speeds).enumerate()
        {
            // If fan RPM dropped more than SENSITIVITY
            if (original_speed - fan.input) > SENSITIVITY {
                affected_fans.push(i);
            }
        }

        let mut str = String::new();
        for i in &affected_fans {
            let (adapter_key, sensor_name) = &fans[*i].get_sensor_key();
            str += adapter_key;
            str += "/";
            str += sensor_name;
            str += " ";
        }
        println!("{} affects {}", p.get_key(), str);

        influence_list.push(affected_fans);
        p.write_value(p.get_max_value())?;
        sleep(Duration::from_secs(5));
    }

    for p in &mut controllers {
        p.set_auto(true)?;
    }

    let mut output = Vec::with_capacity(controllers.len());
    for (i, affected) in influence_list.iter().enumerate() {
        output.push(Pwm2Fan {
            pwm: controllers[i].get_key().to_string(),
            fans: affected.iter().map(|i| fans[*i].get_owned_key()).collect(),
        });
    }
    Ok(output)
}
