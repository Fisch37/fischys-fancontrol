use std::{
    array, cmp::Ordering, error::Error, fmt::Display, hash::Hash, mem, rc::Rc, time::Instant,
};

use hashbrown::{Equivalent, HashMap};
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use strum::{EnumCount, EnumIter, VariantArray};

use crate::{GlobalContext, utils::ErrorGroup};

#[cfg(feature = "libsensors")]
mod libsensors;
#[cfg(feature = "sensors-cmd")]
mod lm_sensors;
mod nvidia;
mod nvml;

pub struct KeyWriter<'a>(&'a str, &'a str);
impl<'a> Display for KeyWriter<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.0, self.1)
    }
}

pub trait SensorKey {
    fn get_sensor_key(&self) -> (&str, &str);

    fn get_owned_key(&self) -> OwnedKey {
        self.get_sensor_key().into()
    }

    fn cmp_by_sensor_key<T: SensorKey>(&self, other: T) -> Ordering {
        let my_key = self.get_sensor_key();
        let other_key = other.get_sensor_key();
        match my_key.0.cmp(other_key.0) {
            Ordering::Equal => my_key.1.cmp(other_key.1),
            x => x,
        }
    }

    fn display(&self) -> KeyWriter<'_> {
        let (name, adapter) = self.get_sensor_key();
        KeyWriter(name, adapter)
    }

    fn get_sensor_name(&self) -> &str {
        self.get_sensor_key().1
    }

    fn get_adapter_key(&self) -> &str {
        self.get_sensor_key().0
    }
}
impl<A: AsRef<str>, B: AsRef<str>> SensorKey for (A, B) {
    fn get_sensor_key(&self) -> (&str, &str) {
        (self.0.as_ref(), self.1.as_ref())
    }
}
impl<T: SensorKey> SensorKey for &T {
    fn get_sensor_key(&self) -> (&str, &str) {
        (*self).get_sensor_key()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
// Hacky, probably causes an allocation on serialize; fixme?
#[serde(from = "(String, String)", into = "(String, String)")]
pub struct OwnedKey {
    pub adapter: String,
    pub name: String,
}
impl From<(String, String)> for OwnedKey {
    fn from(value: (String, String)) -> Self {
        OwnedKey {
            adapter: value.0,
            name: value.1,
        }
    }
}
impl From<OwnedKey> for (String, String) {
    fn from(value: OwnedKey) -> Self {
        (value.adapter, value.name)
    }
}
impl<'a, 'b> From<(&'a str, &'b str)> for OwnedKey {
    fn from(value: (&'a str, &'b str)) -> Self {
        (value.0.to_owned(), value.1.to_owned()).into()
    }
}
impl SensorKey for OwnedKey {
    fn get_sensor_key(&self) -> (&str, &str) {
        (&self.adapter, &self.name)
    }
}
impl Display for OwnedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display())
    }
}
impl Equivalent<OwnedKey> for (&str, &str) {
    fn equivalent(&self, key: &OwnedKey) -> bool {
        *self == key.get_sensor_key()
    }
}

#[derive(Debug, Eq)]
pub struct Adapter {
    pub key: String,
    pub name: String,
}
impl PartialEq for Adapter {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl PartialOrd for Adapter {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Adapter {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key.cmp(&other.key)
    }
}
impl Display for Adapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[derive(Debug, Clone)]
pub struct SensorData<'a> {
    pub name: &'a str,
    pub input: f64,
    pub min: f64,
    pub max: f64,
    pub adapter: Rc<Adapter>,
    pub kind: SensorKind,
}
impl<'a> SensorKey for SensorData<'a> {
    fn get_sensor_key(&self) -> (&str, &str) {
        (&self.adapter.key, &self.name)
    }
}
impl<'a, T: SensorKey> PartialEq<T> for SensorData<'a> {
    fn eq(&self, other: &T) -> bool {
        self.get_sensor_key() == other.get_sensor_key()
    }
}
impl<'a> Eq for SensorData<'a> {}
impl<'a, T: SensorKey> PartialOrd<T> for SensorData<'a> {
    fn partial_cmp(&self, other: &T) -> Option<Ordering> {
        Some(self.cmp_by_sensor_key(other))
    }
}
impl<'a> Ord for SensorData<'a> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.cmp_by_sensor_key((&other.adapter.key, other.name))
    }
}
impl<'a> From<(&'a Sensor, &SensorState)> for SensorData<'a> {
    fn from((sensor, state): (&'a Sensor, &SensorState)) -> Self {
        SensorData {
            name: &sensor.name,
            input: state.input,
            min: state.min,
            max: state.max,
            adapter: sensor.adapter.clone(),
            kind: sensor.kind,
        }
    }
}
impl<'a> From<&'a (Sensor, SensorState)> for SensorData<'a> {
    fn from(value: &'a (Sensor, SensorState)) -> Self {
        Self::from((&value.0, &value.1))
    }
}

#[repr(u8)]
#[derive(
    Debug,
    Clone,
    Copy,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    EnumCount,
    EnumIter,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum SensorKind {
    Temperature,
    Fan,
    Beep,
    Power,
    Voltmeter,
    Current,
    Energy,
}
impl Display for SensorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

