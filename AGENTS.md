# Agent instructions for gh-core

These rules apply to every AI agent working in this repo (Claude Code, Codex, Cursor, etc.).

## Before you start any work

1. Read **every** file in [`handoffs/`](handoffs/), not just your own contributor's file.
2. In each one, check the **Needs from others** section for anything addressed to your contributor or to the area you're about to touch (client, backend, auth, database, infra, ops).
3. If your work changes something another contributor depends on (an API shape, a route, a file path, a config value), check their handoff first so you don't break it.

## While you work

- Only edit **your own contributor's** handoff file (`handoffs/<name>.md`). Never edit someone else's. If you need something from them, write it in the **Needs from others** section of your own file.
- When a need addressed to you is finished, write it in the **Done for others** section of your own file, with a link to the PR or commit. The other contributor's agent will then remove it from their list.

## Commits and PRs

- **Never list an AI as an author or contributor.** Don't add `Co-Authored-By:` trailers for Claude or any other AI, and don't add "Generated with …" lines to commit messages, PR descriptions or comments. Commits are authored by the human contributor only.
- `.githooks/commit-msg` rejects commits that contain these lines. Enable it once per clone with `git config core.hooksPath .githooks`.

## Before you finish a session

Update your contributor's handoff file:
- what changed (PRs opened or merged),
- decisions made and why,
- new needs from others, and needs that were resolved,
- the date (YYYY-MM-DD) at the top.

The design spec is `docs/GH_Tooling.pdf` once it's added to the repo. Until then, ask the contributor for it.
