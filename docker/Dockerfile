
### Fork a suitable rust build environment
FROM rust:1.71-buster as rust-base

# Install proto
RUN apt-get update && apt-get install -y protobuf-compiler

WORKDIR /exch_observer
COPY . .

# Build and install exch_observer
RUN cargo install --root /usr/local --path exch_cli/ --bin exch_cli

RUN mkdir -p /observer_config

# Copy config files
COPY docker/config/ /observer_config

ENV RUST_LOG=info

CMD [ "exch_cli", "launch", "--config", "/observer_config/default.toml" ]
