FROM rust:slim-buster as builder

# Install dependencies required for build
RUN     apt-get update && apt-get install -y curl libdbus-1-dev libssl-dev build-essential cmake make \
    &&  rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/thermobeacon-server
COPY . .

# Start building ...
RUN cargo install --path .

FROM debian:buster-slim

# Install dependencies required for runtime
RUN    apt-get update && apt-get install -y dbus openssl curl\
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /usr/local/cargo/bin/thermobeacon-server /app/thermobeacon-server

# Create empty config file
RUN    touch /app/config.yml \
    && chmod +x /app/thermobeacon-server

CMD ["/app/thermobeacon-server"]
