# CI/CD & Automated Deployment Guide

> **Pipeline Type:** GitHub Actions Continuous Integration & Continuous Deployment  
> **Workflows Path:** `.github/workflows/ci.yml`, `.github/workflows/cd.yml`

---

## 1. CI Pipeline Architecture (`ci.yml`)

The CI workflow triggers on every push and pull request against `main` and `feat/**` branches.

### Automated Stages:
1. **Dependency Resolution:** `npm ci` with cache optimization.
2. **Type Safety & Linting:** `npm run lint` (`tsc --noEmit`) validates complete TypeScript strict compilation with zero implicit any.
3. **Unit Test Suite:** `npm run test` executes Vitest and React Testing Library tests covering:
   - Zustand store transitions & state synchronization
   - Quartermaster Office interactions & message dispatch
   - Mission Board Kan-ban & Quest Map step execution
   - Captain’s Approval review, signing, and rejection workflows
4. **Vite Production Compilation:** `npm run build` verifies tree-shaking, code splitting, asset generation, and zero bundle errors.
5. **Artifact Archiving:** Uploads `dist/` bundle for staging previews.

---

## 2. CD Pipeline Architecture (`cd.yml`)

The CD workflow triggers upon merged commits into `main` or semantic release tags (`v*.*.*`).

### Automated Stages:
1. **Production Build Compilation:** Generates production-minified assets with tree-shaken dependencies and code-split chunks.
2. **Environment Deployment:** Deploys assets to the hosting environment (Cloud Run / Vercel / self-hosted static file server).
3. **Automated Smoke Health Check:** Queries the Crow's Nest health endpoint to ensure zero regression before switching traffic.

---

## 3. Running Locally

```bash
# Typecheck
npm run lint

# Run unit test suite
npm run test

# Build production bundle
npm run build

# Preview build locally
npm run preview
```
