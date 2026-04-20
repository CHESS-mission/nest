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

## Docker image

Building and running NEST inside a Docker container is a convenient option both
for development and deployment. Start by building the Debian-based image
provided in `Dockerfile`, which builds NEST as part of the process.

```bash
docker build -t nest-server:latest .
```

Then, run NEST in the container (forwarding port `3000` to the host).

```bash
docker run --rm -p 3000:3000 --shm-size=2g nest-server:latest
```

> [!NOTE]
> In NEST, QEMU VMs use `/dev/shm` as a file-backed memory backend (required
> for vhost-user devices). Docker limits shared memory to 64MB by default, so
> make sure to increase it to accommodate the total RAM of all running VMs with
> `--shm-size`.

You may also directly pass arguments to NEST by appending them to the command.

```bash
docker run --rm -p 3000:3000 --shm-size=2g nest-server:latest --help
```
