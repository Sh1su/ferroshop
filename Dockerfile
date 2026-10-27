# syntax=docker/dockerfile:1

FROM node:24-alpine AS admin
WORKDIR /admin
COPY admin/package.json admin/package-lock.json admin/.npmrc ./
RUN npm ci
COPY admin/ ./
RUN npm run build

FROM rust:1-slim-bookworm AS backend
WORKDIR /backend
COPY backend/ ./
RUN cargo build --release --locked -p ferroshop-cli

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=backend /backend/target/release/ferroshop-cli /usr/local/bin/ferroshop
COPY --from=admin /admin/dist /srv/admin
ENV ADMIN_DIR=/srv/admin LISTEN=0.0.0.0:8000
EXPOSE 8000
ENTRYPOINT ["ferroshop"]
CMD ["serve", "--migrate"]
