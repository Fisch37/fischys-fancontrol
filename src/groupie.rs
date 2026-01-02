use std::{borrow::Borrow, cmp::Ordering, error::Error, fmt::Display, hash::Hash, ops::{Deref, Index}, rc::Rc, slice::SliceIndex};

use serde::{Deserialize, Serialize};
use strum::{EnumCount, EnumIter};

use crate::GlobalContext;

mod nvidia;
mod lm_sensors;
mod nvml;

pub trait SensorKey {
    fn get_sensor_key(&self) -> (&str, &str);

    fn cmp_by_sensor_key<T: SensorKey>(&self, other: T) -> Ordering {
        let my_key = self.get_sensor_key();
        let other_key = other.get_sensor_key();
        match my_key.0.cmp(other_key.0) {
            Ordering::Equal => my_key.1.cmp(other_key.1),
            x => x
        }
    }

    fn write_key(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let key = self.get_sensor_key();
        write!(f, "{}/{}", key.0, key.1)
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

#[derive(Debug, Eq)]
pub struct Adapter {
    pub key: String,
    pub name: String
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

#[derive(Debug, Clone)]
pub struct SensorData {
    pub kind: SensorKind,
    pub name: String,
    pub input: f64,
    pub min: f64,
    pub max: f64,
    pub adapter: Rc<Adapter>
}
impl SensorKey for SensorData {
    fn get_sensor_key(&self) -> (&str, &str) {
        (&self.adapter.key, &self.name)
    }
}
impl<T: SensorKey> PartialEq<T> for SensorData {
    fn eq(&self, other: &T) -> bool {
        self.get_sensor_key() == other.get_sensor_key()
    }
}
impl Eq for SensorData { }
impl<T: SensorKey> PartialOrd<T> for SensorData {
    fn partial_cmp(&self, other: &T) -> Option<Ordering> {
        Some(self.cmp_by_sensor_key(other))
    }
}
impl Ord for SensorData {
    fn cmp(&self, other: &Self) -> Ordering {
        self.cmp_wireframe(&other.adapter.key, &other.name)
    }
}
impl SensorData {
    fn cmp_wireframe(&self, adapter: &String, name: &String) -> Ordering {
        self.cmp_by_sensor_key((adapter, name))
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, Hash)]
#[derive(PartialEq, Eq, PartialOrd, Ord)]
#[derive(EnumCount, EnumIter)]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SensorKind {
    Temperature, Fan, Voltmeter, Beep
}
impl SensorKind {
    pub fn from_string(name: &str) -> Option<SensorKind> {
        if name.starts_with("temp") {
            Some(SensorKind::Temperature)
        } else if name.starts_with("fan") {
            Some(SensorKind::Fan)
        } else if name.starts_with("in") {
            Some(SensorKind::Voltmeter)
        } else if name.starts_with("beep") {
            Some(SensorKind::Beep)
        } else {
            None
        }
    }
}
impl Display for SensorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

pub struct QueryResult {
    internal: [SensorStorage; SensorKind::COUNT]
}
impl QueryResult {
    pub fn new() -> QueryResult {
        // const block is once per array element. Don't ask me why, but I tested and that's how it works
        QueryResult { internal: [const { SensorStorage::new() }; SensorKind::COUNT] }
    }

    pub const fn get_of_kind(&self, kind: SensorKind) -> &SensorStorage {
        &self.internal[kind as usize]
    }

    pub fn get_of_kind_mut(&mut self, kind: SensorKind) -> &mut SensorStorage {
        &mut self.internal[kind as usize]
    }

    // Returns Err if an equal value is already present
    pub fn add(&mut self, data: SensorData) -> Result<(), SensorData> {
        self.get_of_kind_mut(data.kind).add(data)
    }

    fn clear(&mut self) {
        for store in &mut self.internal {
            store.clear();
        }
    }

    fn shrink_to_fit(&mut self) {
        for store in &mut self.internal {
            store.shrink_to_fit();
        }
    }
}
impl Default for QueryResult {
    fn default() -> Self {
        Self::new()
    }
}
#[derive(Debug, Clone)]
pub struct SensorStorage {
    internal: Vec<SensorData>
}
impl Deref for SensorStorage {
    type Target = [SensorData];

    fn deref(&self) -> &Self::Target {
        &self.internal
    }
}
impl AsRef<Vec<SensorData>> for SensorStorage {
    fn as_ref(&self) -> &Vec<SensorData> {
        &self.internal
    }
}
impl Borrow<Vec<SensorData>> for SensorStorage {
    fn borrow(&self) -> &Vec<SensorData> {
        &self.internal
    }
}
impl<'a> IntoIterator for &'a SensorStorage {
    type Item = &'a SensorData;

    type IntoIter = std::slice::Iter<'a, SensorData>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl<I: SliceIndex<[SensorData], Output = SensorData>> Index<I> for SensorStorage {
    type Output = SensorData;

    fn index(&self, index: I) -> &Self::Output {
        self.internal.index(index)
    }
}
impl<'a> From<&'a SensorStorage> for &'a [SensorData] {
    fn from(value: &'a SensorStorage) -> Self {
        value.internal.as_slice()
    }
}
impl SensorStorage {
    pub const fn new() -> Self {
        // no preset capacity as the required capacity is unknown before insertion
        // SensorStorage is also expected to be reused frequently, so the actual impact is minimal
        SensorStorage { internal: Vec::new() }
    }

    pub fn get_from_parts(&self, adapter: &String, name: &String) -> Option<&SensorData> {
        self.get(&(adapter, name))
    }

    pub fn get<Key: SensorKey>(&self, key: &Key) -> Option<&SensorData> {
        match self.internal.binary_search_by(|sensor| sensor.cmp_by_sensor_key(key)) {
            Ok(i) => Some(&self.internal[i]),
            Err(_) => None
        }
    }

    pub fn unpack(self) -> Vec<SensorData> {
        self.internal
    }

    pub fn add(&mut self, data: SensorData) -> Result<(), SensorData> {
        match self.internal.binary_search(&data) {
            Ok(_) => Err(data),
            Err(i) => {
                self.internal.insert(i, data);
                Ok(())
            }
        }
    }

    fn clear(&mut self) {
        self.internal.clear();
    }

    fn shrink_to_fit(&mut self) {
        self.internal.shrink_to_fit();
    }
}

pub fn query_sensors(state: &mut QueryResult, context: &GlobalContext) -> Result<(), Box<dyn Error>> {
    state.clear();

    let res = lm_sensors::query_sensors(state)
        .and(nvidia::query_sensors(state))
        .and(nvml::query_sensors(state, context));

    // Unfortunately the nature of the data structure makes it impossible to estimate the capacity per category.
    // However! If the QueryResult is reused (as it should be), there is unlikely to be any change in size after the first call.
    state.shrink_to_fit();
    res
}
impl Default for SensorStorage {
    fn default() -> Self {
        Self::new()
    }
}