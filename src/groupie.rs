//! Groupie is the central sensor managing API.
//! This project collects all sensor data in a single [`SensorStorage`],
//! where all enabled [`SensorPlugin`]s are also immediately initialised.
//!
//! This module also contains the datastructures necessary to work with sensor data,
//! most notably the [`SensorKey`] trait and the [`OwnedKey`] struct.

use std::{array, cmp::Ordering, error::Error, fmt::Display, hash::Hash, rc::Rc, time::Instant};

use hashbrown::{Equivalent, HashMap};
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use strum::{EnumCount, EnumIter, EnumString, VariantArray};

use crate::{GlobalContext, utils::ErrorGroup};

mod libsensors;
#[cfg(feature = "sensors-cmd")]
mod lm_sensors;
mod nvidia;
mod nvml;

/// A helper struct for displaying a [`SensorKey`].
/// Used in the [`SensorKey::display`] method.
pub struct KeyWriter<'a>(&'a str, &'a str);
impl<'a> Display for KeyWriter<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.0, self.1)
    }
}

/// A sensor key is any datastructure that can be used to uniquely identify a [`Sensor`].
///
/// Two things uniquely identify a sensor: The key to its adapter and its name.
/// As such, the core of this trait is the [`Self::get_sensor_key`] method,
/// which retrieves this pair of information.
pub trait SensorKey {
    /// Get the uniquely identifying sensor key contained in this datastructure.
    ///
    /// The first value is the adapter key and the second the sensor name.
    fn get_sensor_key(&self) -> (&str, &str);

    /// Like [`Self::get_sensor_key`], but returns an [`OwnedKey`] instead.
    fn get_owned_key(&self) -> OwnedKey {
        self.get_sensor_key().into()
    }

    /// Comparator method providing a total ordering on two [`SensorKey`]
    /// instances. The ordering is lexicographical with highest priority
    /// on the adapter key, secondary comparison on the sensor name.
    ///
    /// This means that the following order would be correct:
    /// (A, a), (A, b), (B, a), (B, a), (B, b)
    fn cmp_by_sensor_key<T: SensorKey>(&self, other: T) -> Ordering {
        let my_key = self.get_sensor_key();
        let other_key = other.get_sensor_key();
        match my_key.0.cmp(other_key.0) {
            Ordering::Equal => my_key.1.cmp(other_key.1),
            x => x,
        }
    }

    /// Returns a helper for displaying a sensor key,
    /// formatted as &lt;adapter key&gt;/&lt;sensor name&gt;.
    ///
    /// The intended use for this is to handle display properties
    /// that cannot be implemented directly, due to ambiguities or Rust's orphaning rules.
    fn display(&self) -> KeyWriter<'_> {
        let (name, adapter) = self.get_sensor_key();
        KeyWriter(name, adapter)
    }

    /// Returns only the second part of the sensor key.
    fn get_sensor_name(&self) -> &str {
        self.get_sensor_key().1
    }

    /// Returns only the first part of the sensor key.
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
/// An owned implementor of [`SensorKey`].
pub struct OwnedKey {
    pub adapter: String,
    pub name: String,
}
impl From<OwnedKey> for (String, String) {
    fn from(value: OwnedKey) -> Self {
        (value.adapter, value.name)
    }
}
impl From<(String, String)> for OwnedKey {
    fn from(value: (String, String)) -> Self {
        OwnedKey {
            adapter: value.0,
            name: value.1,
        }
    }
}
impl<'a, 'b, A, B> From<(&'a A, &'b B)> for OwnedKey
where
    A: AsRef<str> + ?Sized,
    B: AsRef<str> + ?Sized,
{
    fn from(value: (&'a A, &'b B)) -> Self {
        From::<(String, String)>::from((value.0.as_ref().to_owned(), value.1.as_ref().to_owned()))
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
/// An adapter (or chip, or bus, or device) is a logical object on which sensors can be found.
/// The exact semantics of this are intentionally left up to each sensor plugin and Groupie itself
/// makes no assumptions about what qualifies as an adapter.
pub struct Adapter {
    /// The key uniquely identifies this adapter.
    /// Two different logical adapters should never be referred to by the same key
    /// and doing so constitutes a logic error.
    pub key: String,
    /// A human-readable name for this adapter.
    /// This name need not be unique, though developers should strive to choose a readable name.
    /// (e.g. NVML uses the name of the GPU as the adapter name)
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
/// A merged form of [`Sensor`] and [`SensorState`].
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
        (&self.adapter.key, self.name)
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
    Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, EnumCount, EnumIter, EnumString,
)]
#[strum(serialize_all = "kebab-case")]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
/// The kind of value a sensor is storing.
/// This also corresponds to a particular unit.
///
/// Units are kept in their "everyday" format,
/// but scaled down to their base unit where applicable.
/// (e.g. [`Self::Fan`] uses RPM, but [`Self::Power`] uses watts, not milliwatts)
///
/// It is highly recommendable for plugin developers to look at the specified units
/// and scale the values received by their datasource accordingly.
pub enum SensorKind {
    Temperature,
    Fan,
    Beep,
    Power,
    Voltmeter,
    Current,
    Energy,
}
impl SensorKind {
    /// Get the unit affix for this kind of sensor.
    pub fn get_unit(self) -> &'static str {
        match self {
            Self::Temperature => "°C",
            Self::Fan => "RPM",
            Self::Beep => "", // 1 BEEEEEP, 2 BEEEEEP, 3 BEEEEEEP, ...
            Self::Power => "W",
            Self::Voltmeter => "V",
            Self::Current => "A",
            // Energy is open to discussion and may be better used given in "kWh"
            Self::Energy => "J",
        }
    }
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
    /// A monotonic clock value, marking the time at which this state was constructed.
    // Using a u32 here ensures our SensorState is 32 bytes long, instead of the 40 if we had used Instant
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
        self
    }
}

