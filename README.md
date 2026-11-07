<div align="center">
  <p align="center">
    <img src="docs/images/NEST-logo-full.png" alt="NEST logo" width="50%" style="margin: 0px;">
    <!-- <picture> -->
    <!--   <source media="(prefers-color-scheme: dark)" srcset="docs/images/NEST-logo-full-dark.png"> -->
    <!--   <source media="(prefers-color-scheme: light)" srcset="docs/images/NEST-logo-full.png"> -->
    <!--   <img src="docs/images/NEST-logo-full.png" alt="NEST logo" width="50%"> -->
    <!-- </picture> -->
  </p>

  [Getting started] | [Learn] | [Guides] | [Reference] | [Python client]
</div>

NEST (*Numerical Environment for Software Testing*) is modular framework for
reproducible software-in-the-loop testing of embedded systems.

[Getting Started]: https://CHESS-mission.github.io/nest
[Learn]: https://CHESS-mission.github.io/nest/learn
[Guides]: https://CHESS-mission.github.io/nest/guides
[Reference]: https://CHESS-mission.github.io/nest/reference
[Python client]: https://github.com/CHESS-mission/nest-python-client

## Building from source

Building NEST from source requires the following dependencies:

- [SAAS](https://sass-lang.com/install/)
- [Tailwind](https://tailwindcss.com/docs/installation)
- [Binaryen](https://github.com/WebAssembly/binaryen)


Then the `server` release binary can be built using [Cargo
Leptos](https://github.com/leptos-rs/cargo-leptos):

```bash
cargo install --locked cargo-leptos
cargo leptos build --release
```

## Building with Nix

NEST provides both a fully-featured development shell and the `nest-server`
package as outputs of its [Nix](https://nixos.org/) flake. You can manually
build NEST by entering the development shell, which provides all the project's
build dependencies and some useful runtime tools (e.g. QEMU, picocom):

```bash
nix develop
cargo leptos build --release
```

Alternatively, you can directly build NEST with Nix:

```bash
nix build                            # local build
nix build github:CHESS-mission/nest  # or build source from github
```

## Docker image

Building and running NEST inside a Docker container is a convenient option both
for development and deployment. Start by building our Debian-based image, which
builds NEST as part of the process.

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
