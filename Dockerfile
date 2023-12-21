FROM rust:latest as builder

# Install capnp deps
RUN curl -O https://capnproto.org/capnproto-c++-1.0.1.1.tar.gz
RUN tar zxf capnproto-c++-1.0.1.1.tar.gz
RUN cd capnproto-c++-1.0.1.1 && \
    ./configure && \
    make -j6 check && \
    make install

RUN USER=root cargo new --bin zini
WORKDIR /zini

# Copy manifests
COPY Cargo.lock ./Cargo.lock
COPY Cargo.toml ./Cargo.toml

# Build deps to cache them in docker layer
RUN cargo build --release
RUN rm src/*.rs

# Build application
COPY ./src ./src
RUN rm ./target/release/deps/zini*
RUN cargo build --release

# Stage 2
FROM debian:buster-slim
COPY --from=builder /zini/target/release/zini /usr/local/bin/

# Set entrypoint
ENTRYPOINT ["zini"]
