# orbit-api: scheduled, read-only measurement and HTTP API.
#
#   docker build -t orbit-api .
#   docker run -p 8080:8080 -v orbit-data:/data orbit-api
#
# Holds no keys. The publisher runs separately, next to a stellar CLI keystore.

FROM rust:1-slim-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p orbit-api

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home /data orbit \
 && mkdir -p /data && chown orbit /data
COPY --from=build /src/target/release/orbit-api /usr/local/bin/orbit-api
COPY assets.toml /etc/orbit/assets.toml
USER orbit
VOLUME /data
EXPOSE 8080
ENTRYPOINT ["orbit-api", "--bind", "0.0.0.0:8080", "--data-dir", "/data", "--registry", "/etc/orbit/assets.toml"]
