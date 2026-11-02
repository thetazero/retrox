//! A generational pool for things that come and go (settlements,
//! factions, companies, colony ships), with a free list that's saved with
//! the game.
//!
//! `slotmap::SlotMap` doesn't serialize the order of its free list, so
//! after loading a save the next insert can land in a different slot and
//! get a different key than it would have, and history keyed by entity
//! diverges (#62). This pool reuses slots last-in-first-out from a free
//! list that is part of its saved state.
//!
//! Keys are ordinary `slotmap` key types with the same bit layout (index in
//! the low 32 bits, version in the high 32, odd while occupied), so
//! `slotmap::SecondaryMap` works with them unchanged.

use std::marker::PhantomData;
use std::ops::{Index, IndexMut};

use serde::{Deserialize, Serialize};
use slotmap::{Key, KeyData};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Slot<V> {
    /// Odd while occupied, even while free.
    version: u32,
    value: Option<V>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(bound(serialize = "V: Serialize", deserialize = "V: Deserialize<'de>"))]
pub struct Pool<K: Key, V> {
    slots: Vec<Slot<V>>,
    /// Free slot indices; the last one is reused first.
    free: Vec<u32>,
    len: usize,
    #[serde(skip)]
    _key: PhantomData<fn() -> K>,
}

impl<K: Key, V> Default for Pool<K, V> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            len: 0,
            _key: PhantomData,
        }
    }
}

fn key<K: Key>(index: u32, version: u32) -> K {
    KeyData::from_ffi((u64::from(version) << 32) | u64::from(index)).into()
}

fn parts<K: Key>(key: K) -> (usize, u32) {
    let ffi = key.data().as_ffi();
    ((ffi & 0xFFFF_FFFF) as usize, (ffi >> 32) as u32)
}

impl<K: Key, V> Pool<K, V> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn insert(&mut self, value: V) -> K {
        self.len += 1;
        match self.free.pop() {
            Some(index) => {
                let slot = &mut self.slots[index as usize];
                slot.version = slot.version.wrapping_add(1) | 1;
                slot.value = Some(value);
                key(index, slot.version)
            }
            None => {
                let index = self.slots.len() as u32;
                self.slots.push(Slot {
                    version: 1,
                    value: Some(value),
                });
                key(index, 1)
            }
        }
    }

    pub fn remove(&mut self, key: K) -> Option<V> {
        let (index, version) = parts(key);
        let slot = self.slots.get_mut(index)?;
        if slot.version != version || slot.value.is_none() {
            return None;
        }
        slot.version = slot.version.wrapping_add(1);
        self.free.push(index as u32);
        self.len -= 1;
        slot.value.take()
    }

    pub fn get(&self, key: K) -> Option<&V> {
        let (index, version) = parts(key);
        let slot = self.slots.get(index)?;
        (slot.version == version)
            .then_some(slot.value.as_ref())
            .flatten()
    }

    pub fn get_mut(&mut self, key: K) -> Option<&mut V> {
        let (index, version) = parts(key);
        let slot = self.slots.get_mut(index)?;
        (slot.version == version)
            .then_some(slot.value.as_mut())
            .flatten()
    }

    pub fn contains_key(&self, key: K) -> bool {
        self.get(key).is_some()
    }

    /// Occupied entries, in slot order.
    pub fn iter(&self) -> impl Iterator<Item = (K, &V)> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.value.as_ref().map(|v| (key(i as u32, s.version), v)))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (K, &mut V)> + '_ {
        self.slots.iter_mut().enumerate().filter_map(|(i, s)| {
            let version = s.version;
            s.value.as_mut().map(|v| (key(i as u32, version), v))
        })
    }

    pub fn keys(&self) -> impl Iterator<Item = K> + '_ {
        self.iter().map(|(k, _)| k)
    }

    pub fn values(&self) -> impl Iterator<Item = &V> + '_ {
        self.slots.iter().filter_map(|s| s.value.as_ref())
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> + '_ {
        self.slots.iter_mut().filter_map(|s| s.value.as_mut())
    }
}

impl<K: Key, V> Index<K> for Pool<K, V> {
    type Output = V;

    fn index(&self, key: K) -> &V {
        self.get(key).expect("invalid pool key")
    }
}

impl<K: Key, V> IndexMut<K> for Pool<K, V> {
    fn index_mut(&mut self, key: K) -> &mut V {
        self.get_mut(key).expect("invalid pool key")
    }
}

impl<'a, K: Key, V> IntoIterator for &'a Pool<K, V> {
    type Item = (K, &'a V);
    type IntoIter = Box<dyn Iterator<Item = (K, &'a V)> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

impl<'a, K: Key, V> IntoIterator for &'a mut Pool<K, V> {
    type Item = (K, &'a mut V);
    type IntoIter = Box<dyn Iterator<Item = (K, &'a mut V)> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter_mut())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    slotmap::new_key_type! { struct TestKey; }

    #[test]
    fn reuse_survives_a_save() {
        let mut pool: Pool<TestKey, u32> = Pool::new();
        let keys: Vec<TestKey> = (0..6).map(|i| pool.insert(i)).collect();
        pool.remove(keys[4]);
        pool.remove(keys[1]);
        pool.remove(keys[3]);
        let json = serde_json::to_string(&pool).unwrap();
        let mut loaded: Pool<TestKey, u32> = serde_json::from_str(&json).unwrap();
        for v in 10..14 {
            assert_eq!(pool.insert(v), loaded.insert(v));
        }
        assert_eq!(pool.len(), loaded.len());
    }

    #[test]
    fn stale_keys_miss() {
        let mut pool: Pool<TestKey, &str> = Pool::new();
        let a = pool.insert("a");
        pool.remove(a);
        let b = pool.insert("b");
        assert_ne!(a, b);
        assert!(pool.get(a).is_none());
        assert_eq!(pool[b], "b");
        let mut secondary = slotmap::SecondaryMap::new();
        secondary.insert(b, 1);
        assert_eq!(secondary.get(b), Some(&1));
        assert_eq!(secondary.get(a), None);
    }
}
