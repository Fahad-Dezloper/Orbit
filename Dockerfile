# pocky-api: scheduled, read-only measurement and HTTP API.
#
#   docker build -t pocky-api .
#   docker run -p 8080:8080 -v pocky-data:/data pocky-api
#
# Holds no keys. The publisher runs separately, next to a stellar CLI keystore.

FROM rust:1-slim-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p pocky-api

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home /data pocky \
 && mkdir -p /data && chown pocky /data
COPY --from=build /src/target/release/pocky-api /usr/local/bin/pocky-api
COPY assets.toml /etc/pocky/assets.toml
USER pocky
VOLUME /data
EXPOSE 8080
ENTRYPOINT ["pocky-api", "--bind", "0.0.0.0:8080", "--data-dir", "/data", "--registry", "/etc/pocky/assets.toml"]