#[derive(Debug, Clone)]
/// The unchanging metadata about a sensor.
/// Collected by [`SensorStorage`] during the first [`SensorStorage::update`] call
/// and expected to be permanent for the entire runtime of the program.
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

/// The central data holder of Groupie.
/// Stores all sensor data as well as all [`SensorPlugin`] instances.
///
/// Groupie does not guarantee that only one instance of this struct will exist at a time
/// and plugins should especially be able to handle being reinstantiated over the plugin lifetime.
pub struct SensorStorage<'ctx> {
    /// An array of all plugins and their respective storages.
    plugins: [(Box<dyn SensorPlugin + 'ctx>, PluginStorage); GroupiePluginType::COUNT],
    /// A mapping of sensor keys to plugin types.
    /// This is a cross-dependency with self.plugins as any key represented here must necessarily
    /// appear in the PluginStorage of that plugin.
    sensor_to_plugin: HashMap<OwnedKey, GroupiePluginType>,
    has_discovered_sensors: bool,
}
impl<'ctx> SensorStorage<'ctx> {
    /// Create a new storage with all active plugins initialised
    /// and no stored data.
    pub fn new(context: &'ctx GlobalContext) -> Self {
        Self {
            plugins: array::from_fn(|i| {
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
    /// Get the [`PluginStorage`] for a particular sensor, if it exists within the database.
    fn get_storage_for<K: SensorKey>(&self, key: K) -> Option<&PluginStorage> {
        self.sensor_to_plugin
            .get(&key.get_sensor_key())
            .map(|plugin| &self.plugins[*plugin as usize].1)
    }

    /// Get the metadata of a sensor, if that sensor exists.
    pub fn get_sensor<K: SensorKey>(&self, key: K) -> Option<&Sensor> {
        let key = key.get_sensor_key();
        self.get_storage_for(key)
            .and_then(|storage| storage.get_sensor(key))
    }

    /// Get the metadata and state data for a sensor combined.
    ///
    /// Returns [`None`] if the sensor does not exist _or_ it does not have any data.
    pub fn get_sensor_data<T: SensorKey>(&self, key: T) -> Option<SensorData<'_>> {
        let key = key.get_sensor_key();
        self.get_storage_for(key)
            .and_then(|storage| storage.get(key))
    }

    /// Repoll all sensors.
    pub fn update(&mut self) -> Result<(), ErrorGroup> {
        let mut errors = ErrorGroup::new();
        if !self.has_discovered_sensors {
            for (plugin_type, (plugin, plugin_storage)) in
                GroupiePluginType::VARIANTS.iter().zip(&mut self.plugins)
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
            self.has_discovered_sensors = true;
        }
        for (plugin, plugin_storage) in &mut self.plugins {
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
        self.plugins
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
///
/// Most importantly, there is no simple `insert` function, due to the integrity constraint in [`SensorStorage`].
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
        state.as_ref().map(|state| (sensor, state).into())
    }

    /// Returns the sensor metadata, if a sensor for that key exists.
    pub fn get_sensor<K: SensorKey>(&self, key: K) -> Option<&Sensor> {
        self.get_raw(&key).map(|(sensor, _)| sensor)
    }
    fn put_sensor(&mut self, sensor: Sensor) -> Option<(Sensor, Option<SensorState>)> {
        self.inner
            .insert(sensor.get_sensor_key().into(), (sensor, None))
    }

    /// Returns the state of the sensor referenced by `key`.
    /// Returns [`None`] if the sensor does not exist, or it does not have a key.
    pub fn get_state<K: SensorKey>(&self, key: K) -> Option<&SensorState> {
        self.get_raw(&key)?.1.as_ref()
    }
    /// Returns a mutbale reference to the state of the sensor referred to by that key, if it exists.
    ///
    /// Returns [`None`], if no sensor for `key` exists, else a mutable reference to an [`Option<SensorState>`],
    /// allowing you to modify the state or indeed delete it.
    pub fn get_state_mut<K: SensorKey>(&mut self, key: K) -> Option<&mut Option<SensorState>> {
        Some(&mut self.get_raw_mut(&key)?.1)
    }

    /// Inserts a [`SensorState`] for a sensor, referred to by `key`.
    ///
    /// Returns an [`Err`] variant containing the passed state, if no sensor for that key exists.
    pub fn put_state<K: SensorKey>(
        &mut self,
        key: K,
        state: SensorState,
    ) -> Result<Option<SensorState>, SensorState> {
        match self.get_state_mut(key) {
            Some(x) => Ok(x.replace(state)),
            None => Err(state),
        }
    }

    /// Get references to both the sensor metadata as well as the sensor state,
    /// allowing mutation on the state.
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
impl Default for PluginStorage {
    fn default() -> Self {
        Self::new()
    }
}

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
