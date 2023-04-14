FROM rust:1.67
WORKDIR .
COPY . .

RUN cargo install --path .

CMD ["moviemail"]