# Builds a fully static binary (musl + rustls, no OpenSSL) and ships it alone
# in an empty image. Build from this folder: docker build -t gh-server .
FROM rust:1.100-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY . .
RUN cargo build --release --locked --target x86_64-unknown-linux-musl \
 && cp target/x86_64-unknown-linux-musl/release/gh-server /gh-server

FROM scratch
COPY --from=build /gh-server /gh-server
USER 65534:65534
EXPOSE 8080
ENTRYPOINT ["/gh-server"]
