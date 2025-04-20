use crate::*;

impl<'a, K, V> IntoIterator for &'a Node<K, V> {
    type Item = (&'a K, &'a V);

    type IntoIter = Iter<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        Iter {
            stack: vec![to_iter_layer(self)],
        }
    }
}

impl<K, V> Node<K, V> {
    pub(crate) fn iter(&self) -> Iter<K, V> {
        self.into_iter()
    }
}

impl<'a, K, V> IntoIterator for &'a MerkleMap<K, V> {
    type Item = (&'a K, &'a V);

    type IntoIter = Iter<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        Iter {
            stack: vec![to_iter_layer(&self.0)],
        }
    }
}

impl<'a, K, V> Iterator for Iter<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        'outer: loop {
            let last = self.stack.last_mut()?;
            match last {
                IterLayer::Leaf(leaf, idx_ref) => {
                    let idx = (*idx_ref).unwrap_or(0);
                    match leaf.values.get(usize::from(idx)) {
                        Some(entry) => {
                            *idx_ref = Some(idx + 1);
                            return Some((&entry.key, &entry.value));
                        }
                        None => {
                            self.stack.pop();
                        }
                    }
                }
                IterLayer::Tree(tree, idx_ref) => {
                    let mut idx = (*idx_ref).unwrap_or(0);
                    if idx == 0 {
                        *idx_ref = Some(1);
                        if let Some(entry) = &tree.leaf {
                            return Some((&entry.key, &entry.value));
                        } else {
                            idx = 1;
                        }
                    }
                    if idx < 17 {
                        let entry = to_iter_layer(&tree.branches[usize::from(idx - 1)]);
                        *idx_ref = Some(idx + 1);
                        self.stack.push(entry);
                        continue 'outer;
                    }
                    self.stack.pop();
                }
            }
        }
    }
}

impl<K, V> DoubleEndedIterator for Iter<'_, K, V> {
    fn next_back(&mut self) -> Option<Self::Item> {
        'outer: loop {
            let last = self.stack.last_mut()?;
            match last {
                IterLayer::Leaf(leaf, idx_ref) => {
                    let mut idx = (*idx_ref).unwrap_or(leaf.values.len() as u16);
                    if idx == 0 {
                        self.stack.pop();
                        continue 'outer;
                    }

                    idx -= 1;

                    let entry = &leaf.values[usize::from(idx)];
                    *idx_ref = Some(idx);
                    return Some((&entry.key, &entry.value));
                }
                IterLayer::Tree(tree, idx_ref) => {
                    let idx = (*idx_ref).unwrap_or(16);

                    if idx > 0 {
                        let entry = to_iter_layer(&tree.branches[usize::from(idx - 1)]);
                        *idx_ref = Some(idx - 1);
                        self.stack.push(entry);
                        continue 'outer;
                    }

                    assert_eq!(idx, 0);

                    if let Some(entry) = &tree.leaf {
                        self.stack.pop();
                        return Some((&entry.key, &entry.value));
                    }
                    self.stack.pop();
                }
            }
        }
    }
}

pub struct Iter<'a, K, V> {
    stack: Vec<IterLayer<'a, K, V>>,
}

enum IterLayer<'a, K, V> {
    Leaf(&'a LeafContents<K, V>, Option<u16>),
    Tree(&'a TreeContents<K, V>, Option<u16>),
}

impl<K, V> std::fmt::Debug for IterLayer<'_, K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            IterLayer::Leaf(_, x) => write!(f, "Leaf({x:?})"),
            IterLayer::Tree(_, x) => write!(f, "Tree({x:?})"),
        }
    }
}

fn to_iter_layer<K, V>(node: &Node<K, V>) -> IterLayer<K, V> {
    match node {
        Node::Leaf(leaf) => IterLayer::Leaf(leaf.as_ref(), None),
        Node::Tree(tree) => IterLayer::Tree(tree.as_ref(), None),
    }
}

pub struct IntoIter<K, V>(Node<K, V>);

impl<K: Clone, V: Clone> IntoIterator for MerkleMap<K, V> {
    type Item = (K, V);

    type IntoIter = IntoIter<K, V>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter(self.0)
    }
}

impl<K: Clone, V: Clone> Iterator for IntoIter<K, V> {
    type Item = (K, V);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.pop_first()
    }
}

impl<K, V> Node<K, V>
where
    K: Clone,
    V: Clone,
{
    fn pop_first(&mut self) -> Option<(K, V)> {
        match self {
            Node::Leaf(leaf) => {
                let leaf = leaf.as_mut();
                if leaf.values.is_empty() {
                    None
                } else {
                    let entry = leaf.values.remove(0);
                    Some((entry.key, entry.value))
                }
            }
            Node::Tree(tree) => {
                let tree = tree.as_mut();
                if let Some(entry) = tree.leaf.take() {
                    return Some((entry.key, entry.value));
                }

                for branch in &mut tree.branches {
                    if let Some(pair) = branch.pop_first() {
                        return Some(pair);
                    }
                }
                None
            }
        }
    }
}

