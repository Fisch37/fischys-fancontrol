use crate::{GlobalContext, groupie::{Adapter, SensorKey, SensorKind, SensorStorage}};

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
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Warn)
        .env()
        .init()
        .unwrap();
    let context = GlobalContext::init().unwrap();
    let mut state = SensorStorage::new(&context);
    state.update().unwrap();
    
    let mut state: Vec<_> = state.iter().flat_map(|s| s.iter()).collect();
    state.sort_by(|(a, _), (b, _)| a.cmp_by_sensor_key(b));

    let mut last_adapter: Option<&Adapter> = None;
    for (sensor, state) in &state {
        if last_adapter.is_none_or(|last_adapter| *last_adapter != *sensor.adapter) {
            println!("\n{}:", sensor.adapter.name);

            last_adapter = Some(&sensor.adapter);
        }

        println!("{}", sensor.name);
        match state {
            None => println!("  N/A"),
            Some(state) => {
                println!("  Val: {}", format_input(sensor.kind, state.input));
                println!("  Min: {}", format_input(sensor.kind, state.min));
                println!("  Max: {}", format_input(sensor.kind, state.max));
            }
        }
    }
}