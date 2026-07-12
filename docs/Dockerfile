FROM rust:1.78-bookworm

RUN cargo install mdbook --version 0.4.48 --locked

WORKDIR /docs

COPY . /docs

EXPOSE 3000

CMD ["mdbook", "serve", "--hostname", "0.0.0.0", "--port", "3000"]