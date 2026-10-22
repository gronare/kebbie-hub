# syntax=docker/dockerfile:1.10
FROM rust:1.94.0-alpine3.22 AS toolchain
RUN apk add --no-cache musl-dev perl make
ARG JOBS=4
WORKDIR /src
COPY . .

FROM toolchain AS test
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo test --locked --workspace -j ${JOBS}

FROM toolchain AS build
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --locked --release -j ${JOBS} -p kelpie-hub \
    && cp target/release/kelpie-hub /kelpie-hub

FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=build /kelpie-hub /usr/local/bin/kelpie-hub
USER 65532:65532
EXPOSE 8470
ENTRYPOINT ["/usr/local/bin/kelpie-hub"]
