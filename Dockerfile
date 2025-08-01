FROM rust:alpine AS build
WORKDIR /app

COPY xiv-dl .
RUN cargo build --release

FROM alpine AS runtime

WORKDIR /app
COPY --from=build /app/target/release/xiv-dl .
ENTRYPOINT ["./xiv-dl"]