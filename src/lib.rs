//! Bend-style `fold` / `bend` for Rust, plus the laws that make them safe to
//! rewrite and to run in parallel. Works in `no_std` (needs `alloc`) and `std`.
//!
//! Bend (<https://bend-lang.com>):
//! ```text
//! type Tree: Node { ~left, ~right } | Leaf { value }
//! ```
//! `~` marks a recursive field. `fold` replaces every recursive field with the
//! already-folded result (a catamorphism). `bend` grows a structure from a seed,
//! where `fork(x)` means "a recursive field, grown from seed x" (an anamorphism).
//!
//! From that same shape, [`bend_type!`] generates:
//!   - the recursive type `Tree<T>` (recursive fields are `Box<Tree<T>>`)
//!   - its one-layer "base functor" `Layer<T, R>` (recursive fields are `R`)
//!   - an impl of [`Bend`], which gives you `fold`, `bend`, `hylo` and, with
//!     the `std` feature, `fold_par`.
//!
//! ## `no_std` / Polkadot SDK
//! The crate is `no_std` unless the `std` feature is on, following the usual
//! runtime-crate setup:
//! ```toml
//! bend_laws = { version = "0.1", default-features = false }
//! [features]
//! std = ["bend_laws/std"]
//! ```
//! Attributes written on the type in `bend_type!` are copied to both generated
//! enums, so you can add `#[derive(Encode, Decode, TypeInfo)]` etc. there.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

/// Paths the macros use, so the code they generate works in `no_std` crates.
#[doc(hidden)]
pub mod __private {
    pub use alloc::boxed::Box;
}

pub mod explore;

/// A recursive type described by its one-layer shape.
pub trait Bend: Sized {
    /// One layer of the structure, with recursive positions replaced by `R`.
    type Layer<R>;

    /// Peel one layer off.
    fn project(self) -> Self::Layer<Self>;
    /// Put one layer back on.
    fn embed(layer: Self::Layer<Self>) -> Self;
    /// Apply `f` to every recursive position of a layer (the functor `map`).
    /// Children are visited left to right, in the order the fields were declared.
    fn map_layer<A, B>(layer: Self::Layer<A>, f: impl FnMut(A) -> B) -> Self::Layer<B>;

    /// Bend's `fold`: consume bottom-up.
    fn fold<A>(self, alg: &mut impl FnMut(Self::Layer<A>) -> A) -> A {
        let layer = Self::map_layer(self.project(), |child| child.fold(alg));
        alg(layer)
    }

    /// Bend's `bend`: grow top-down from a seed. Inside the coalgebra, each
    /// recursive field holds the seed for that child (Bend's `fork(seed)`).
    fn bend<S>(seed: S, coalg: &mut impl FnMut(S) -> Self::Layer<S>) -> Self {
        let layer = Self::map_layer(coalg(seed), |s| Self::bend(s, coalg));
        Self::embed(layer)
    }

    /// `fold(bend(seed))` without building the intermediate structure
    /// (deforestation; HVM gets the same effect from lazy reduction).
    fn hylo<S, A>(
        seed: S,
        coalg: &mut impl FnMut(S) -> Self::Layer<S>,
        alg: &mut impl FnMut(Self::Layer<A>) -> A,
    ) -> A {
        let layer = Self::map_layer(coalg(seed), |s| Self::hylo(s, coalg, alg));
        alg(layer)
    }

    /// Parallel fold: sibling subtrees run on separate threads until `depth`
    /// reaches 0, then the rest is folded sequentially. This is only correct
    /// because of the purity law (see [`laws::parallel_fold`]): `alg` is
    /// `Fn + Sync`, so it can't depend on evaluation order.
    #[cfg(feature = "std")]
    fn fold_par<A: Send>(self, alg: &(impl Fn(Self::Layer<A>) -> A + Sync), depth: u32) -> A
    where
        Self: Send,
    {
        if depth == 0 {
            return self.fold(&mut |l| alg(l));
        }
        std::thread::scope(|s| {
            let handles = Self::map_layer(self.project(), |c| {
                s.spawn(move || c.fold_par(alg, depth - 1))
            });
            let layer = Self::map_layer(handles, |h| h.join().expect("fold_par worker panicked"));
            alg(layer)
        })
    }
}

