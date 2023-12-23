FROM rust:latest
RUN apt-get install -y postgres
RUN cargo install diesel_cli --no-default-features --features postgres
COPY scripts/setup.sh setup.sh
ENTRYPOINT ["./setup.sh"]
