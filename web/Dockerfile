# Orbit dashboard. Renders on each request from a running orbit-api (ORBIT_API_URL).
#
#   docker build -t orbit-dashboard web
#   docker run -p 3100:3100 -e ORBIT_API_URL=http://orbit-api:8080 orbit-dashboard

FROM node:22-bookworm-slim AS build
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci
COPY . .
ENV NEXT_STANDALONE=1 NEXT_TELEMETRY_DISABLED=1
RUN npm run build

FROM node:22-bookworm-slim
WORKDIR /app
ENV NODE_ENV=production NEXT_TELEMETRY_DISABLED=1 HOSTNAME=0.0.0.0 PORT=3100
COPY --from=build --chown=node /app/.next/standalone ./
COPY --from=build --chown=node /app/.next/static ./.next/static
USER node
EXPOSE 3100
CMD ["node", "server.js"]
