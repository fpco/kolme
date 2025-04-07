use crate::*;

impl<'a, K, V> IntoIterator for &'a MerkleMap<K, V> {
    type Item = (&'a K, &'a V);

    type IntoIter = Iter<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        Iter {
            stack: match to_iter_layer(&self.0) {
                Some(layer) => vec![layer],
                None => vec![],
            },
        }
    }
}

impl<'a, K, V> Iterator for Iter<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        'outer: loop {
            let last = self.stack.last_mut()?;
            match last {
                IterLayer::Leaf(leaf, idx) => match leaf.values.get(usize::from(*idx)) {
                    Some(entry) => {
                        *idx += 1;
                        return Some((&entry.key, &entry.value));
                    }
                    None => {
                        self.stack.pop();
                    }
                },
                IterLayer::Tree(tree, idx) => {
                    if *idx == 0 {
                        *idx += 1;
                        if let Some(entry) = &tree.leaf {
                            return Some((&entry.key, &entry.value));
                        }
                    }
                    while *idx < 17 {
                        if let Some(entry) = to_iter_layer(&tree.branches[usize::from(*idx - 1)]) {
                            *idx += 1;
                            self.stack.push(entry);
                            continue 'outer;
                        }
                        *idx += 1;
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
    Leaf(&'a LeafContents<K, V>, u16),
    Tree(&'a TreeContents<K, V>, u16),
}

fn to_iter_layer<K, V>(node: &Node<K, V>) -> Option<IterLayer<K, V>> {
    match node {
        Node::Empty => None,
        Node::LockedLeaf(leaf) => Some(IterLayer::Leaf(&leaf.inner, 0)),
        Node::UnlockedLeaf(leaf) => Some(IterLayer::Leaf(leaf, 0)),
        Node::LockedTree(tree) => Some(IterLayer::Tree(&tree.inner, 0)),
        Node::UnlockedTree(tree) => Some(IterLayer::Tree(tree, 0)),
    }
}

pub struct IntoIter<K, V>(UnlockedNode<K, V>);

impl<K: Clone, V: Clone> IntoIterator for MerkleMap<K, V> {
    type Item = (K, V);

    type IntoIter = IntoIter<K, V>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter(self.0.unlock())
    }
}

impl<K: Clone, V: Clone> Iterator for IntoIter<K, V> {
    type Item = (K, V);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.pop_first()
    }
}

impl<K, V> UnlockedNode<K, V>
where
    K: Clone,
    V: Clone,
{
    fn pop_first(&mut self) -> Option<(K, V)> {
        match self {
            UnlockedNode::Leaf(leaf) => {
                if leaf.values.is_empty() {
                    None
                } else {
                    let entry = leaf.values.remove(0);
                    Some((entry.key, entry.value))
                }
            }
            UnlockedNode::Tree(tree) => {
                if let Some(entry) = tree.leaf.take() {
                    return Some((entry.key, entry.value));
                }

                for idx in 0..16 {
                    let mut branch = std::mem::take(&mut tree.branches[idx]).unlock();
                    if let Some(pair) = branch.pop_first() {
                        tree.branches[idx] = branch.into();
                        return Some(pair);
                    }
                }
                None
            }
        }
    }
}
