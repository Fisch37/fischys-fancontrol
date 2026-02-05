use std::{rc::Rc, str::Utf8Error};

use libsensors_rs::{Chip, Feature, GenericSubfeature, LibSensors, feature::FeatureType};
use log::debug;

use crate::{eg_push_and_continue, groupie::{Adapter, PluginStorage, Sensor, SensorKey, SensorKind, SensorPlugin, SensorState}, utils::{ErrorGroup, SimpleError}};

pub fn chip_key(chip: &Chip) -> Result<String, Utf8Error> {
    Ok(format!(
        "{}-{}-0b{:b}@{:X}",
        chip.get_prefix().to_str()?,
        Into::<&'static str>::into(chip.get_bus_id().type_),
        chip.get_bus_id().nr,
        chip.get_address()
    ))
}

pub struct LibsensorsPlugin<'ctx> {
    libsensors: &'ctx LibSensors
}
impl<'ctx> LibsensorsPlugin<'ctx> {
    pub fn new(libsensors: &'ctx LibSensors) -> Self {
        Self { libsensors }
    }
}
impl<'ctx> SensorPlugin for LibsensorsPlugin<'ctx> {
    fn discover_sensors(&mut self, add_fn: fn(Sensor) -> Option<Sensor>) -> Result<(), Box<dyn std::error::Error>> {
        let mut error_group = ErrorGroup::new();
        let mut chip_iterator = self.libsensors.get_chips()?;
        while let Some(chip) = error_group.push_until_ok(&mut chip_iterator) {
            let chip_key = eg_push_and_continue!(error_group, chip_key(&chip));
            let chip_name = eg_push_and_continue!(error_group, chip.get_name());
            let adapter = Rc::new(Adapter {
                name: chip_name.map(str::to_string).unwrap_or_else(|| chip_key.clone()),
                key: chip_key,
            });

            let mut feature_it = eg_push_and_continue!(error_group, chip.get_features());
            while let Some(feature) = error_group.push_until_ok(&mut feature_it) {
                let feature_label = eg_push_and_continue!(error_group, feature.get_label());
                let sensor_kind = eg_push_and_continue!(
                    error_group,
                    sensor_kind_from_feature_type(feature.get_type())
                        .ok_or_else(|| SimpleError::new(format!("Sensor of unknown kind {adapter}/{feature_label}")))
                );

                let input_subtype = match GenericSubfeature::Input.to_primitive(feature.get_type()) {
                    Some(x) => x,
                    None => {
                        debug!("Feature type {:?} does not have an input subtype (or it is not supported). Skipping it.", feature.get_type());
                        continue;
                    }
                };
                if eg_push_and_continue!(error_group, feature.get_subfeature_by_type(input_subtype)).is_none() {
                    debug!("Sensor {adapter}/{feature_label} has no input subtype, skipping it.");
                    continue;
                }

                add_fn(Sensor {
                    name: feature_label,
                    adapter: adapter.clone(),
                    kind: sensor_kind
                });
            }
        }
        Ok(error_group.ok()?)
    }

    fn update(&mut self, storage: &mut PluginStorage) -> Result<(), Box<dyn std::error::Error>> {
        let mut error_group = ErrorGroup::new();
        let mut chip_iterator = self.libsensors.get_chips()?;
        while let Some(chip) = error_group.push_until_ok(&mut chip_iterator) {
            let chip_key = eg_push_and_continue!(error_group, chip_key(&chip));

            let mut feature_it = eg_push_and_continue!(error_group, chip.get_features());
            while let Some(feature) = error_group.push_until_ok(&mut feature_it) {
                let feature_label = eg_push_and_continue!(error_group, feature.get_label());
                
                if let Some((sensor, state)) = storage.get_both_mut((&chip_key, &feature_label)) {
                    let input_subfeature = eg_push_and_continue!(
                        error_group,
                        GenericSubfeature::Input.to_primitive(feature.get_type())
                            .ok_or_else(|| SimpleError::new(
                                format!("Input subfeature type for sensor {} was available at discovery, but disappeared at update. How can this even happen?", sensor.display())
                            ))
                    );
                    let input_subfeature = eg_push_and_continue!(
                        error_group,
                        eg_push_and_continue!(
                            error_group,
                            feature.get_subfeature_by_type(input_subfeature)
                        ).ok_or_else(|| {
                            SimpleError::new(format!("Input subfeature does not exist for sensor {}", sensor.display()))
                        })
                    );

                    *state = Some(SensorState::new(
                        eg_push_and_continue!(error_group, input_subfeature.get_value()),
                        try_read_subfeature(
                            &feature,
                            GenericSubfeature::Min,
                            -f64::INFINITY
                        ),
                        try_read_subfeature(
                            &feature,
                            GenericSubfeature::Max,
                            f64::INFINITY
                        )
                    ));
                }
            }
        }
        Ok(error_group.ok()?)
    }
}


fn sensor_kind_from_feature_type(feature_type: FeatureType) -> Option<SensorKind> {
    use FeatureType::*;
    Some(match feature_type {
        Current => SensorKind::Current,
        Energy => SensorKind::Energy,
        Fan => SensorKind::Fan,
        In => SensorKind::Voltmeter,
        Power => SensorKind::Power,
        Temp => SensorKind::Temperature,
        _ => return None
    })
}

fn try_read_subfeature(feature: &Feature, subfeature_type: GenericSubfeature, default: f64) -> f64 {
    subfeature_type.to_primitive(feature.get_type())
        .and_then(|specific_type| {
            feature.get_subfeature_by_type(specific_type).ok()
                .and_then(|s| s)
                .and_then(|s| s.get_value().ok())
        })
        .unwrap_or_else(|| {
            debug!("Failed to grab subfeature {subfeature_type:?}. Failing quietly");
            default
        })
}
