use bend_laws::{Bend, bend, bend_laws, bend_type, fold, laws};

// Bend:
//   type Tree: Node { ~left, ~right } | Leaf { value }
bend_type! {
    #[derive(Clone, Debug, PartialEq, Eq)]
    mod tree: Tree<T> {
        Node { ~left, ~right },
        Leaf { value: T },
    }
}
use tree::{Layer, Tree};

// Bend:
//   type List: Cons { head, ~tail } | Nil
bend_type! {
    #[derive(Clone, Debug, PartialEq, Eq)]
    mod list: List<T> {
        Cons { head: T, ~tail },
        Nil {},
    }
}

// Bend:
//   def grow(depth):
//     bend d = 0:
//       when d < depth: x = Tree/Node { left: fork(d+1), right: fork(d+1) }
//       else:           x = Tree/Leaf { value: d }
//     return x
fn grow(depth: u32) -> Tree<u32> {
    bend!(Tree<u32>; d = 0 => if d < depth {
        Layer::Node { left: fork(d + 1), right: fork(d + 1) }
    } else {
        Layer::Leaf { value: d }
    })
}

// Bend:
//   def sum(tree):
//     fold tree:
//       case Tree/Node: return tree.left + tree.right
//       case Tree/Leaf: return tree.value
fn sum(t: Tree<u32>) -> u64 {
    fold!(t;
        Layer::Node { left, right } => left + right,
        Layer::Leaf { value } => value as u64,
    )
}

fn sum_alg(l: Layer<u32, u64>) -> u64 {
    match l {
        Layer::Node { left, right } => left + right,
        Layer::Leaf { value } => value as u64,
    }
}

#[test]
fn bend_then_fold() {
    // 2^10 leaves, each holding 10
    assert_eq!(sum(grow(10)), 10 * 1024);
}

#[test]
fn list_roundtrip() {
    let xs: list::List<i32> = <list::List<i32> as Bend>::bend(3, &mut |n| {
        if n == 0 {
            list::Layer::Nil {}
        } else {
            list::Layer::Cons {
                head: n,
                tail: n - 1,
            }
        }
    });
    let v: Vec<i32> = xs.fold(&mut |l: list::Layer<i32, Vec<i32>>| match l {
        list::Layer::Nil {} => vec![],
        list::Layer::Cons { head, mut tail } => {
            tail.insert(0, head);
            tail
        }
    });
    assert_eq!(v, vec![3, 2, 1]);
}

#[test]
fn law_hylo_fusion() {
    laws::hylo_fusion::<Tree<u32>, _, _>(
        0u32,
        |d| {
            if d < 8 {
                Layer::Node {
                    left: d + 1,
                    right: d + 1,
                }
            } else {
                Layer::Leaf { value: d }
            }
        },
        sum_alg,
    );
}

#[test]
fn law_fold_fusion() {
    // h = (*2), alg2 doubles at the leaves: h . fold(sum) == fold(sum with leaves doubled)
    laws::fold_fusion(
        &grow(6),
        sum_alg,
        |l: Layer<u32, u64>| match l {
            Layer::Node { left, right } => left + right,
            Layer::Leaf { value } => 2 * value as u64,
        },
        |n| n * 2,
    );
}

#[cfg(feature = "std")]
#[test]
fn law_parallel_fold() {
    laws::parallel_fold(&grow(16), sum_alg, 4);
}

#[test]
#[should_panic(expected = "h . fold(alg) != fold(alg2)")]
fn unlawful_fusion_is_caught() {
    // h = (+1) does NOT distribute over +, so the premise fails and the check catches it.
    laws::fold_fusion(
        &grow(3),
        sum_alg,
        |l: Layer<u32, u64>| match l {
            Layer::Node { left, right } => left + right,
            Layer::Leaf { value } => value as u64 + 1,
        },
        |n| n + 1,
    );
}

bend_laws! { tree_laws for Tree<u32>, samples = [grow(0), grow(3), grow(7)] }

// ---- explore + finding! -------------------------------------------------

use bend_laws::explore::{Counterexample, Machine, explore};

/// A toy counter that must stay within `0..=3`. The buggy version lets
/// `Dec` run at zero.
struct Counter {
    guard_zero: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    Inc,
    Dec,
}

impl Machine for Counter {
    type State = i32;
    type Action = Op;
    fn init(&self) -> i32 {
        0
    }
    fn actions(&self, _: &i32) -> Vec<Op> {
        vec![Op::Inc, Op::Dec]
    }
    fn step(&self, &n: &i32, a: &Op) -> Option<i32> {
        match *a {
            Op::Inc if n < 3 => Some(n + 1),
            Op::Inc => None,
            Op::Dec if self.guard_zero && n == 0 => None,
            Op::Dec => Some(n - 1),
        }
    }
    fn check(&self, &n: &i32) -> Result<(), String> {
        if (0..=3).contains(&n) {
            Ok(())
        } else {
            Err(format!("counter at {n}, outside 0..=3"))
        }
    }
}

#[test]
fn explore_finds_a_shortest_counterexample() {
    // Depth-first order would try `Inc` first and find [Inc, Dec, Dec];
    // iterative deepening returns the shortest trace.
    let cx = explore(&Counter { guard_zero: false }, 4).expect("bug is reachable");
    assert_eq!(
        cx,
        Counterexample {
            trace: vec![Op::Dec],
            violation: "counter at -1, outside 0..=3".into()
        }
    );
    assert!(explore(&Counter { guard_zero: true }, 6).is_none());
}

fn counter_law(guard_zero: bool) -> Result<(), Counterexample<Op>> {
    match explore(&Counter { guard_zero }, 4) {
        Some(cx) => Err(cx),
        None => Ok(()),
    }
}

bend_laws::finding! {
    /// The toy defect still reproduces, as its status says.
    TOY_001: open => counter_law(false)
}
bend_laws::finding! {
    /// The toy fix holds.
    TOY_002: fixed => counter_law(true)
}

#[test]
#[should_panic(expected = "TOY-003 no longer reproduces but is marked Open")]
fn open_finding_that_was_fixed_fails_loudly() {
    bend_laws::check_finding("TOY-003", bend_laws::Status::Open, counter_law(true));
}

#[test]
#[should_panic(expected = "TOY-004 regressed")]
fn fixed_finding_that_regressed_fails_loudly() {
    bend_laws::check_finding("TOY-004", bend_laws::Status::Fixed, counter_law(false));
}
