//! Bounded state-machine exploration, written as a Bend `hylo`.
//!
//! `bend` grows the tree of every action sequence from the initial state, up to
//! a depth. `fold` walks it back up and keeps the first path that reaches a
//! state breaking the invariant. Because the two run as one `hylo`, the tree is
//! never stored: memory is O(depth), and the search stops growing a branch as
//! soon as it breaks the invariant.
//!
//! [`Machine::step`] can call the real implementation (for example, dispatch a
//! call in a mock runtime), so the check runs on the code itself rather than on
//! a separate model of it that could drift.

use crate::Bend;
use alloc::{string::String, vec::Vec};
use core::fmt::Debug;

/// A system under test: its initial state, the actions possible in a state, how
/// an action changes the state, and the invariant every reachable state must
/// satisfy.
pub trait Machine {
    type State;
    type Action: Clone + Debug;

    fn init(&self) -> Self::State;
    /// The actions to try from `state`. The explorer tries them in this order.
    fn actions(&self, state: &Self::State) -> Vec<Self::Action>;
    /// Run `action` from `state`. `None` means it was rejected (e.g. the
    /// call returned an error). A rejected action changes no state, so the
    /// explorer drops that branch.
    fn step(&self, state: &Self::State, action: &Self::Action) -> Option<Self::State>;
    /// The invariant. `Err` says what broke.
    fn check(&self, state: &Self::State) -> Result<(), String>;
}

/// A shortest-prefix path to a state that breaks the invariant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Counterexample<A> {
    /// The actions from the initial state, in order.
    pub trace: Vec<A>,
    /// What `Machine::check` reported for the last state.
    pub violation: String,
}

/// The exploration tree: one node per reached state, one child per accepted
/// action. A rose tree, so its `Bend` impl is written by hand (the children are
/// a `Vec`, not fixed fields).
pub struct Paths<A> {
    pub violation: Option<String>,
    pub children: Vec<(A, Paths<A>)>,
}

/// One layer of [`Paths`].
pub struct PathsLayer<A, R> {
    pub violation: Option<String>,
    pub children: Vec<(A, R)>,
}

impl<A> Bend for Paths<A> {
    type Layer<R> = PathsLayer<A, R>;

    fn project(self) -> Self::Layer<Self> {
        PathsLayer {
            violation: self.violation,
            children: self.children,
        }
    }

    fn embed(layer: Self::Layer<Self>) -> Self {
        Paths {
            violation: layer.violation,
            children: layer.children,
        }
    }

    fn map_layer<X, Y>(layer: Self::Layer<X>, mut f: impl FnMut(X) -> Y) -> Self::Layer<Y> {
        PathsLayer {
            violation: layer.violation,
            children: layer.children.into_iter().map(|(a, x)| (a, f(x))).collect(),
        }
    }
}

/// Every reachable state within `depth` actions, checked against the invariant.
/// Returns a shortest counterexample, or `None` if the invariant holds for every
/// one of them.
///
/// Iterative deepening: one [`search`] per depth bound, so the first trace
/// found is a shortest one, while memory stays O(depth).
pub fn explore<M: Machine>(m: &M, depth: usize) -> Option<Counterexample<M::Action>> {
    (0..=depth).find_map(|d| search(m, d))
}

/// A path to a violation from some node, and the violation.
type Found<A> = Option<(Vec<A>, String)>;

/// One depth-first `hylo` over every path of at most `depth` actions. Returns
/// the first counterexample in depth-first order.
pub fn search<M: Machine>(m: &M, depth: usize) -> Option<Counterexample<M::Action>> {
    // The seed is a state and the actions left; the coalgebra is one `bend` step.
    let mut coalg = |(state, left): (M::State, usize)| {
        let violation = m.check(&state).err();
        let children = if violation.is_some() || left == 0 {
            Vec::new()
        } else {
            m.actions(&state)
                .into_iter()
                .filter_map(|a| m.step(&state, &a).map(|next| (a, (next, left - 1))))
                .collect()
        };
        PathsLayer {
            violation,
            children,
        }
    };
    // The algebra is one `fold` step: a violation here, or the first child that
    // leads to one, with this edge's action put in front of its trace.
    let mut alg = |layer: PathsLayer<M::Action, Found<M::Action>>| {
        if let Some(v) = layer.violation {
            return Some((Vec::new(), v));
        }
        layer.children.into_iter().find_map(|(a, found)| {
            found.map(|(mut rest, v)| {
                rest.insert(0, a);
                (rest, v)
            })
        })
    };
    Paths::<M::Action>::hylo((m.init(), depth), &mut coalg, &mut alg)
        .map(|(trace, violation)| Counterexample { trace, violation })
}
