# Builds fully static binaries (musl + rustls, no OpenSSL) and ships each alone
# in an empty image. Build from this folder:
#   docker build -t gh-server .                     # the server (default)
#   docker build --target bench -t gh-bench .       # the load-test tool
FROM rust:1.100-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY . .
RUN cargo build --release --locked --target x86_64-unknown-linux-musl --workspace \
 && cp target/x86_64-unknown-linux-musl/release/gh-server /gh-server \
 && cp target/x86_64-unknown-linux-musl/release/gh-bench /gh-bench

FROM scratch AS bench
COPY --from=build /gh-bench /gh-bench
ENTRYPOINT ["/gh-bench"]

FROM scratch
COPY --from=build /gh-server /gh-server
USER 65534:65534
EXPOSE 8080 9090
ENTRYPOINT ["/gh-server"]
