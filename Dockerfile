FROM rust:slim-buster as builder

ARG TARGETARCH
ENV UPX_VERSION="4.2.2"
ENV UPX_URL="https://github.com/upx/upx/releases/download/v${UPX_VERSION}/upx-${UPX_VERSION}-${TARGETARCH}_linux.tar.xz"

# Install dependencies required for build
RUN    apt-get update \
    && apt-get install -y curl libdbus-1-dev libssl-dev build-essential cmake make

WORKDIR /usr/src/thermobeacon-server

# Download and install UPX
RUN    mkdir -p tools/upx/ \ 
    && curl -L "$UPX_URL" | tar -x -J -C tools/upx/ --strip-components 1 \
    && chmod +x tools/upx/upx

COPY . .

# Start building ...
RUN cargo build --release

# Compress executable. This saves about 12MB (97MB -> 85MB) in the final image
RUN ./tools/upx/upx --best --lzma target/release/thermobeacon-server

FROM debian:buster-slim

# Install dependencies required for runtime
RUN    apt-get update \ 
    && apt-get install -y dbus openssl curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /usr/src/thermobeacon-server/target/release/thermobeacon-server /app/thermobeacon-server

# Create empty config file
RUN    touch /app/config.yml \
    && chmod +x /app/thermobeacon-server

CMD ["/app/thermobeacon-server"]
