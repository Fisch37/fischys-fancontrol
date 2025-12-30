use std::{iter::zip, thread::sleep, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{groupie::{QueryResult, SensorKind, query_sensors}, pwms::{FanController as _, Pwm}};

#[derive(Serialize, Deserialize)]
pub struct Pwm2Fan {
    pub pwm: String,
    pub fans: Vec<(String, String)>
}

fn join_borrowed<'a, I: IntoIterator<Item = &'a str>>(vec: I, separator: char) -> String {
    let mut str = String::new();
    for e in vec {
        str += e;
        str.push(separator);
    }
    return str;
}

const SENSITIVITY: f64 = 300.0;
pub fn discover_pwm_fans() -> Result<Vec<Pwm2Fan>, Box<dyn std::error::Error>> {
    let mut state = QueryResult::new();
    let pwms = Pwm::scan()?;
    
    query_sensors(&mut state)?;
    {
        let fans = state.get_of_kind(SensorKind::Fan)
            .into_iter().map(|s| s.name.as_str());
        
        println!("Found {} fans: {}", fans.len(), join_borrowed(fans, ' '));
        println!("Found {} pwms: {}", pwms.len(), join_borrowed(pwms.iter().map(|p| p.get_key()), ' '));
    }

    for p in &pwms {
        p.set_auto(false)?;
        p.write_value(255)?;
    }
    sleep(Duration::from_secs(5));
    println!("Spun up fans");
    query_sensors(&mut state)?;
    for fan in state.get_of_kind(SensorKind::Fan) {
        print!("{} {:.0} RPM ", fan.name, fan.input)
    }
    println!();

    let fan_speeds: Vec<f64> = state.get_of_kind(SensorKind::Fan).into_iter()
        .map(|f| f.input).collect();
    let mut influence_list: Vec<Vec<usize>> = Vec::new();
    for p in &pwms {
        println!("Testing {}", p.get_key());
        p.write_value(0)?;
        sleep(Duration::from_secs(5));
        query_sensors(&mut state)?;
        let fans = state.get_of_kind(SensorKind::Fan);
        let mut affected_fans = vec![];
        for (i, (fan, original_speed)) in zip(fans, &fan_speeds).enumerate() {
            if (original_speed - fan.input) > SENSITIVITY {
                affected_fans.push(i);
            }
        }
        
        let mut str = String::new();
        for i in &affected_fans {
            str += &fans[*i].name;
            str += " ";
        }
        println!("{} affects {}", p.get_key(), str);
        
        influence_list.push(affected_fans);
        p.write_value(255)?;
        sleep(Duration::from_secs(5));
    }

    for p in &pwms {
        p.set_auto(true)?;
    }

    let fans = state.get_of_kind(SensorKind::Fan);
    let mut output = Vec::with_capacity(pwms.len());
    for (i, affected) in influence_list.iter().enumerate() {
        output.push(Pwm2Fan {
            pwm: pwms[i].get_key().to_string(),
            fans: affected.iter()
                .map(|i| (fans[*i].adapter.key.clone(), fans[*i].name.clone()))
                .collect()
        });
    }
    Ok(output)
}
