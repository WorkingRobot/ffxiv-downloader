FROM rust:alpine AS build
USER root
WORKDIR /app
RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static

COPY rust/ .
RUN cargo build --release --bin xiv-dl

FROM alpine AS runtime
WORKDIR /app

COPY --from=build /app/target/release/xiv-dl xiv-dl
ENTRYPOINT ["/app/xiv-dl"]