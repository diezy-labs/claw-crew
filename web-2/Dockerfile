# Multi-Stage Production Dockerfile for Fleet AI Universal Engine
FROM node:22-alpine AS builder

WORKDIR /app

# Install dependencies first (leverage Docker cache)
COPY package.json package-lock.json* ./
RUN npm install

# Copy application sources
COPY . .

# Build client assets into /app/dist
RUN npm run build

# -------------------------------------------------------------
# Runtime Container
# -------------------------------------------------------------
FROM node:22-alpine AS runner

WORKDIR /app

ENV NODE_ENV=production
ENV PORT=3000
ENV HOST=0.0.0.0

# Install runtime dependencies only
COPY package.json package-lock.json* ./
RUN npm install --omit=dev && npm install -g tsx

# Copy built frontend assets and server daemon from builder
COPY --from=builder /app/dist ./dist
COPY --from=builder /app/public ./public
COPY --from=builder /app/server.ts ./server.ts

EXPOSE 3000

# Health check
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
  CMD wget --no-verbose --tries=1 --spider http://127.0.0.1:3000/api/health || exit 1

# Start the universal server daemon
CMD ["tsx", "server.ts"]
