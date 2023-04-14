FROM rust:1.67 as builder
WORKDIR .
COPY . .
RUN cargo install --path .

FROM debian:bullseye-slim
COPY --from=builder /usr/local/cargo/bin/moviemail /usr/local/bin/moviemail
CMD ["moviemail"]