//! Typed, append-only arenas for things that are never removed (star
//! systems, hyperlanes). Things that can die (factions, ships, fleets) use
//! generational keys from `slotmap` instead.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ops::{Index, IndexMut};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A typed index into an [`Arena<T>`]. Cheap to copy, and an `Id<StarSystem>`
/// can't be confused with an `Id<Hyperlane>`.
pub struct Id<T> {
    index: u32,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Id<T> {
    pub const fn from_index(index: u32) -> Self {
        Self {
            index,
            _marker: PhantomData,
        }
    }

    pub const fn index(self) -> usize {
        self.index as usize
    }
}

// Manual impls so `T` doesn't need to implement these traits itself.
impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Id<T> {}
impl<T> PartialEq for Id<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}
impl<T> Eq for Id<T> {}
impl<T> PartialOrd for Id<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl<T> Ord for Id<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.index.cmp(&other.index)
    }
}
impl<T> Hash for Id<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.index.hash(state);
    }
}
impl<T> fmt::Debug for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let full = std::any::type_name::<T>();
        let short = full.rsplit("::").next().unwrap_or(full);
        write!(f, "{short}#{}", self.index)
    }
}
impl<T> Serialize for Id<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(self.index)
    }
}
impl<'de, T> Deserialize<'de> for Id<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        u32::deserialize(deserializer).map(Self::from_index)
    }
}

/// Dense, append-only storage addressed by [`Id<T>`].
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Arena<T> {
    items: Vec<T>,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self { items: Vec::new() }
    }
}

impl<T> Arena<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            items: Vec::with_capacity(capacity),
        }
    }

    pub fn push(&mut self, item: T) -> Id<T> {
        let index = u32::try_from(self.items.len()).expect("arena exceeded u32::MAX items");
        self.items.push(item);
        Id::from_index(index)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, id: Id<T>) -> Option<&T> {
        self.items.get(id.index())
    }

    pub fn get_mut(&mut self, id: Id<T>) -> Option<&mut T> {
        self.items.get_mut(id.index())
    }

    pub fn ids(&self) -> impl ExactSizeIterator<Item = Id<T>> + use<T> {
        (0..self.items.len() as u32).map(Id::from_index)
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = (Id<T>, &T)> {
        self.items
            .iter()
            .enumerate()
            .map(|(i, item)| (Id::from_index(i as u32), item))
    }

    pub fn iter_mut(&mut self) -> impl ExactSizeIterator<Item = (Id<T>, &mut T)> {
        self.items
            .iter_mut()
            .enumerate()
            .map(|(i, item)| (Id::from_index(i as u32), item))
    }

    /// The underlying items, in id order — useful for parallel iteration.
    pub fn as_slice(&self) -> &[T] {
        &self.items
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.items
    }
}

impl<T> Index<Id<T>> for Arena<T> {
    type Output = T;

    fn index(&self, id: Id<T>) -> &T {
        &self.items[id.index()]
    }
}

impl<T> IndexMut<Id<T>> for Arena<T> {
    fn index_mut(&mut self, id: Id<T>) -> &mut T {
        &mut self.items[id.index()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_index() {
        let mut arena = Arena::new();
        let a = arena.push("a");
        let b = arena.push("b");
        assert_eq!(arena[a], "a");
        assert_eq!(arena[b], "b");
        assert_eq!(arena.len(), 2);
        assert_eq!(arena.ids().collect::<Vec<_>>(), vec![a, b]);
    }

    #[test]
    fn ids_serialize_as_plain_numbers() {
        let id: Id<String> = Id::from_index(7);
        assert_eq!(serde_json::to_string(&id).unwrap(), "7");
        let back: Id<String> = serde_json::from_str("7").unwrap();
        assert_eq!(back, id);
    }
}
