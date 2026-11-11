# syntax=docker/dockerfile:1.10
FROM rust:1.94.0-alpine3.22 AS toolchain
RUN apk add --no-cache musl-dev perl make
ARG JOBS=4
WORKDIR /src
COPY . .

FROM toolchain AS test
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,id=kebbie-hub-target,target=/src/target \
    cargo test --locked --workspace -j ${JOBS}

FROM toolchain AS build
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,id=kebbie-hub-target,target=/src/target \
    cargo build --locked --release -j ${JOBS} -p kebbie-hub \
    && cp target/release/kebbie-hub /kebbie-hub

FROM toolchain AS sbom-build
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo install --locked cargo-cyclonedx@0.5.10 \
    && cargo cyclonedx --manifest-path crates/kebbie-hub/Cargo.toml -f json --spec-version 1.5 --no-build-deps --override-filename kebbie-hub.cdx

FROM scratch AS sbom
COPY --from=sbom-build /src/crates/kebbie-hub/kebbie-hub.cdx.json /kebbie-hub.cdx.json

FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=build /kebbie-hub /usr/local/bin/kebbie-hub
USER 65532:65532
EXPOSE 8470
ENTRYPOINT ["/usr/local/bin/kebbie-hub"]
