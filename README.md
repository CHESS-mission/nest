<div align="center">
  <p align="center">
    <img src="docs/images/NEST-logo-full.png" alt="NEST logo" height="150" style="margin: 20px;">
    <!-- <picture> -->
    <!--   <source media="(prefers-color-scheme: dark)" srcset="docs/images/NEST-logo-full-dark.png"> -->
    <!--   <source media="(prefers-color-scheme: light)" srcset="docs/images/NEST-logo-full.png"> -->
    <!--   <img src="docs/images/NEST-logo-full.png" alt="NEST logo" height="150" style="margin: 20px;"> -->
    <!-- </picture> -->
  </p>

  <p align="center">
    <strong>
       A modular framework for reproducible software-in-the-loop testing of
       embedded systems
    </strong>
  </p>

  <p align="center">
    <img src="https://img.shields.io/badge/status-active-brightgreen?style=for-the-badge" alt="Status" />
    <img src="https://img.shields.io/badge/license-MIT-blue?style=for-the-badge" alt="License" />
    <img src="https://img.shields.io/badge/python-3.9+-blueviolet?style=for-the-badge" alt="Python" />
  </p>
</div>

<!-- NEST (Numerical Environment for Software Testing) is a modular integration -->
<!-- testing framework intended for flight software verification written in Rust -->
<!-- using Extism. -->

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