impl<T> Bound<T> {
    fn map<U, F: FnOnce(T) -> U>(self, f: F) -> Bound<U> {
        match self {
            Bound::Inclusive(x) => Bound::Inclusive(f(x)),
            Bound::Exclusive(x) => Bound::Exclusive(f(x)),
        }
    }
}

impl<'a, K: ToMerkleKey, V> Node<K, V> {
    pub(crate) fn range(
        &'a self,
        lower: Option<Bound<K>>,
        upper: Option<Bound<K>>,
        order: Order,
    ) -> Range<'a, K, V> {
        fn to_key<K: ToMerkleKey>(x: Option<Bound<K>>) -> Option<Bound<MerkleKey>> {
            x.map(|bound| bound.map(|k| k.to_merkle_key()))
        }

        let lower = to_key(lower);
        let upper = to_key(upper);

        let (start, stop) = match order {
            Order::Asc => (lower, upper),
            Order::Desc => (upper, lower),
        };

        Range {
            iter: match &start {
                None => self.iter(),
                Some(start) => self.iter_from(start, order),
            },
            order,
            start,
            stop,
        }
    }

    fn iter_from(&'a self, start: &Bound<MerkleKey>, order: Order) -> Iter<'a, K, V> {
        match order {
            Order::Asc => self.iter_from_asc(start),
            Order::Desc => self.iter_from_desc(start),
        }
    }

    fn iter_from_asc(&'a self, start: &Bound<MerkleKey>) -> Iter<'a, K, V> {
        let start = match start {
            Bound::Inclusive(start) => start,
            Bound::Exclusive(start) => start,
        };
        let mut curr = self;
        let mut stack = vec![];
        let mut depth = 0;
        loop {
            match curr {
                Node::Leaf(lockable) => {
                    stack.push(IterLayer::Leaf(lockable.as_ref(), None));
                    break;
                }
                Node::Tree(lockable) => {
                    let tree = lockable.as_ref();
                    let branch = match start.get_index_for_depth(depth) {
                        Some(branch) => branch,
                        None => {
                            stack.push(IterLayer::Tree(tree, None));
                            break;
                        }
                    };
                    depth += 1;
                    stack.push(IterLayer::Tree(tree, Some((branch + 2).into())));
                    curr = &tree.branches[usize::from(branch)];
                }
            }
        }
        Iter { stack }
    }

    fn iter_from_desc(&'a self, start: &Bound<MerkleKey>) -> Iter<'a, K, V> {
        let start = match start {
            Bound::Inclusive(start) => start,
            Bound::Exclusive(start) => start,
        };
        let mut curr = self;
        let mut stack = vec![];
        let mut depth = 0;
        loop {
            match curr {
                Node::Leaf(lockable) => {
                    stack.push(IterLayer::Leaf(lockable.as_ref(), None));
                    break;
                }
                Node::Tree(lockable) => {
                    let tree = lockable.as_ref();
                    let branch = match start.get_index_for_depth(depth) {
                        Some(branch) => branch,
                        None => {
                            stack.push(IterLayer::Tree(tree, None));
                            break;
                        }
                    };
                    depth += 1;
                    stack.push(IterLayer::Tree(tree, Some((branch).into())));
                    curr = &tree.branches[usize::from(branch)];
                }
            }
        }
        Iter { stack }
    }
}

pub struct Range<'a, K, V> {
    iter: Iter<'a, K, V>,
    order: Order,
    start: Option<Bound<MerkleKey>>,
    stop: Option<Bound<MerkleKey>>,
}

impl<'a, K: ToMerkleKey, V> Iterator for Range<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        // We only check the start once
        let start = self.start.take();
        loop {
            let pair = match self.order {
                Order::Asc => self.iter.next(),
                Order::Desc => self.iter.next_back(),
            }?;
            if let Some(start) = &start {
                let key = pair.0.to_merkle_key();
                let to_use = match (self.order, start) {
                    (Order::Asc, Bound::Inclusive(start)) => start <= &key,
                    (Order::Asc, Bound::Exclusive(start)) => start < &key,
                    (Order::Desc, Bound::Inclusive(start)) => start >= &key,
                    (Order::Desc, Bound::Exclusive(start)) => start > &key,
                };
                if !to_use {
                    continue;
                }
            }

            if let Some(stop) = &self.stop {
                let key = pair.0.to_merkle_key();
                let to_stop = match (self.order, stop) {
                    (Order::Asc, Bound::Inclusive(stop)) => stop < &key,
                    (Order::Asc, Bound::Exclusive(stop)) => stop <= &key,
                    (Order::Desc, Bound::Inclusive(stop)) => stop > &key,
                    (Order::Desc, Bound::Exclusive(stop)) => stop >= &key,
                };
                if to_stop {
                    return None;
                }
            }
            break Some(pair);
        }
    }
}
