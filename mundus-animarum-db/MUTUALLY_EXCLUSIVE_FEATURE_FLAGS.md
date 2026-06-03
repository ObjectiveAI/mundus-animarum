# Mutually exclusive feature flags

Cargo features are **additive by design** — there is no built-in way to declare
two features mutually exclusive. You can only enforce it at compile time.

## Enforce with `compile_error!`

Emit a compile error when an invalid combination is enabled:

```rust
#[cfg(all(feature = "foo", feature = "bar"))]
compile_error!("features `foo` and `bar` are mutually exclusive — enable only one");
```

Require *exactly one* of a set:

```rust
#[cfg(not(any(feature = "foo", feature = "bar")))]
compile_error!("exactly one of `foo` or `bar` must be enabled");

#[cfg(all(feature = "foo", feature = "bar"))]
compile_error!("`foo` and `bar` are mutually exclusive");
```

## Caveat: feature unification

Features unify across the whole dependency graph. If crate A pulls us in with
`foo` and crate B (elsewhere in the tree) pulls us in with `bar`, Cargo turns
**both** on for the single shared build — and the `compile_error!` fires through
no fault of the end user, who often can't fix it without patching a transitive
dependency. The more public the crate, the worse this gets.

## Robust alternative: priority `cfg`

For a library, prefer letting one feature silently win when both are on, so
unification can't break a build:

```rust
#[cfg(feature = "foo")]
fn backend() { /* foo impl */ }

// bar only takes effect when foo is NOT enabled
#[cfg(all(feature = "bar", not(feature = "foo")))]
fn backend() { /* bar impl */ }
```

## Other options

- **Runtime selection** — expose both paths, pick via config/env at runtime.
- **Separate crates** — `mycrate-foo` / `mycrate-bar` instead of one crate with
  exclusive features; the choice becomes which dependency you add.
- **`build.rs`** — read `CARGO_FEATURE_FOO` / `CARGO_FEATURE_BAR` and `panic!`.
  No real advantage over `compile_error!`, but runs earlier.

**Bottom line:** `compile_error!` is the idiomatic way to *signal* the intent
and is fine for internal crates/binaries where you control all callers. For a
published library, lean toward priority `cfg` or splitting crates.
