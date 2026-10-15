//! A persistent ordered map, the store's (docs/architecture.md, decision
//! 7): a B-tree whose nodes are shared between versions behind `Arc`.
//! Cloning a map is one pointer; a write copies the path to what it
//! changes, but only the nodes another version still shares: a node no
//! other version holds is changed in place. So a transaction's snapshot
//! costs nothing to take, and a statement that keeps its changes copies
//! a few nodes, not the model.
//!
//! The deletion is the textbook's, top-down: a node is given a key to
//! spare, by a sibling or by a merge, before the descent enters it.

use std::fmt;
use std::sync::Arc;

/// The minimum degree: a node holds `T - 1` to `2T - 1` entries, the root
/// as few as one.
const T: usize = 8;
const MAX: usize = 2 * T - 1;

#[derive(Clone)]
struct Node<K, V> {
    keys: Vec<K>,
    vals: Vec<V>,
    /// None in a leaf; one more than the keys elsewhere.
    children: Vec<Arc<Node<K, V>>>,
}

impl<K, V> Node<K, V> {
    fn leaf(&self) -> bool {
        self.children.is_empty()
    }
}

pub struct PMap<K, V> {
    root: Option<Arc<Node<K, V>>>,
    len: usize,
    /// Entries copied because another version shared their node.
    copied: u64,
}

impl<K, V> Clone for PMap<K, V> {
    fn clone(&self) -> Self {
        PMap {
            root: self.root.clone(),
            len: self.len,
            copied: self.copied,
        }
    }
}

impl<K, V> Default for PMap<K, V> {
    fn default() -> Self {
        PMap {
            root: None,
            len: 0,
            copied: 0,
        }
    }
}

/// The node, to change: copied first if another version shares it.
fn cow<'a, K: Clone, V: Clone>(n: &'a mut Arc<Node<K, V>>, copied: &mut u64) -> &'a mut Node<K, V> {
    if Arc::get_mut(n).is_none() {
        *copied += n.keys.len() as u64;
    }
    Arc::make_mut(n)
}

/// The node's contents, to take: copied if another version shares it.
fn take<K: Clone, V: Clone>(n: Arc<Node<K, V>>, copied: &mut u64) -> Node<K, V> {
    Arc::try_unwrap(n).unwrap_or_else(|n| {
        *copied += n.keys.len() as u64;
        (*n).clone()
    })
}

impl<K: Ord + Clone, V: Clone> PMap<K, V> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Entries this map, and the versions it comes from, copied on write.
    pub fn copied(&self) -> u64 {
        self.copied
    }

    /// Goes back to `older`, a version this one comes from, keeping the
    /// count of what was copied in between.
    pub fn restore(&mut self, older: PMap<K, V>) {
        let copied = self.copied;
        *self = older;
        self.copied = copied;
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        let mut node = self.root.as_deref()?;
        loop {
            match node.keys.binary_search(key) {
                Ok(i) => return Some(&node.vals[i]),
                Err(_) if node.leaf() => return None,
                Err(i) => node = &node.children[i],
            }
        }
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.get(key).is_some()
    }

    /// The value, to change in place; copies its path only if it is there.
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        if !self.contains_key(key) {
            return None;
        }
        let PMap { root, copied, .. } = self;
        let mut node = cow(root.as_mut()?, copied);
        loop {
            match node.keys.binary_search(key) {
                Ok(i) => return Some(&mut node.vals[i]),
                Err(_) if node.leaf() => return None,
                Err(i) => node = cow(&mut node.children[i], copied),
            }
        }
    }

    /// Sets `key` to `val`; the value it had, if any.
    pub fn insert(&mut self, key: K, val: V) -> Option<V> {
        if self.root.is_none() {
            self.root = Some(Arc::new(Node {
                keys: vec![key],
                vals: vec![val],
                children: Vec::new(),
            }));
            self.len = 1;
            return None;
        }
        let PMap { root, copied, .. } = self;
        let root = root.as_mut().unwrap();
        if root.keys.len() == MAX {
            let old = root.clone();
            *root = Arc::new(Node {
                keys: Vec::new(),
                vals: Vec::new(),
                children: vec![old],
            });
            split(cow(root, copied), 0, copied);
        }
        let old = insert(cow(root, copied), key, val, copied);
        if old.is_none() {
            self.len += 1;
        }
        old
    }

    /// Removes `key`; the value it had, if any.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        if !self.contains_key(key) {
            return None;
        }
        let PMap { root, copied, .. } = self;
        let node = cow(root.as_mut()?, copied);
        let old = remove(node, key, copied);
        if node.keys.is_empty() {
            let child = node.children.pop();
            *root = child;
        }
        self.len -= 1;
        old
    }

    /// Keeps the entries `keep` accepts. Only the paths to those removed
    /// are copied.
    pub fn retain(&mut self, mut keep: impl FnMut(&K, &V) -> bool) {
        let gone: Vec<K> = (self.iter())
            .filter(|(k, v)| !keep(k, v))
            .map(|(k, _)| k.clone())
            .collect();
        for k in &gone {
            self.remove(k);
        }
    }

    /// The entries in key order.
    pub fn iter(&self) -> Iter<'_, K, V> {
        let mut it = Iter {
            stack: Vec::new(),
            left: self.len,
        };
        if let Some(root) = &self.root {
            it.descend(root);
        }
        it
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.iter().map(|(k, _)| k)
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.iter().map(|(_, v)| v)
    }
}

