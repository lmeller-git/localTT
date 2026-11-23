@AGENTS.md

## Claude Code

- `AGENTS.md` is the single source of truth for conventions; this file only adds Claude Code specifics. Edit `AGENTS.md`, not this file, to change rules.
- Skills (`spec`, `skeleton`, `implement`, `test`, `optimize`, `review`) live in `.opencode/skills/` and are exposed to Claude Code through the `.claude/skills` symlink. Edit them in `.opencode/skills/`. Invoke only when the user asks (`/spec`, `/skeleton`, ...).
- Docs under `docs/` are plain Markdown and need no adaptation. Read them on demand via the links in `AGENTS.md`.
- Use `just` recipes for test, lint and check (see `justfile`); never call `cargo fmt`/`cargo clippy` directly.
