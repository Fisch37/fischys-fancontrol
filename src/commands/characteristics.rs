use crate::{fan_configuration::{detect_fan_properties, FanProperties}, fan_discovery, groupie::{query_sensors, QueryResult, SensorKind}, pwms::Pwm};

pub fn start() { 

}

fn find_fan_characteristics() -> Result<Vec<FanProperties>, Box<dyn std::error::Error>> {
    /*match ctrlc::set_handler(|| {
        let _ = Pwm::scan().and_then(|pwms| {
            let mut res = Ok(());
            for p in pwms {
                res = res.and(p.set_auto(true));
            }
            return res;
        }).unwrap();
    }) {
        Ok(_) => { },
        Err(e) => println!("Failed to add Ctrl+C cleanup handler. When interrupting, you will need to manually set the fans back to automatic. Error: {}", e)
    }*/

    let fan_map = fan_discovery::discover_pwm_fans()?;
    let pwms = Pwm::scan()?;
    let mut sensors = QueryResult::new();
    query_sensors(&mut sensors)?;

    let mut output = Vec::with_capacity(fan_map.iter().map(|p|p.fans.len()).sum());
    for pwm_fans in &fan_map {
        let pwm = pwms.iter().find(|p| p.get_name() == pwm_fans.pwm).expect(format!("Could not find a PWM with name {}", pwm_fans.pwm).as_str());
        for fan_key in &pwm_fans.fans {
            let fan = sensors.get_of_kind(SensorKind::Fan)
                .get(&fan_key.0, &fan_key.1)
                .expect(format!("Could not find a fan {}/{}", fan_key.0, fan_key.1).as_str());
            let properties = detect_fan_properties(fan, pwm)?;
            output.push(properties);
        }
    }

    Ok(output)
}