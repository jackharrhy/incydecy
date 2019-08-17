FROM ekidd/rust-musl-builder:stable AS builder

ADD . ./

RUN sudo chown -R rust:rust /home/rust

RUN cargo build --release

FROM alpine:latest
COPY --from=builder \
    /home/rust/src/target/x86_64-unknown-linux-musl/release/incydecy \
    /usr/local/bin/

WORKDIR /app

CMD /usr/local/bin/incydecy
