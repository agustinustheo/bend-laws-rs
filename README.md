# bend-laws

[Bend](https://bend-lang.com)'s `fold` and `bend` for Rust, the algebraic laws behind them as executable checks, and a bounded state-machine explorer built on the same idea. `no_std` (with `alloc`) by default, `std` optional, so it works in Polkadot SDK runtime crates as well as ordinary ones.

## Bend in Rust

In Bend, `~` marks a recursive field, `fold` consumes a structure bottom-up and `bend` grows one from a seed:

```text
type Tree: Node { ~left, ~right } | Leaf { value }
```

The same shape in Rust:

```rust
use bend_laws::{bend, bend_type, fold};

bend_type! {
    #[derive(Clone, Debug, PartialEq)]
    mod tree: Tree<T> {
        Node { ~left, ~right },
        Leaf { value: T },
    }
}
use tree::{Layer, Tree};

// bend d = 0: when d < 10: Node { fork(d+1), fork(d+1) } else: Leaf { d }
let t = bend!(Tree<u32>; d = 0 => if d < 10 {
    Layer::Node { left: fork(d + 1), right: fork(d + 1) }
} else {
    Layer::Leaf { value: d }
});

// fold t: Node => left + right, Leaf => value
let total = fold!(t;
    Layer::Node { left, right } => left + right,
    Layer::Leaf { value } => value as u64,
);
```

`bend_type!` generates the recursive type, its one-layer base functor `Layer<T, R>` (recursive fields become `R`), and an impl of the `Bend` trait:

| Method | Meaning |
|--------|---------|
| `fold` | catamorphism: consume bottom-up |
| `bend` | anamorphism: grow from a seed, `fork(x)` seeds a child |
| `hylo` | `fold(bend(seed))` without building the structure in memory |
| `fold_par` (`std`) | fold sibling subtrees on separate threads, as Bend does |

Attributes on the type are copied to both generated enums, so SCALE derives (`Encode`, `Decode`, `TypeInfo`) work. `no-std-check/` is a `#![no_std]` crate that does this and builds for `wasm32v1-none`, a target that has only `core` and `alloc`.

## The laws

`laws::*` checks the properties that make it safe to rewrite and parallelise folds. Each panics with a message naming the broken law:

| Law | Statement |
|-----|-----------|
| `reflection` | `fold(embed) = id` |
| `iso` | `embed . project = id` |
| `functor_identity` | `map_layer(l, id) = l` |
| `hylo_fusion` | `fold(bend(s)) = hylo(s)` |
| `fold_fusion` | if `h . alg = alg2 . map(h)` then `h . fold(alg) = fold(alg2)` |
| `parallel_fold` (`std`) | for a pure algebra, `fold_par = fold` |

`bend_laws!` turns the structural ones into `#[test]`s from sample values.

## Bounded state-machine exploration

`explore::explore` checks an invariant over every action sequence up to a given length, and returns a shortest sequence that breaks it. It is written as a `hylo`: `bend` grows the tree of action sequences from the initial state and `fold` searches it, so the tree is never stored and memory stays proportional to the depth.

```rust
use bend_laws::explore::{explore, Machine};

struct Counter;
#[derive(Clone, Copy, Debug)]
enum Op { Inc, Dec }

impl Machine for Counter {
    type State = i32;
    type Action = Op;
    fn init(&self) -> i32 { 0 }
    fn actions(&self, _: &i32) -> Vec<Op> { vec![Op::Inc, Op::Dec] }
    fn step(&self, &n: &i32, a: &Op) -> Option<i32> {   // None = rejected
        Some(match a { Op::Inc => n + 1, Op::Dec => n - 1 })
    }
    fn check(&self, &n: &i32) -> Result<(), String> {
        if n >= 0 { Ok(()) } else { Err(format!("counter at {n}")) }
    }
}

let cx = explore(&Counter, 4).unwrap();
assert_eq!(cx.violation, "counter at -1");   // trace: [Dec]
```

`step` can call real code, for example dispatch a call in a mock runtime and replay the accepted actions, so the check runs on the implementation itself.

## Tracking known defects

`finding!` ties a known defect to a law that returns `Err(evidence)` while the defect reproduces, and to a status:

```rust
bend_laws::finding! {
    /// Counter could go below zero.
    BUG_042: fixed => counter_law()
}
```

| Status | Defect reproduces | Defect gone |
|--------|-------------------|-------------|
| `open`, `accepted` | passes, prints the evidence | fails: mark it `fixed` |
| `fixed` | fails: regression, prints the evidence | passes |

A fix therefore has to change the status, and from then on the test guards against the defect coming back.

Don't commit `open` reproducers of security bugs to a public repository: an `open` law is a working exploit until the fix is deployed. Keep those private and publish the law as `fixed` with the fix.

## How this relates to proptest

This is property-based, state-machine testing with Bend's vocabulary. [`proptest`](https://crates.io/crates/proptest) and [`proptest-state-machine`](https://crates.io/crates/proptest-state-machine) are the mature tools for the same job. They give random, longer sequences with shrinking, saved regression files and comparison against a reference model. `explore` differs in being exhaustive within its bound, `no_std`, and dependency-free. Its limits: it only explores the actions you list, a clean run means "nothing found within the bound", and the search grows exponentially with depth.

## Testing

```sh
cargo test --workspace                          # std
cargo test -p bend_laws --no-default-features   # no std
cargo build -p bend_laws -p no-std-check --no-default-features --target wasm32v1-none
```

## License

MIT or Apache-2.0, at your option.
