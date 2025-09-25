# NEST

NEST (Numerical Environment for Software Testing) is a modular integration
testing framework intended for flight software verification written in Rust
using [Extism](https://extism.org/).

## Build

Install the following dependencies:

- `cargo-leptos`: `cargo install --locked cargo-leptos`.
- `saas`: <https://sass-lang.com/install/>.
- `tailwindcss`: <https://tailwindcss.com/docs/installation>.
- `binaryen`: <https://github.com/WebAssembly/binaryen>.

Then the `server` release binary can be built using Cargo Leptos:

```bash
cargo leptos build --release
```
