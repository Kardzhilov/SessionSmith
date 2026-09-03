# SessionSmith website

The product and download site for SessionSmith, built with Vite, React, and
TypeScript. Production is published to GitHub Pages at:

https://kardzhilov.github.io/SessionSmith/

## Development

From the repository root:

```bash
npm --prefix website install
npm --prefix website run dev
```

Vite serves the development site at `http://localhost:5173/`. The production
build uses the `/SessionSmith/` base path required by GitHub project pages.

## Checks

```bash
npm --prefix website run lint
npm --prefix website run build
```

The build output is written to `website/dist/`.

## Deployment

The `Website` GitHub Actions workflow builds and publishes `website/dist` with
GitHub's official Pages actions. It runs on pushes to `main` that touch the
website or workflow, and can also be started manually.

Repository Pages settings must use **GitHub Actions** as the source.
