# syntax=docker/dockerfile:1

# Builder image
FROM rust:1.92.0-trixie AS builder

# Pin toolchain version
ENV RUSTUP_TOOLCHAIN=1.92.0

# Install cargo-binstall, which makes it easier to install other
# cargo extensions like cargo-leptos
RUN wget https://github.com/cargo-bins/cargo-binstall/releases/latest/download/cargo-binstall-x86_64-unknown-linux-musl.tgz
RUN tar -xvf cargo-binstall-x86_64-unknown-linux-musl.tgz
RUN cp cargo-binstall /usr/local/cargo/bin

# Install required tools
RUN apt-get update -y \
 && apt-get install -y --no-install-recommends clang

# Install cargo-leptos
RUN cargo binstall cargo-leptos -y

# Add the WASM target
RUN rustup target add wasm32-unknown-unknown

# Make an /app dir, which everything will eventually live in
RUN mkdir -p /app
WORKDIR /app
COPY . .

# Build the app with cach mounts
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    --mount=type=cache,target=/root/.cache/cargo-leptos \
    cargo leptos build --release -vv \
 && mkdir -p /out \
 && cp target/release/nest-server /out/nest-server \
 && cp -r target/site /out/site

# Runtime image
FROM debian:trixie-slim AS runtime

WORKDIR /app

RUN apt-get update -y \
 && apt-get install -y --no-install-recommends qemu-system \
 && apt-get autoremove -y \
 && apt-get clean -y \
 && rm -rf /var/lib/apt/lists/*

# Copy the server binary to the /app directory
COPY --from=builder /out/nest-server /app/

# /target/site contains our JS/WASM/CSS, etc.
COPY --from=builder /out/site /app/site

ENV LEPTOS_SITE_ROOT="site"
EXPOSE 3000

# Run the server
ENTRYPOINT ["/app/nest-server"]
CMD ["--listen", "0.0.0.0:3000"]
