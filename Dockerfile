FROM rust:alpine AS build
WORKDIR /app

RUN apk add --no-cache musl-dev openssl-dev

COPY xiv-dl .
RUN cargo build --release

FROM alpine AS runtime

WORKDIR /app
COPY --from=build /app/target/release/xiv-dl .
ENTRYPOINT ["./xiv-dl"]