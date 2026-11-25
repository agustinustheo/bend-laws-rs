//! Uses `bend_laws` the way a Polkadot SDK runtime/pallet crate would.
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;
use alloc::vec::Vec;
use bend_laws::{Bend, bend, bend_type, fold, laws};
use codec::{Decode, Encode};
use scale_info::TypeInfo;

bend_type! {
    #[derive(Clone, Debug, PartialEq, Eq, Encode, Decode, TypeInfo)]
    mod tree: Tree<T> {
        Node { ~left, ~right },
        Leaf { value: T },
    }
}
use tree::{Layer, Tree};

/// Bend: `bend d = 0: when d < depth: Node{fork(d+1), fork(d+1)} else: Leaf{d}`
pub fn grow(depth: u32) -> Tree<u32> {
    bend!(Tree<u32>; d = 0 => if d < depth {
        Layer::Node { left: fork(d + 1), right: fork(d + 1) }
    } else {
        Layer::Leaf { value: d }
    })
}

pub fn sum(t: Tree<u32>) -> u64 {
    fold!(t;
        Layer::Node { left, right } => left + right,
        Layer::Leaf { value } => value as u64,
    )
}

/// Sum of 2^depth leaves without ever allocating the tree.
pub fn sum_fused(depth: u32) -> u64 {
    Tree::<u32>::hylo(
        0u32,
        &mut |d| {
            if d < depth {
                Layer::Node {
                    left: d + 1,
                    right: d + 1,
                }
            } else {
                Layer::Leaf { value: d }
            }
        },
        &mut |l: Layer<u32, u64>| match l {
            Layer::Node { left, right } => left + right,
            Layer::Leaf { value } => value as u64,
        },
    )
}

/// Encode a tree with SCALE, decode it back, and check the laws on the result.
pub fn roundtrip_and_check(depth: u32) -> Vec<u8> {
    let t = grow(depth);
    let bytes = t.encode();
    let back = Tree::<u32>::decode(&mut &bytes[..]).expect("valid encoding");
    laws::reflection(&back);
    laws::iso(&back);
    assert_eq!(sum(back), sum_fused(depth));
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_roundtrip_and_laws() {
        let bytes = roundtrip_and_check(4);
        assert!(!bytes.is_empty());
        assert_eq!(sum_fused(4), 4 * 16);
    }
}
