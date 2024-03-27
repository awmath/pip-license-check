FROM rust:1.77 as builder
WORKDIR /usr/src/app
COPY . .
RUN cargo install --path .

FROM scratch
COPY --from=builder /usr/local/cargo/bin/pip-license-check /usr/local/bin/pip-license-check
CMD ["pip-license-check"]