/// Splits the full child `i` of `node` around its median, which moves up.
fn split<K: Clone, V: Clone>(node: &mut Node<K, V>, i: usize, copied: &mut u64) {
    let child = cow(&mut node.children[i], copied);
    let keys = child.keys.split_off(T);
    let vals = child.vals.split_off(T);
    let children = if child.leaf() {
        Vec::new()
    } else {
        child.children.split_off(T)
    };
    let (k, v) = (child.keys.pop().unwrap(), child.vals.pop().unwrap());
    node.keys.insert(i, k);
    node.vals.insert(i, v);
    let right = Node {
        keys,
        vals,
        children,
    };
    node.children.insert(i + 1, Arc::new(right));
}

/// Inserts into a node that is not full.
fn insert<K: Ord + Clone, V: Clone>(
    mut node: &mut Node<K, V>,
    key: K,
    val: V,
    copied: &mut u64,
) -> Option<V> {
    loop {
        let mut i = match node.keys.binary_search(&key) {
            Ok(i) => return Some(std::mem::replace(&mut node.vals[i], val)),
            Err(i) => i,
        };
        if node.leaf() {
            node.keys.insert(i, key);
            node.vals.insert(i, val);
            return None;
        }
        if node.children[i].keys.len() == MAX {
            split(node, i, copied);
            match key.cmp(&node.keys[i]) {
                std::cmp::Ordering::Equal => {
                    return Some(std::mem::replace(&mut node.vals[i], val));
                }
                std::cmp::Ordering::Greater => i += 1,
                std::cmp::Ordering::Less => {}
            }
        }
        node = cow(&mut node.children[i], copied);
    }
}

/// Removes `key` from the subtree of `node`, which holds at least `T`
/// entries, or is the root.
fn remove<K: Ord + Clone, V: Clone>(node: &mut Node<K, V>, key: &K, copied: &mut u64) -> Option<V> {
    match node.keys.binary_search(key) {
        Ok(i) if node.leaf() => {
            node.keys.remove(i);
            Some(node.vals.remove(i))
        }
        Ok(i) => {
            if node.children[i].keys.len() >= T {
                let (k, v) = remove_last(cow(&mut node.children[i], copied), copied);
                node.keys[i] = k;
                Some(std::mem::replace(&mut node.vals[i], v))
            } else if node.children[i + 1].keys.len() >= T {
                let (k, v) = remove_first(cow(&mut node.children[i + 1], copied), copied);
                node.keys[i] = k;
                Some(std::mem::replace(&mut node.vals[i], v))
            } else {
                merge(node, i, copied);
                remove(cow(&mut node.children[i], copied), key, copied)
            }
        }
        Err(_) if node.leaf() => None,
        Err(i) => {
            let i = fill(node, i, copied);
            remove(cow(&mut node.children[i], copied), key, copied)
        }
    }
}

fn remove_last<K: Clone, V: Clone>(node: &mut Node<K, V>, copied: &mut u64) -> (K, V) {
    if node.leaf() {
        return (node.keys.pop().unwrap(), node.vals.pop().unwrap());
    }
    let i = fill(node, node.children.len() - 1, copied);
    remove_last(cow(&mut node.children[i], copied), copied)
}

fn remove_first<K: Clone, V: Clone>(node: &mut Node<K, V>, copied: &mut u64) -> (K, V) {
    if node.leaf() {
        return (node.keys.remove(0), node.vals.remove(0));
    }
    let i = fill(node, 0, copied);
    remove_first(cow(&mut node.children[i], copied), copied)
}