lazy_static! {
    static ref MONOTONIC_COUNT_START: Instant = Instant::now();
}
fn get_monotonic_seconds() -> u32 {
    // downcasting to u32 will cause a rollover in approximately 136 years.
    // I don't plan on running my PC for that long
    MONOTONIC_COUNT_START.elapsed().as_secs() as u32
}

#[derive(Debug, Clone)]
pub struct SensorState {
    pub input: f64,
    pub min: f64,
    pub max: f64,
    // Using a u32 here ensures our SensorState is 32 bytes long, instead of the 40 if we had used Instant
    /// A monotonic clock value, marking the time at which this state was constructed.
    pub polled_at: u32,
}
impl SensorState {
    #[inline]
    pub(self) fn new(input: f64, min: f64, max: f64) -> Self {
        Self {
            input,
            min,
            max,
            polled_at: get_monotonic_seconds(),
        }
    }
}
impl AsRef<SensorState> for SensorState {
    fn as_ref(&self) -> &SensorState {
        &self
    }
}

#[derive(Debug, Clone)]
pub struct Sensor {
    pub name: String,
    pub adapter: Rc<Adapter>,
    pub kind: SensorKind,
}
impl SensorKey for Sensor {
    fn get_sensor_key(&self) -> (&str, &str) {
        (&self.adapter.key, &self.name)
    }
}

/// A plugin is responsible for discovering and updating sensors.
/// It does not store any data itself, except such data as is necessary to allow for its functions.
trait SensorPlugin {
    /// Discover all sensors available from this plugin.
    /// This method will generally run exactly once per plugin instance.
    fn discover_sensors(
        &mut self,
        add_fn: &mut dyn FnMut(Sensor) -> Option<Sensor>,
    ) -> Result<(), Box<dyn Error>>;
    fn update(&mut self, storage: &mut PluginStorage) -> Result<(), Box<dyn Error>>;
}

/// I need:
///   - A persistent storage of sensors (all metadata not expected to change during program lifetime)
///     currently this means: key, adapter, and kind.
///   - A temporary storage for the state of sensors
///   - Access to those states by sensor key (e.g. for service & characteristics subcommands)
///   - Mutable access to those states by sensor key (for updates)
///   - Access to all sensors by plugin for plugin updates
pub struct SensorStorage<'ctx> {
    sensors: [(Box<dyn SensorPlugin + 'ctx>, PluginStorage); GroupiePluginType::COUNT],
    sensor_to_plugin: HashMap<OwnedKey, GroupiePluginType>,
    has_discovered_sensors: bool,
}
impl<'ctx> SensorStorage<'ctx> {
    pub fn new(context: &'ctx GlobalContext) -> Self {
        Self {
            sensors: array::from_fn(|i| {
                (
                    GroupiePluginType::VARIANTS[i].make_plugin(context),
                    PluginStorage::new(),
                )
            }),
            sensor_to_plugin: HashMap::new(),
            has_discovered_sensors: false,
        }
    }

    #[inline]
    fn get_storage_for<K: SensorKey>(&self, key: K) -> Option<&PluginStorage> {
        self.sensor_to_plugin
            .get(&key.get_sensor_key())
            .map(|plugin| &self.sensors[*plugin as usize].1)
    }

    pub fn get_sensor<K: SensorKey>(&self, key: K) -> Option<&Sensor> {
        let key = key.get_sensor_key();
        self.get_storage_for(key)
            .and_then(|storage| storage.get_sensor(key))
    }

    pub fn get_sensor_data<T: SensorKey>(&self, key: T) -> Option<SensorData<'_>> {
        let key = key.get_sensor_key();
        self.get_storage_for(key)
            .and_then(|storage| storage.get(key))
    }

    pub fn update(&mut self) -> Result<(), ErrorGroup> {
        let mut errors = ErrorGroup::new();
        if !self.has_discovered_sensors {
            for (plugin_type, (plugin, plugin_storage)) in
                GroupiePluginType::VARIANTS.iter().zip(&mut self.sensors)
            {
                match plugin.discover_sensors(&mut |sensor| {
                    self.sensor_to_plugin
                        .insert(sensor.get_sensor_key().into(), *plugin_type);
                    plugin_storage.put_sensor(sensor).map(|(s, _)| s)
                }) {
                    Ok(_) => {}
                    Err(e) => errors.push_box(e),
                }
            }
        }
        for (plugin, plugin_storage) in &mut self.sensors {
            if let Err(e) = plugin.update(plugin_storage) {
                errors.push_box(e);
            }
        }
        errors.ok()
    }

    /// Iterate over all sensors in an unspecified order.
    ///
    /// Note that if you only care about the updated sensors, [`Self::iter_data`] is preferrable.
    pub fn iter(&self) -> impl Iterator<Item = &(Sensor, Option<SensorState>)> {
        self.sensors
            .iter()
            .map(|(_, storage)| storage)
            .flat_map(PluginStorage::iter)
    }

    pub fn iter_sensors(&self) -> impl Iterator<Item = &Sensor> {
        self.iter().map(|(sensor, _)| sensor)
    }

    /// Iterate over all sensors that have data in an unspecified order.
    pub fn iter_data(&self) -> impl Iterator<Item = SensorData<'_>> {
        self.iter()
            .filter_map(|(sensor, state)| state.as_ref().map(|state| (sensor, state).into()))
    }
}

