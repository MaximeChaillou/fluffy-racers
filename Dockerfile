# Issue #225: the dedicated server of Fluffy Racers (docs/SERVEUR.md), its own signaling inside.
# docker compose up -d --build   (docker-compose.yml: always on, restarted after a crash)

# 1. The build: the server's program, in release, for this machine (x86-64 or ARM64).
FROM rust:1-bookworm AS build
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libasound2-dev libudev-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
COPY src src
COPY assets assets
RUN cargo build --release --bin fluffy-racers-server \
    && cp target/release/fluffy-racers-server /fluffy-racers-server

# 2. What runs: the program and the game's files (the circuits). No window, no GPU, no sound
# card: the libraries below are only those the program links to.
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends libasound2 libudev1 ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /fluffy-racers-server /app/fluffy-racers-server
COPY assets /app/assets
ENV BEVY_ASSET_ROOT=/app
# The signaling (WebSocket, TCP). WebRTC takes UDP ports at random: docker-compose.yml uses the
# host's network.
EXPOSE 3536
ENTRYPOINT ["/app/fluffy-racers-server"]
CMD ["--port", "3536"]
