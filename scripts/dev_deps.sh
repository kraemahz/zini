#!/bin/bash -e
sudo apt install capnproto
cargo install diesel_cli --no-default-features --features postgres