/// A wrapper struct that removes some of the functionalities of the internal HashMap.
/// Used to ensure plugins cannot mess up the invariants required for internal datastructure integrity.
pub struct PluginStorage {
    inner: HashMap<OwnedKey, (Sensor, Option<SensorState>)>,
}
impl PluginStorage {
    /// Create a newly constructed, empty storage.
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    pub fn trim_to_size(&mut self) {
        self.inner.shrink_to_fit();
    }

    fn get_raw<K: SensorKey>(&self, key: &K) -> Option<&(Sensor, Option<SensorState>)> {
        self.inner.get(&key.get_sensor_key())
    }
    fn get_raw_mut<K: SensorKey>(&mut self, key: &K) -> Option<&mut (Sensor, Option<SensorState>)> {
        self.inner.get_mut(&key.get_sensor_key())
    }

    // TODO: None represents two logical states here and in get_state.
    //  This is less than beautiful and may be better served by a ternary enum.

    /// Get the combined state of a sensor on this key, if it is available.
    /// If no sensor for the key exists, or it does not have a state, returns [`None`].
    /// Otherwise, returns a newly constructed [`SensorData`] instance (wrapped in [`Some`]).
    pub fn get<K: SensorKey>(&self, key: K) -> Option<SensorData<'_>> {
        let (sensor, state) = self.get_raw(&key)?;
        match state {
            &None => None,
            &Some(ref state) => Some((sensor, state).into()),
        }
    }

    /// Returns the sensor metadata, if a sensor for that key exists.
    pub fn get_sensor<K: SensorKey>(&self, key: K) -> Option<&Sensor> {
        self.get_raw(&key).map(|(sensor, _)| sensor)
    }
    pub fn put_sensor(&mut self, sensor: Sensor) -> Option<(Sensor, Option<SensorState>)> {
        self.inner
            .insert(sensor.get_sensor_key().into(), (sensor, None))
    }

    /// Returns the state of the sensor referenced by `key`.
    /// Returns [`None`] if the sensor does not exist, or it does not have a key.
    pub fn get_state<K: SensorKey>(&self, key: K) -> Option<&SensorState> {
        match self.get_raw(&key)?.1 {
            None => None,
            Some(ref state) => Some(state),
        }
    }
    /// Returns a mutbale reference to the state of the sensor referred to by that key, if it exists.
    ///
    /// Returns [`None`], if no sensor for `key` exists, else a mutable reference to an [`Option<SensorState>`],
    /// allowing you to modify the state or indeed delete it.
    pub fn get_state_mut<K: SensorKey>(&mut self, key: K) -> Option<&mut Option<SensorState>> {
        Some(&mut self.get_raw_mut(&key)?.1)
    }

    pub fn put_state<K: SensorKey>(
        &mut self,
        key: K,
        state: SensorState,
    ) -> Result<Option<SensorState>, ()> {
        match self.get_state_mut(key) {
            Some(x) => Ok(mem::replace(x, Some(state))),
            None => Err(()),
        }
    }

    pub fn get_both_mut<K: SensorKey>(
        &mut self,
        key: K,
    ) -> Option<(&Sensor, &mut Option<SensorState>)> {
        self.get_raw_mut(&key).map(|raw| (&raw.0, &mut raw.1))
    }

    pub fn iter(&self) -> impl Iterator<Item = &(Sensor, Option<SensorState>)> {
        self.inner.values()
    }
}
pub struct SensorIterator<'a, I: Iterator<Item = (&'a OwnedKey, SensorData<'a>)>>(I);

#[derive(Clone, Copy, Hash, PartialEq, Eq, strum::EnumCount, strum::VariantArray)]
#[repr(u8)]
enum GroupiePluginType {
    #[cfg(feature = "sensors-cmd")]
    LmSensors,
    #[cfg(feature = "libsensors")]
    LibSensors,
    #[cfg(feature = "nvidia-smi")]
    NvidiaSmi,
    #[cfg(feature = "nvml")]
    Nvml,
}
impl GroupiePluginType {
    pub fn make_plugin<'a>(self, context: &'a GlobalContext) -> Box<dyn SensorPlugin + 'a> {
        match self {
            #[cfg(feature = "sensors-cmd")]
            Self::LmSensors => todo!(),
            #[cfg(feature = "libsensors")]
            Self::LibSensors => Box::new(libsensors::LibsensorsPlugin::new(&context.libsensors)),
            #[cfg(feature = "nvidia-smi")]
            Self::NvidiaSmi => todo!(),
            #[cfg(feature = "nvml")]
            Self::Nvml => Box::new(nvml::NvmlPlugin::new(&context.nvml)),
        }
    }
}
