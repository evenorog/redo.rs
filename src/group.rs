use std::collections::HashMap;
use std::hash::Hash;
use record::Commands;
use {Command, Error, Record, Stack};

/// A group of either stacks or records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Group<K: Hash + Eq, V> {
    map: HashMap<K, V>,
    active: Option<K>,
}

impl<K: Hash + Eq, V> Group<K, V> {
    /// Returns a new `Group`.
    #[inline]
    pub fn new() -> Group<K, V> {
        Group {
            map: HashMap::new(),
            active: None,
        }
    }

    /// Returns the number of items in the group.
    #[inline]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns `true` if the group is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Inserts an item into the group.
    #[inline]
    pub fn insert(&mut self, k: K, v: V) -> Option<V> {
        self.map.insert(k, v)
    }

    /// Removes an item from the group.
    #[inline]
    pub fn remove(&mut self, k: &K) -> Option<V> {
        self.map.remove(k)
    }

    /// Gets the current active item in the group.
    #[inline]
    pub fn get(&self) -> Option<&V> {
        self.active.as_ref().and_then(|active| self.map.get(active))
    }

    /// Sets the current active item in the group.
    #[inline]
    pub fn set<T: Into<Option<K>>>(&mut self, k: T) -> bool {
        match k.into() {
            Some(ref k) if !self.map.contains_key(k) => false,
            k => {
                self.active = k;
                true
            }
        }
    }
}

impl<K: Hash + Eq, R, C: Command<R>> Group<K, Stack<R, C>> {
    /// Calls the [`push`] method on the active `Stack`.
    ///
    /// [`push`]: stack/struct.Stack.html#method.push
    #[inline]
    pub fn push(&mut self, cmd: C) -> Option<Result<(), Error<R, C>>> {
        let map = &mut self.map;
        self.active
            .as_ref()
            .and_then(|active| map.get_mut(active))
            .map(move |stack| stack.push(cmd))
    }

    /// Calls the [`pop`] method on the active `Stack`.
    ///
    /// [`pop`]: stack/struct.Stack.html#method.pop
    #[inline]
    pub fn pop(&mut self) -> Option<Result<C, Error<R, C>>> {
        let map = &mut self.map;
        self.active
            .as_ref()
            .and_then(|active| map.get_mut(active))
            .and_then(|stack| stack.pop())
    }
}

impl<'a, K: Hash + Eq, R, C: Command<R>> Group<K, Record<'a, R, C>> {
    /// Calls the [`push`] method on the active `Record`.
    ///
    /// [`push`]: record/struct.Record.html#method.push
    #[inline]
    pub fn push(&mut self, cmd: C) -> Option<Result<Commands<C>, Error<R, C>>> {
        let map = &mut self.map;
        self.active
            .as_ref()
            .and_then(|active| map.get_mut(active))
            .map(move |record| record.push(cmd))
    }

    /// Calls the [`redo`] method on the active `Record`.
    ///
    /// [`redo`]: record/struct.Record.html#method.redo
    #[inline]
    pub fn redo(&mut self) -> Option<Result<(), C::Err>> {
        let map = &mut self.map;
        self.active
            .as_ref()
            .and_then(|active| map.get_mut(active))
            .and_then(|record| record.redo())
    }

    /// Calls the [`undo`] method on the active `Record`.
    ///
    /// [`undo`]: record/struct.Record.html#method.undo
    #[inline]
    pub fn undo(&mut self) -> Option<Result<(), C::Err>> {
        let map = &mut self.map;
        self.active
            .as_ref()
            .and_then(|active| map.get_mut(active))
            .and_then(|record| record.undo())
    }
}

impl<K: Hash + Eq, V> Default for Group<K, V> {
    #[inline]
    fn default() -> Group<K, V> {
        Group::new()
    }
}