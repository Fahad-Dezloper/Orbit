# Orbit images. Two targets:
#
#   docker build -t orbit-api .                              # measurement + HTTP API (default)
#   docker build --target publisher -t orbit-publisher .     # publishes to a depth oracle
#
# Or run everything with `docker compose up` (see docker-compose.yml and docs/DEPLOY.md).
# The API image holds no keys. The publisher image holds none either: its key is mounted at
# run time as a Docker secret.

FROM rust:1-slim-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p orbit-api -p orbit-publisher

# The stellar CLI signs the publisher's transactions.
FROM debian:bookworm-slim AS stellar-cli
ARG STELLAR_CLI_VERSION=30.0.1
ARG TARGETARCH
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && case "${TARGETARCH:-amd64}" in \
      amd64) arch=x86_64 ;; \
      arm64) arch=aarch64 ;; \
      *) echo "unsupported architecture ${TARGETARCH}"; exit 1 ;; \
    esac \
 && curl -fsSL -o /tmp/stellar.tgz \
      "https://github.com/stellar/stellar-cli/releases/download/v${STELLAR_CLI_VERSION}/stellar-cli-${STELLAR_CLI_VERSION}-${arch}-unknown-linux-gnu.tar.gz" \
 && tar -xzf /tmp/stellar.tgz -C /usr/local/bin stellar

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --create-home --home-dir /home/orbit orbit \
 && mkdir -p /data && chown orbit /data
COPY assets.toml /etc/orbit/assets.toml

FROM runtime AS publisher
# The stellar CLI links against libdbus (for the OS keyring, unused here).
RUN apt-get update \
 && apt-get install -y --no-install-recommends libdbus-1-3 \
 && rm -rf /var/lib/apt/lists/*
COPY --from=stellar-cli /usr/local/bin/stellar /usr/local/bin/stellar
RUN stellar --version
COPY --from=build /src/target/release/orbit-publish /usr/local/bin/orbit-publish
COPY docker/publisher-entrypoint.sh /usr/local/bin/orbit-publisher-entrypoint
USER orbit
ENTRYPOINT ["orbit-publisher-entrypoint"]

FROM runtime AS api
COPY --from=build /src/target/release/orbit-api /usr/local/bin/orbit-api
USER orbit
VOLUME /data
EXPOSE 8080
ENTRYPOINT ["orbit-api", "--bind", "0.0.0.0:8080", "--data-dir", "/data", "--registry", "/etc/orbit/assets.toml"]
