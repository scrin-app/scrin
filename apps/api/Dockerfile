# syntax=docker/dockerfile:1.12
# scrin accounts API. Build from the repository root:
#   docker build -f apps/api/Dockerfile -t scrin-api .
# Run migrations first (same image): docker run --rm -e DATABASE_URL=… scrin-api dist/migrate.mjs

ARG NODE_IMAGE=node:24-alpine
ARG RUNTIME_IMAGE=gcr.io/distroless/nodejs24-debian13:nonroot

FROM ${NODE_IMAGE} AS build
ENV PNPM_HOME=/pnpm CI=true
ENV PATH=$PNPM_HOME:$PATH
RUN corepack enable
WORKDIR /repo
# Manifests first so the dependency layer caches across source edits.
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY apps/api/package.json apps/api/
RUN --mount=type=cache,id=pnpm,target=/pnpm/store \
    pnpm install --frozen-lockfile --filter @scrin/api... --ignore-scripts
COPY tsconfig.base.json ./
COPY apps/api apps/api
RUN pnpm --filter @scrin/api exec tsdown
# Production-only node_modules for the API, flattened into /out.
RUN --mount=type=cache,id=pnpm,target=/pnpm/store \
    pnpm --filter @scrin/api deploy --prod --ignore-scripts /out \
 && cp -r apps/api/dist apps/api/drizzle /out/

FROM ${RUNTIME_IMAGE}
WORKDIR /app
ENV NODE_ENV=production PORT=8080 MIGRATIONS_DIR=/app/drizzle
COPY --from=build /out/package.json ./
COPY --from=build /out/node_modules ./node_modules
COPY --from=build /out/dist ./dist
COPY --from=build /out/drizzle ./drizzle
USER nonroot
EXPOSE 8080
CMD ["dist/server.mjs"]
