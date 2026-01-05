use std::ops::Deref;

use strum::IntoEnumIterator;

use crate::{GlobalContext, groupie::{QueryResult, SensorKind, query_sensors}};

fn format_input(kind: SensorKind, input: f64) -> String {
    match kind {
        SensorKind::Temperature => format!("{input:>2.1}°C"),
        SensorKind::Fan => format!("{input:>4.0} RPM"),
        SensorKind::Beep => (if input > 0.0 { "true" } else { "false" }).to_string(),
        SensorKind::Power => format!("{input:.2}W"),
        SensorKind::Voltmeter => format!("{input:.3}V"),
        SensorKind::Current => format!("{input:.3}A"),
        SensorKind::Energy => format!("{input:.3}J")
    }
}

pub fn start() {
    let context = GlobalContext::init().unwrap();
    let mut state = QueryResult::new();
    query_sensors(&mut state, &context).unwrap();
    for kind in SensorKind::iter() {
        println!("{kind}:");
        let mut last_adapter = None;
        for sensor in state.get_of_kind(kind).deref() {
            if Some(&*sensor.adapter) != last_adapter {
                println!("{} ({})", sensor.adapter.name, sensor.adapter.key);
            }
            println!("\t{:<15} {} (min {}, max {})",
                sensor.name, format_input(kind, sensor.input),
                format_input(kind, sensor.min), format_input(kind, sensor.max)
            );
            last_adapter = Some(&*sensor.adapter);
        }
        println!();
    }
}