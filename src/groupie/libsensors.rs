#![cfg(feature = "libsensors")]

use std::{rc::Rc, str::Utf8Error};

use libsensors_rs::{
    Chip, Feature, GenericSubfeature, LibSensors, Subfeature, error::Error as LibsensorsError,
    feature::FeatureType,
};
use log::debug;

use crate::{
    eg_push_and_continue,
    groupie::{
        Adapter, OwnedKey, PluginStorage, Sensor, SensorKey, SensorKind, SensorPlugin, SensorState,
    },
    utils::{ErrorGroup, SimpleError},
};

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
    libsensors: &'ctx LibSensors,
    registered_sensors: Vec<LibsensorsSensorInfo<'ctx>>,
}
impl<'ctx> LibsensorsPlugin<'ctx> {
    pub fn new(libsensors: &'ctx LibSensors) -> Self {
        Self {
            libsensors,
            registered_sensors: Vec::new(),
        }
    }
}
impl<'ctx> SensorPlugin for LibsensorsPlugin<'ctx> {
    fn discover_sensors(
        &mut self,
        add_fn: &mut dyn FnMut(Sensor) -> Option<Sensor>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut error_group = ErrorGroup::new();
        let mut chip_iterator = self.libsensors.get_chips()?;
        while let Some(chip) = error_group.push_until_ok(&mut chip_iterator) {
            let chip_key = eg_push_and_continue!(error_group, chip_key(&chip));
            let chip_name = eg_push_and_continue!(error_group, chip.get_name());
            let adapter = Rc::new(Adapter {
                name: chip_name
                    .map(str::to_string)
                    .unwrap_or_else(|| chip_key.clone()),
                key: chip_key,
            });

            let mut feature_it = eg_push_and_continue!(error_group, chip.get_features());
            while let Some(feature) = error_group.push_until_ok(&mut feature_it) {
                let feature_label = eg_push_and_continue!(error_group, feature.get_label());
                let sensor_kind = match sensor_kind_from_feature_type(feature.get_type()) {
                    Some(x) => x,
                    None => {
                        debug!("Skipping sensor of unknown kind {adapter}/{feature_label}");
                        continue;
                    }
                };

                let input_subtype = match GenericSubfeature::Input.to_primitive(feature.get_type())
                {
                    Some(x) => x,
                    None => {
                        debug!(
                            "Feature type {:?} does not have an input subtype (or it is not supported). Skipping it.",
                            feature.get_type()
                        );
                        continue;
                    }
                };
                let input_subfeature = match eg_push_and_continue!(
                    error_group,
                    feature.get_subfeature_by_type(input_subtype)
                ) {
                    Some(x) => x,
                    None => {
                        debug!(
                            "Sensor {adapter}/{feature_label} has no input subtype. Skipping it."
                        );
                        continue;
                    }
                };
                let min_subfeature = eg_push_and_continue!(
                    error_group,
                    try_get_subfeature(&feature, GenericSubfeature::Min)
                );
                let max_subfeature = eg_push_and_continue!(
                    error_group,
                    try_get_subfeature(&feature, GenericSubfeature::Max)
                );

                let sensor = Sensor {
                    name: feature_label,
                    adapter: adapter.clone(),
                    kind: sensor_kind,
                };
                let key: OwnedKey = sensor.get_sensor_key().into();
                add_fn(sensor);
                self.registered_sensors.push(LibsensorsSensorInfo {
                    key,
                    subfeatures: SubfeatureArray {
                        input: input_subfeature,
                        min: min_subfeature,
                        max: max_subfeature,
                    },
                });
            }
        }
        Ok(error_group.ok()?)
    }

    fn update(&mut self, storage: &mut PluginStorage) -> Result<(), Box<dyn std::error::Error>> {
        let mut error_group = ErrorGroup::new();

        for sensor in &self.registered_sensors {
            const MAX_DEFAULT: f64 = f64::INFINITY;
            const MIN_DEFAULT: f64 = -f64::INFINITY;
            let input = eg_push_and_continue!(error_group, sensor.subfeatures.input.get_value());
            let max = match &sensor.subfeatures.max {
                None => Ok(MAX_DEFAULT),
                Some(x) => x.get_value(),
            }
            .unwrap_or_else(|e| {
                error_group.push(e);
                MAX_DEFAULT
            });
            let min = match &sensor.subfeatures.min {
                None => Ok(MIN_DEFAULT),
                Some(x) => x.get_value(),
            }
            .unwrap_or_else(|e| {
                error_group.push(e);
                MIN_DEFAULT
            });

            match storage.put_state(&sensor.key, SensorState::new(input, min, max)) {
                Ok(_) => {}
                Err(_) => error_group.push(SimpleError::new(format!(
                    "Could not find a sensor {} in storage, even though libsensors registered it.",
                    sensor.key.display()
                ))),
            };
        }

        Ok(error_group.ok()?)
    }
}

fn try_get_subfeature<'a>(
    feature: &Feature<'a>,
    subfeature: GenericSubfeature,
) -> Result<Option<Subfeature<'a>>, LibsensorsError> {
    /*
    None -> Ok(None)
    Some(Ok(x)) -> Ok(x)
    Some(Err(x)) -> Err(x)
    */
    subfeature
        .to_primitive(feature.get_type())
        .map(|subfeature_type| feature.get_subfeature_by_type(subfeature_type))
        .unwrap_or(Ok(None))
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
        _ => return None,
    })
}

struct LibsensorsSensorInfo<'lib> {
    key: OwnedKey,
    subfeatures: SubfeatureArray<'lib>,
}

struct SubfeatureArray<'lib> {
    input: Subfeature<'lib>,
    min: Option<Subfeature<'lib>>,
    max: Option<Subfeature<'lib>>,
}
