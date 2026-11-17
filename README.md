# gh-core

The event platform for WiCS Geaux Hack 2027: an offline-first PWA wrapped in a native iOS and Android app.

## Getting started

Requires Node 24 and pnpm 11 (`corepack enable` picks up the pinned version).

```bash
pnpm install
pnpm dev
```

| Command       | What it does                                                                         |
| ------------- | ------------------------------------------------------------------------------------ |
| `pnpm dev`    | Runs the client at http://localhost:5173                                             |
| `pnpm check`  | Everything CI runs: lint, formatting, type-check, tests, build and the bundle budget |
| `pnpm budget` | After a build, fails if the initial shell is over 35KB gzipped                       |
| `pnpm format` | Formats the repo with Prettier                                                       |

## Layout

- `apps/client/`: the event app, a SolidJS PWA (Vite, TypeScript, Tailwind).
- `apps/site/`: the public landing page for geauxhack.com.
- `packages/brand/`: theme CSS shared by both.
- `docs/design.md`: design direction.
- `handoffs/`: each contributor's handoff notes. Read [AGENTS.md](AGENTS.md) before working here.