/// Gives child `i` at least `T` entries, from a sibling or by a merge;
/// where the entries it covered now are.
fn fill<K: Clone, V: Clone>(node: &mut Node<K, V>, i: usize, copied: &mut u64) -> usize {
    if node.children[i].keys.len() >= T {
        return i;
    }
    if i > 0 && node.children[i - 1].keys.len() >= T {
        // The left sibling's last entry goes up, the separator down.
        let left = cow(&mut node.children[i - 1], copied);
        let (k, v) = (left.keys.pop().unwrap(), left.vals.pop().unwrap());
        let c = left.children.pop();
        let k = std::mem::replace(&mut node.keys[i - 1], k);
        let v = std::mem::replace(&mut node.vals[i - 1], v);
        let child = cow(&mut node.children[i], copied);
        child.keys.insert(0, k);
        child.vals.insert(0, v);
        if let Some(c) = c {
            child.children.insert(0, c);
        }
        i
    } else if i + 1 < node.children.len() && node.children[i + 1].keys.len() >= T {
        // The right sibling's first entry goes up, the separator down.
        let right = cow(&mut node.children[i + 1], copied);
        let (k, v) = (right.keys.remove(0), right.vals.remove(0));
        let c = (!right.leaf()).then(|| right.children.remove(0));
        let k = std::mem::replace(&mut node.keys[i], k);
        let v = std::mem::replace(&mut node.vals[i], v);
        let child = cow(&mut node.children[i], copied);
        child.keys.push(k);
        child.vals.push(v);
        if let Some(c) = c {
            child.children.push(c);
        }
        i
    } else if i + 1 < node.children.len() {
        merge(node, i, copied);
        i
    } else {
        merge(node, i - 1, copied);
        i - 1
    }
}

/// Merges child `i + 1` and the separator between them into child `i`.
fn merge<K: Clone, V: Clone>(node: &mut Node<K, V>, i: usize, copied: &mut u64) {
    let right = take(node.children.remove(i + 1), copied);
    let (k, v) = (node.keys.remove(i), node.vals.remove(i));
    let left = cow(&mut node.children[i], copied);
    left.keys.push(k);
    left.vals.push(v);
    left.keys.extend(right.keys);
    left.vals.extend(right.vals);
    left.children.extend(right.children);
}

/// The entries of a map in key order.
pub struct Iter<'a, K, V> {
    /// The nodes on the way down, each with its next entry.
    stack: Vec<(&'a Node<K, V>, usize)>,
    left: usize,
}

impl<'a, K, V> Iter<'a, K, V> {
    fn descend(&mut self, mut node: &'a Node<K, V>) {
        loop {
            self.stack.push((node, 0));
            match node.children.first() {
                Some(c) => node = c,
                None => break,
            }
        }
    }
}

impl<'a, K, V> Iterator for Iter<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (node, i) = self.stack.last_mut()?;
            let node: &'a Node<K, V> = node;
            if *i == node.keys.len() {
                self.stack.pop();
                continue;
            }
            let item = (&node.keys[*i], &node.vals[*i]);
            *i += 1;
            let next = *i;
            if !node.leaf() {
                self.descend(&node.children[next]);
            }
            self.left -= 1;
            return Some(item);
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.left, Some(self.left))
    }
}

impl<'a, K: Ord + Clone, V: Clone> IntoIterator for &'a PMap<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = Iter<'a, K, V>;

    fn into_iter(self) -> Iter<'a, K, V> {
        self.iter()
    }
}

impl<K: Ord + Clone, V: Clone> std::ops::Index<&K> for PMap<K, V> {
    type Output = V;

    fn index(&self, key: &K) -> &V {
        self.get(key).expect("no entry for this key")
    }
}

impl<K: Ord + Clone + fmt::Debug, V: Clone + fmt::Debug> fmt::Debug for PMap<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

/// Equal entries; what was copied to get there does not count.
impl<K: Ord + Clone, V: Clone + PartialEq> PartialEq for PMap<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len && self.iter().eq(other.iter())
    }
}

impl<K: Ord + Clone, V: Clone> FromIterator<(K, V)> for PMap<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(entries: I) -> Self {
        let mut map = PMap::new();
        for (k, v) in entries {
            map.insert(k, v);
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// The B-tree's invariants: keys sorted within and across nodes, every
    /// node but the root between `T - 1` and `MAX` entries, an internal
    /// node one child more than its keys, every leaf at one depth. The
    /// number of entries.
    fn check<K: Ord + Clone + fmt::Debug, V: Clone>(map: &PMap<K, V>) {
        fn walk<K: Ord + fmt::Debug, V>(
            n: &Node<K, V>,
            root: bool,
            lo: Option<&K>,
            hi: Option<&K>,
            depth: usize,
            leaves: &mut Option<usize>,
        ) -> usize {
            assert_eq!(n.keys.len(), n.vals.len());
            assert!(n.keys.len() <= MAX, "a node over full");
            assert!(root || n.keys.len() >= T - 1, "a node under half");
            assert!(!n.keys.is_empty(), "an empty node");
            assert!(n.keys.windows(2).all(|w| w[0] < w[1]), "keys out of order");
            assert!(lo.is_none_or(|lo| lo < &n.keys[0]));
            assert!(hi.is_none_or(|hi| n.keys.last().unwrap() < hi));
            if n.leaf() {
                assert_eq!(*leaves.get_or_insert(depth), depth, "leaves at two depths");
                return n.keys.len();
            }
            assert_eq!(n.children.len(), n.keys.len() + 1);
            let mut count = n.keys.len();
            for (i, c) in n.children.iter().enumerate() {
                let lo = if i == 0 { lo } else { Some(&n.keys[i - 1]) };
                let hi = n.keys.get(i).or(hi);
                count += walk(c, false, lo, hi, depth + 1, leaves);
            }
            count
        }
        let count = map
            .root
            .as_deref()
            .map_or(0, |r| walk(r, true, None, None, 0, &mut None));
        assert_eq!(count, map.len, "len is not the number of entries");
    }

    fn generator(mut seed: u64) -> impl FnMut(u64) -> u64 {
        move |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        }
    }

