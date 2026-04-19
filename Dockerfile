# Stage 1: Build frontend
FROM node:20-slim AS frontend-build
WORKDIR /app/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

# Stage 2: Build backend (includes embedded frontend via include_dir!)
FROM rust:1.85-bookworm AS backend-build
WORKDIR /app
COPY backend/ backend/
COPY db/ db/
COPY --from=frontend-build /app/frontend/dist frontend/dist
WORKDIR /app/backend
RUN cargo build --release

# Stage 3: Runtime
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Run as non-root
RUN useradd --create-home --shell /bin/bash fynance
USER fynance
WORKDIR /home/fynance

COPY --from=backend-build /app/backend/target/release/fynance /usr/local/bin/fynance

ENV FYNANCE_HOST=0.0.0.0
ENV FYNANCE_PORT=7433
ENV FYNANCE_DB_PATH=/home/fynance/data/fynance.db

EXPOSE 7433

ENTRYPOINT ["fynance"]
CMD ["serve", "--no-open"]
