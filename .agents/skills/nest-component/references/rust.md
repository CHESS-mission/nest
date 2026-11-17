# Rust

Two routes. The first is what the [custom components guide](../../../../docs/guides/introduction/custom-devices.md)
describes and what `new-component.sh --language rust` scaffolds. The second produces a
smaller module, at the cost of stepping outside `std`.

Both need the toolchain of this repository: `wasm32-wasip2` and `wasm32-unknown-unknown`
are listed in `rust-toolchain.toml`, which the flake reads when it assembles the shells.

```shell
nix develop .#component-rust        # from the root of the checkout
```

Outside Nix:

```shell
rustup target add wasm32-wasip2
```

## `wasm32-wasip2` — the default

```toml
[package]
name = "lm75a"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
wit-bindgen = { version = "0.51", default-features = false, features = ["realloc"] }

[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
panic = "abort"
strip = true
```

`panic = "abort"` is not a size option. A panic unwinds, and only the call that panicked
fails; with `abort` the trap crosses to the host, which is the honest outcome: the
component's state is unknown, and answering as if it were not is worse than stopping.

`wit_bindgen::generate!` with `generate_all` turns the world's `include` lines into
modules, including the `nest:plugin` interfaces. Naming is what binds, so the contract
spelled out in [contract.md](contract.md) holds regardless of this scaffolding.

```rust
wit_bindgen::generate!({
    world: "lm75a",
    generate_all,
    generate: { all },
});

mod exports;  pub use exports::Component;
mod i2c;      pub use i2c::{I2cTransaction, I2cRequest, I2cResponse, ...};
mod errors;   pub use errors::{InitError, CommandError};

struct Component;
impl Global for Component {}          // Global, not the nest_component trait
export!(Component, types, exports);   // the world, its types, and its exports

use crate::exports::nest::plugin::nest_component::{CommandError, InitError};
use crate::i2c::{I2cRequest, I2cResponse, I2cTransaction};
use crate::nest::plugin::{emit, get_time, log, set_timer};
```

State goes in the `thread_local!` block the scaffold writes — a `RefCell` panics on a
borrow across a host call, and an `UnsafeCell` is unsound for the same reason.

```rust
thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}
```

The handlers are the four in the contract. A port's signature is `(msg: T)`, not
`(msg: &T)`, even for `bytes`.

Two habits from `std` are wrong here, and both are silent:

- `thread::sleep` blocks the thread, which is where the message came in on, so later
  traffic and the next command wait too. Use `set_timer(ms)` and answer `on_timeout`.
- `SystemTime::now()` reads `clock_time_get`, a WASI import which is not implemented, and
  traps. Use `get_time()`, in seconds since the simulation started.

`println!` traps the same way; use `log_info`, `log_warning`, `log_error`.

## `wasm32-unknown-unknown` — the lean route

```shell
cargo build --target wasm32-unknown-unknown --release
wasm-tools component new target/wasm32-unknown-unknown/release/lm75a.wasm -o build/lm75a.wasm
```

No adapter, and none is wanted: a module that imports WASI cannot be componentized this
way at all, which is the point.

Inside this module, `SystemTime::now()` is a compile error rather than a trap, and
`println!` aborts — but aborting is *reachable code*, and reaching it unwinds the module
with no trap to explain it. `dead_code` warns at a line number; read the warnings, because
that path is easy to enter by accident. The route also drops `std::error::Error`, `JoinHandle`,
`File`, and `env::var`, and anything from `std::fs` or `std::env`.

A build of a trivial component is about a third of the size of the `wasip2` one, with
exports and imports identical. Reach for it when the module is large enough for that to
matter, or when you would rather the mistakes were compile errors.

## Building

```shell
cargo build --target wasm32-wasip2 --release
```

produces `target/wasm32-wasip2/release/lm75a.wasm`, a component already: no `jco`, no
`wasm-tools component new`.

Then read what it says it is, because this is the earliest point at which a naming mistake
is visible:

```shell
wasm-tools component wit build/lm75a.wasm
```

The exports should be `init`, `on-command`, and one `<port>-in` per system port; the
imports `emit`, `get-time`, one `<port>-out` per system port, and one `read-<name>` per
environment port. `wasm-tools` is in the shells; installed elsewhere, use `cargo install
wasm-tools`.

## Reference material

The guide walks through a complete component in this route, register table and all:
[Custom components](../../../../docs/guides/introduction/custom-devices.md). The host side is
documented in [the `wasm` component reference](../../../../docs/reference/components/wasm.md).