    fn same(map: &PMap<u64, u64>, model: &BTreeMap<u64, u64>) {
        check(map);
        assert_eq!(map.len(), model.len());
        assert!(map.iter().eq(model.iter()), "the entries differ");
    }

    /// Random inserts, removes, gets and changes, against `BTreeMap`, over
    /// key ranges that keep the tree small, grow it to three levels, and
    /// shrink it again.
    #[test]
    fn like_a_btreemap() {
        let mut next = generator(0x2545_F491_4F6C_DD1D);
        for range in [8, 64, 1000, 5000] {
            let mut map = PMap::new();
            let mut model = BTreeMap::new();
            for step in 0..20_000u64 {
                let k = next(range);
                // More inserts first, then more removes: the tree grows,
                // then empties.
                let insert = if step < 10_000 { 6 } else { 3 };
                match next(10) {
                    n if n < insert => assert_eq!(map.insert(k, step), model.insert(k, step)),
                    n if n < 9 => assert_eq!(map.remove(&k), model.remove(&k)),
                    _ => {
                        assert_eq!(map.get(&k), model.get(&k));
                        if let (Some(a), Some(b)) = (map.get_mut(&k), model.get_mut(&k)) {
                            *a += 1;
                            *b += 1;
                        }
                    }
                }
                if step % 97 == 0 {
                    same(&map, &model);
                }
            }
            same(&map, &model);
            map.retain(|k, _| k % 3 == 0);
            model.retain(|k, _| k % 3 == 0);
            same(&map, &model);
        }
    }

    /// Versions kept along the way read the same after later writes to
    /// the newest, and writes to an old one leave the others alone.
    #[test]
    fn versions_are_independent() {
        let mut next = generator(0x9E37_79B9_7F4A_7C15);
        let mut map = PMap::new();
        let mut model = BTreeMap::new();
        let mut kept = Vec::new();
        for step in 0..30_000u64 {
            let k = next(3000);
            if next(3) == 0 {
                map.remove(&k);
                model.remove(&k);
            } else {
                map.insert(k, step);
                model.insert(k, step);
            }
            if step % 1500 == 0 {
                kept.push((map.clone(), model.clone()));
            }
        }
        for (old, old_model) in &kept {
            same(old, old_model);
        }
        let (mut old, mut old_model) = kept[5].clone();
        for k in 0..3000 {
            if k % 2 == 0 {
                old.remove(&k);
                old_model.remove(&k);
            } else {
                old.insert(k, 0);
                old_model.insert(k, 0);
            }
        }
        same(&old, &old_model);
        same(&map, &model);
        for (v, m) in &kept {
            same(v, m);
        }
    }

    /// A write to a map no other version shares copies nothing; with a
    /// version kept, it copies one path, and a restore keeps the count.
    #[test]
    fn copies_only_what_is_shared() {
        let mut map: PMap<u64, u64> = (0..10_000).map(|k| (k, k)).collect();
        assert_eq!(map.copied(), 0);
        map.insert(5, 0);
        map.remove(&7);
        assert_eq!(map.copied(), 0, "nothing shared, nothing copied");
        let before = map.clone();
        map.insert(5, 1);
        let path = map.copied();
        assert!(
            path > 0 && path <= 6 * MAX as u64,
            "{path} entries copied for one write"
        );
        map.insert(6, 1);
        assert_eq!(map.copied(), path, "the path is this version's now");
        map.restore(before);
        assert_eq!(map.get(&5), Some(&0));
        assert_eq!(map.copied(), path);
    }

    #[test]
    fn missing_keys_copy_nothing() {
        let mut map: PMap<u64, u64> = (0..1000).map(|k| (k * 2, k)).collect();
        let _kept = map.clone();
        assert_eq!(map.get_mut(&3), None);
        assert_eq!(map.remove(&3), None);
        assert_eq!(map.copied(), 0);
        assert_eq!(map[&4], 2);
    }
}
