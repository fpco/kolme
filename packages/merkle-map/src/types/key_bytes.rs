use get_size2::GetSize;

/// A key used in a [MerkleMap].
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Default, GetSize)]
pub struct MerkleKey(
    #[get_size(size = 32)]
    tinyvec::TinyVec<[u8; 32]>
);

impl MerkleKey {
    pub fn from_slice(slice: &[u8]) -> Self {
        MerkleKey(slice.into())
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub(crate) fn get_index_for_depth(&self, depth: u16) -> Option<u8> {
        // First get the byte in question...
        let byte = *self.0.get(usize::from(depth / 2))?;
        // Then get either the high or low bits, depending on whether our depth is even or odd
        Some(if depth % 2 == 0 {
            byte >> 4
        } else {
            byte & 0x0F
        })
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use super::*;

    quickcheck::quickcheck! {
        // Use minimal implementations inside the macro so we get easier to read error messages during dev.
        fn ordering(xorig: Vec<u8>, yorig: Vec<u8>) -> bool {
            ordering_test(xorig, yorig)
        }
    }

    fn ordering_test(xorig: Vec<u8>, yorig: Vec<u8>) -> bool {
        let x = MerkleKey::from_slice(&xorig);
        let y = MerkleKey::from_slice(&yorig);
        assert_eq!(xorig.cmp(&yorig), x.cmp(&y));

        let expected = x.cmp(&y);
        for i in 0.. {
            let xi = x.get_index_for_depth(i);
            let yi = y.get_index_for_depth(i);
            let (xi, yi) = match (xi, yi) {
                (None, None) => {
                    assert_eq!(xorig, yorig);
                    return true;
                }
                (Some(_), None) => {
                    assert!(xorig > yorig);
                    return true;
                }
                (None, Some(_)) => {
                    assert!(xorig < yorig);
                    return true;
                }
                (Some(xi), Some(yi)) => (xi, yi),
            };
            match xi.cmp(&yi) {
                Ordering::Equal => (),
                cmp => {
                    assert_eq!(
                        cmp, expected,
                        "Comparing {xorig:?} vs {yorig:?}, failing on index {i}, {xi} vs {yi}",
                    );
                    return true;
                }
            }
        }
        false
    }
}
