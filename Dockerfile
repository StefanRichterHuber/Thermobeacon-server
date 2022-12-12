FROM  debian:buster-slim as builder

# Install dependencies required for build
RUN      apt-get update && apt-get install -y curl libdbus-1-dev libssl-dev build-essential cmake make \
     &&  rm -rf /var/lib/apt/lists/*

# Install rust nightly (to use the sparse registry)
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain nightly

WORKDIR /usr/src/thermobeacon-server
COPY . .

# Build the app (sparse-registry helps to prevent memory issues during cargo download of deps, but requires nightly)
# see https://github.com/rust-lang/cargo/issues/10781#issuecomment-1163829239
RUN cargo +nightly install -Z sparse-registry --path .

FROM debian:buster-slim

# Install dependencies required for runtime
RUN    apt-get update && apt-get install -y dbus openssl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /usr/local/cargo/bin/thermobeacon-server /app/thermobeacon-server

# Create empty config file
RUN    touch /app/config.yml \
    && chmod +x /app/thermobeacon-server

CMD ["/app/thermobeacon-server"]
