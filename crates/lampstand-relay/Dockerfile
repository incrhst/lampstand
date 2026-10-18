# Lampstand sync relay (CD-3). Build from the repository root:
#
#   docker build -f crates/lampstand-relay/Dockerfile -t lampstand-relay .
#   docker buildx build --platform linux/amd64,linux/arm64 -f crates/lampstand-relay/Dockerfile .
#
# Run:
#
#   docker run -d --name lampstand-relay -p 7878:7878 -v lampstand-relay:/data \
#     -e LAMPSTAND_RELAY_CHURCHES="grace=$(openssl rand -hex 32)" lampstand-relay
#
# Put a TLS reverse proxy in front of it on the internet; devices then use
# wss://your.host. The image is a static musl binary on Alpine, a few MB.

FROM rust:1.91-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY . .
# Small, stripped binary without touching the workspace's release profile.
ENV CARGO_PROFILE_RELEASE_LTO=true \
    CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 \
    CARGO_PROFILE_RELEASE_STRIP=true \
    CARGO_PROFILE_RELEASE_PANIC=abort
RUN cargo build --release --locked -p lampstand-relay \
    && cp target/release/lampstand-relay /lampstand-relay

FROM alpine:3.22
RUN adduser -D -H -u 10001 relay && mkdir /data && chown relay /data
COPY --from=build /lampstand-relay /usr/local/bin/lampstand-relay
USER relay
ENV LAMPSTAND_RELAY_DATA=/data \
    LAMPSTAND_RELAY_LISTEN=0.0.0.0:7878
VOLUME /data
EXPOSE 7878
ENTRYPOINT ["lampstand-relay"]