/// Declare a Bend-style recursive type. Recursive fields are written `~name`
/// (no type); other fields are `name: Type`.
///
/// ```
/// bend_laws::bend_type! {
///     #[derive(Clone, Debug, PartialEq)]
///     mod tree: Tree<T> {
///         Node { ~left, ~right },
///         Leaf { value: T },
///     }
/// }
/// ```
///
/// Generates `pub mod tree { pub enum Tree<T>, pub enum Layer<T, R>, impl Bend for Tree<T> }`.
#[macro_export]
macro_rules! bend_type {
    (
        $(#[$attr:meta])*
        mod $m:ident : $name:ident $(< $($g:ident),* >)? { $($body:tt)* }
    ) => {
        $crate::bend_type!(@norm ($name $(< $($g),* >)?)
            [[$(#[$attr])*] $m $name [$($($g),*)?]] [] { $($body)* });
    };

    // Normalise one variant at a time.
    (@norm ($selfty:ty) $hdr:tt [$($out:tt)*] { $v:ident { $($fields:tt)* } $(, $($rest:tt)*)? }) => {
        $crate::bend_type!(@fields ($selfty) $hdr [$($out)*] $v [] { $($fields)* } { $($($rest)*)? });
    };
    (@norm ($selfty:ty) $hdr:tt [$($out:tt)*] { }) => {
        $crate::bend_type!(@emit $hdr ($selfty) $($out)*);
    };

    // Normalise each field to: name [type in the recursive enum] [type in Layer] kind;
    (@fields ($selfty:ty) $hdr:tt $out:tt $v:ident [$($f:tt)*]
        { ~ $field:ident $(, $($rest:tt)*)? } $vrest:tt) => {
        $crate::bend_type!(@fields ($selfty) $hdr $out $v
            [$($f)* $field [$crate::__private::Box<$selfty>] [R] rec;] { $($($rest)*)? } $vrest);
    };
    (@fields ($selfty:ty) $hdr:tt $out:tt $v:ident [$($f:tt)*]
        { $field:ident : $t:ty $(, $($rest:tt)*)? } $vrest:tt) => {
        $crate::bend_type!(@fields ($selfty) $hdr $out $v
            [$($f)* $field [$t] [$t] val;] { $($($rest)*)? } $vrest);
    };
    (@fields ($selfty:ty) $hdr:tt [$($out:tt)*] $v:ident [$($f:tt)*] { } $vrest:tt) => {
        $crate::bend_type!(@norm ($selfty) $hdr [$($out)* $v { $($f)* }] $vrest);
    };

    (@emit [[$($attrs:tt)*] $m:ident $name:ident [$($g:ident),*]] ($selfty:ty)
        $( $variant:ident { $( $field:ident [$tty:ty] [$lty:ty] $kind:ident; )* } )*
    ) => {
        pub mod $m {
            #![allow(unused_imports)]
            use super::*;

            $($attrs)*
            pub enum $name < $($g),* > {
                $( $variant { $( $field: $tty, )* }, )*
            }

            /// One layer of the type above, with every recursive field replaced by `R`.
            $($attrs)*
            pub enum Layer< $($g,)* R > {
                $( $variant { $( $field: $lty, )* }, )*
            }

            impl < $($g),* > $crate::Bend for $selfty {
                type Layer<R> = Layer< $($g,)* R >;

                fn project(self) -> Self::Layer<Self> {
                    match self {
                        $( $name::$variant { $($field),* } => Layer::$variant {
                            $( $field: $crate::__field_map!(
                                |b: $crate::__private::Box<Self>| *b, $field, $kind), )*
                        }, )*
                    }
                }

                fn embed(layer: Self::Layer<Self>) -> Self {
                    match layer {
                        $( Layer::$variant { $($field),* } => $name::$variant {
                            $( $field: $crate::__field_map!(
                                $crate::__private::Box::new, $field, $kind), )*
                        }, )*
                    }
                }

                #[allow(unused_mut, unused_variables)]
                fn map_layer<A, B>(layer: Self::Layer<A>, mut f: impl FnMut(A) -> B) -> Self::Layer<B> {
                    match layer {
                        $( Layer::$variant { $($field),* } => Layer::$variant {
                            $( $field: $crate::__field_map!(&mut f, $field, $kind), )*
                        }, )*
                    }
                }
            }
        }
    };
}

/// A recursive field gets `f` applied; any other field passes through unchanged.
#[doc(hidden)]
#[macro_export]
macro_rules! __field_map {
    ($f:expr, $x:ident, rec) => {
        ($f)($x)
    };
    ($f:expr, $x:ident, val) => {
        $x
    };
}

/// Bend-style `fold`:
/// `fold!(tree; Layer::Node { left, right } => left + right, Layer::Leaf { value } => value)`.
#[macro_export]
macro_rules! fold {
    ($e:expr; $($pat:pat => $body:expr),+ $(,)?) => {
        $crate::Bend::fold($e, &mut |layer| match layer { $($pat => $body),+ })
    };
}

/// Bend-style `bend`, with `fork` in scope:
/// `bend!(Tree<u32>; d = 0 => if d < 3 { Layer::Node { left: fork(d + 1), right: fork(d + 1) } } else { Layer::Leaf { value: d } })`.
#[macro_export]
macro_rules! bend {
    ($ty:ty; $seed:ident = $init:expr => $body:expr) => {
        <$ty as $crate::Bend>::bend($init, &mut |$seed| {
            #[allow(unused)]
            fn fork<S>(s: S) -> S {
                s
            }
            $body
        })
    };
}

/// The laws, as executable checks.
///
/// Rust's type system can't prove these for arbitrary algebras, so each one is
/// a function you run on sample inputs (unit or property tests). Each is a
/// theorem for lawful inputs, so a failure means the algebra, `map_layer` or
/// type is wrong. All of them work in `no_std` (they only use `core`).
pub mod laws {
    use super::Bend;
    use core::fmt::Debug;

    /// Reflection: folding with the constructors rebuilds the value. `fold embed = id`.
    pub fn reflection<T: Bend + Clone + PartialEq + Debug>(x: &T) {
        assert_eq!(x.clone().fold(&mut T::embed), *x, "fold(embed) != id");
    }

    /// `embed . project = id` (the type really is the fixpoint of its layer).
    pub fn iso<T: Bend + Clone + PartialEq + Debug>(x: &T) {
        assert_eq!(T::embed(x.clone().project()), *x, "embed . project != id");
    }

    /// Functor identity on one layer: `map_layer(l, id) = l`.
    pub fn functor_identity<T: Bend>(x: T)
    where
        T::Layer<T>: Clone + PartialEq + Debug,
    {
        let l = x.project();
        assert_eq!(T::map_layer(l.clone(), |c| c), l, "map(id) != id");
    }

    /// Cancellation / deforestation: `fold(bend(seed)) = hylo(seed)`.
    pub fn hylo_fusion<T: Bend, S: Clone, A: PartialEq + Debug>(
        seed: S,
        mut coalg: impl FnMut(S) -> T::Layer<S>,
        mut alg: impl FnMut(T::Layer<A>) -> A,
    ) {
        let built = T::bend(seed.clone(), &mut coalg).fold(&mut alg);
        let fused = T::hylo(seed, &mut coalg, &mut alg);
        assert_eq!(built, fused, "fold(bend(s)) != hylo(s)");
    }

    /// Fold fusion: if `h(alg(l)) = alg2(map(l, h))` then `h(fold alg x) = fold alg2 x`.
    /// This checks the conclusion. Making sure the premise holds is up to the caller.
    pub fn fold_fusion<T: Bend + Clone, A, B: PartialEq + Debug>(
        x: &T,
        mut alg: impl FnMut(T::Layer<A>) -> A,
        mut alg2: impl FnMut(T::Layer<B>) -> B,
        mut h: impl FnMut(A) -> B,
    ) {
        let lhs = h(x.clone().fold(&mut alg));
        let rhs = x.clone().fold(&mut alg2);
        assert_eq!(lhs, rhs, "h . fold(alg) != fold(alg2)");
    }

    /// Bend's main law: for a pure algebra, parallel fold = sequential fold.
    #[cfg(feature = "std")]
    pub fn parallel_fold<T: Bend + Clone + Send, A: Send + PartialEq + Debug>(
        x: &T,
        alg: impl Fn(T::Layer<A>) -> A + Sync,
        depth: u32,
    ) {
        let seq = x.clone().fold(&mut |l| alg(l));
        let par = x.clone().fold_par(&alg, depth);
        assert_eq!(seq, par, "fold_par != fold");
    }
}

/// Generate a `#[test]` for each structural law of a type, from sample values.
///
/// ```ignore
/// bend_laws! { tree_laws for Tree<u32>, samples = [grow(0), grow(3)] }
/// ```
#[macro_export]
macro_rules! bend_laws {
    ($m:ident for $ty:ty, samples = [$($s:expr),* $(,)?]) => {
        #[cfg(test)]
        mod $m {
            use super::*;

            #[test] fn reflection() { $( $crate::laws::reflection::<$ty>(&$s); )* }
            #[test] fn iso() { $( $crate::laws::iso::<$ty>(&$s); )* }
            #[test] fn functor_identity() { $( $crate::laws::functor_identity::<$ty>($s); )* }
        }
    };
}

/// The status of a known defect, as tracked in an issue tracker or a list of
/// known defects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// The defect is known and not fixed. It must still reproduce.
    Open,
    /// The defect is known and kept on purpose. It must still reproduce.
    Accepted,
    /// The defect was fixed. It must never reproduce again.
    Fixed,
}

/// Check a defect's law against its status. Panics (fails the test) on a
/// mismatch. `law` is `Err(evidence)` when the defect reproduces.
///
/// - `Open` or `Accepted` and `Ok`: the defect no longer reproduces. The status
///   has to move to `Fixed`, so the law guards against a regression from then
///   on. A fix therefore has to update the status where it is tracked.
/// - `Fixed` and `Err`: a regression. The panic prints the evidence.
#[track_caller]
pub fn check_finding<E: core::fmt::Debug>(id: &str, status: Status, law: Result<(), E>) {
    match (status, law) {
        (Status::Fixed, Ok(())) => {}
        (Status::Fixed, Err(evidence)) => {
            panic!("{id} regressed: it is marked fixed but reproduces again.\n{evidence:#?}")
        }
        (Status::Open | Status::Accepted, Err(_evidence)) => {
            #[cfg(feature = "std")]
            std::println!("{id} ({status:?}) reproduces:\n{_evidence:#?}");
        }
        (Status::Open | Status::Accepted, Ok(())) => panic!(
            "{id} no longer reproduces but is marked {status:?}. If a change fixed it, mark it \
             `fixed`, here and wherever it is tracked, so this law guards against a \
             regression from now on."
        ),
    }
}

/// Pin a known defect to an executable law, with its status.
///
/// ```ignore
/// bend_laws::finding! {
///     /// What the defect is, where the evidence is.
///     BUG_042: fixed => law_that_returns_a_result()
/// }
/// ```
///
/// Generates a `#[test] fn BUG_042` that runs the law (an expression of type
/// `Result<(), E>`, `Err` when the defect reproduces) and checks it against
/// the status with [`check_finding`]. The status is `open`, `accepted` or `fixed`.
#[macro_export]
macro_rules! finding {
    ($(#[$doc:meta])* $id:ident : $status:ident => $law:expr) => {
        $(#[$doc])*
        #[test]
        #[allow(non_snake_case)]
        fn $id() {
            $crate::check_finding(
                &stringify!($id).replace('_', "-"),
                $crate::finding!(@status $status),
                $law,
            );
        }
    };
    (@status open) => { $crate::Status::Open };
    (@status accepted) => { $crate::Status::Accepted };
    (@status fixed) => { $crate::Status::Fixed };
}
