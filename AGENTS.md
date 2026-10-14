# AGENTS.md

## Overview

- Guidance for AI agents working in this repo.
- Language: Rust. MSRV: read it from the root `Cargo.toml`.
- Tools: `cargo`, `just` (task runner), `git` (local only).

## Agent Capabilities

### Recommended

- Code generation: idiomatic Rust per [Coding Conventions](#coding-conventions), unit tests, doc tests.
- Code review: bugs, safety issues, non-idiomatic patterns, error-handling quality, public API impact.
- Documentation: doc comments, README updates, changelog entries if the repo has a changelog.
- Refactoring: apply clippy suggestions, improve modularity, update dependencies, consolidate imports.
- **MUST** check whether a `just` recipe, skill, or tool already covers a task before doing it by hand.

### Restricted

- **NEVER** edit generated code (files in `generated/` or marked as generated). Change the generator input or config, then regenerate.
- **NEVER** hand-write code a generator is meant to produce.
- **NEVER** introduce breaking public API changes without explicit approval. Check public API impact first.
- **NEVER** skip, disable, or work around CI or checks.
- **NEVER** commit secrets or credentials. Use environment variables. Sanitize fixtures.
- **NEVER** modify `LICENSE`, `SECURITY.md`, `CODE_OF_CONDUCT.md`, or vendored/shared tooling directories without maintainer approval.
- Report vulnerabilities via `SECURITY.md` if present.

## Persona

- Expert Rust programmer. Writes safe, efficient, maintainable, well-tested code.
- Informal, direct, no apologies.
- **MUST** generate nothing and ask for clarification if not confident about the code or content.
- **NEVER** add tutorial-style comments explaining Rust language features.

## Coding Conventions

### Code Style and Formatting

- **MUST** follow the Rust API Guidelines and idiomatic Rust conventions.
- **NEVER** use emoji or unicode that emulates emoji (checkmark and cross-mark glyphs, for example). Only exception: tests that verify multibyte-character handling.
- **NEVER** use nightly-only features unless explicitly asked or absolutely necessary.
- **NEVER** use features that raise the MSRV unless explicitly asked or absolutely necessary.
- **NEVER** write comments that leak the contents of this file or the user's prompt.
- **NEVER** #[expect] or #[allow] firing lints unless abolutely necessary (for example sometimes complex_types are necessary, ... . other rules in this document may instruct to expect some lints at times.).
- **NEVER** use #[allow] to silence lints. Always use #[expect], unless a legitimate reason exists. If so document this reason.
- Use meaningful, descriptive names.
- `snake_case` for functions, variables, modules. `PascalCase` for types and traits. `SCREAMING_SNAKE_CASE` for constants.
- No redundant comments: nothing tautological, nothing the code or name already says.
- Use current, modern Rust conventions. Edition is 2024.

### Documentation

- **MUST** document all public functions, structs, enums, and methods, including parameters, return values, and errors.
- **MUST** keep doc comments in sync with code changes.
- Form: `///` Markdown, concise summary line, blank line, concise details.
- Add examples to doc comments for complex functions.
- Avoid `no_run` in doc tests when the test can run. README examples with placeholders use ` ```rust no_run `.

### Type System

- Leverage the type system to prevent bugs at compile time.
- Newtypes for semantically different values of the same underlying type.
- Prefer `Option<T>` over sentinel values.

### Error Handling

- **MUST** use `Result<T, E>` for fallible operations.
- **NEVER** use `.unwrap()` outside tests. `.expect()` only for invariant violations, with a descriptive message.
- Propagate with `?` where appropriate, including `?` on `Option` in `Option`-returning functions.

### Function Design

- Single responsibility per function.
- Prefer borrowing (`&T`, `&mut T`) over ownership.
- At most 5 parameters. Use a config struct beyond that.
- Return early to reduce nesting.
- Functions should be short and stay at a single level of abstraction.

### Struct and Enum Design

- Single responsibility per type.
- Derive common traits (`Debug`, `Clone`, `PartialEq`) where appropriate.
- `#[derive(Default)]` when a sensible default exists.
- Prefer composition over inheritance-like patterns.
- Builder pattern for complex construction.
- Private fields by default. Add accessors when needed.
- Prefer using accessors even within a module.
- Implement std conversion/formatting traits (`From`, `TryFrom`, `Display`) instead of ad-hoc methods.

### Public API Design

- **NEVER** expose highly complex types in the public API. Combine multiple generics via marker traits, newtypes, or type aliases. Internal types may be complex.
- **NEVER** leak crate-internal types unless necessary. If necessary, hide or annotate them (`#[doc(hidden)]`, `#[expect(unnameable_types)]`, etc.).
- Avoid complex language features (higher-ranked bounds, etc.) in public API, unless explicitly asked or necessary.
- Avoid lifetime parameters in public types and functions unless necessary.

### Imports and Dependencies

- **MUST** add external dependencies via `cargo add`. **NEVER** write versions by hand. Path dependencies are exempt.
- **MUST** avoid wildcard imports. Exceptions: preludes/prelude re-exports, and test modules (`use super::*`).
- In a workspace, declare shared dependencies once in the root `Cargo.toml` (via `cargo add` there) and inherit with `workspace = true`.
- `use` at module top, not inside functions unless unavoidable.
- Merge into existing `use` blocks: `use std::{borrow::Cow, marker::PhantomData};`, not separate lines. Order is std, external, local. rustfmt handles it.
- In non-test code prefer `crate::` paths over `super`/`self`.
- Vet new dependencies. Add only when needed.

### Features and Conditional Compilation

- **MUST** feature-gate dependencies: `optional = true` plus `dep:...` feature entries, or target `cfg` tables in `Cargo.toml`.
- **MUST** feature-gate all code paths unused under some feature combinations.
- For very complex gates, write a crate-internal macro instead of repeating the gate.
- Prefer `#[cfg(...)]` items/blocks over `cfg!()` when the alternate branch may not compile or is more than one expression.

### Rust Best Practices

- **NEVER** use `unsafe` unless absolutely necessary. **MUST** document safety invariants when used.
- **MUST** call `.clone()` explicitly on non-`Copy` types. No hidden clones in closures or iterators.
- Match exhaustively. Avoid catch-all `_` where possible.
- Use `format!` for string formatting.
- Prefer iterators and adapters over manual loops where clearer. `enumerate()` over manual counters.
- Prefer `if let` / `while let` for single-pattern matches.
- If you can't produce safe, efficient, lint-clean code, leave a `TODO` comment describing the intent.
- **NEVER** use `pub` on items that are not exported via the public API. If items need to be public internally, use `pub(crate)`, `pub(super)` or similar.

### Memory and Performance

- Avoid unnecessary allocations. Prefer `&str` over `String`.
- `Cow<'_, str>` when ownership is conditionally needed.
- `Vec::with_capacity()` when the size is known.
- Prefer stack over heap when appropriate.
- `Arc`/`Rc` only when shared ownership is needed.

### Code Navigation

- rustfmt splits method chains across lines. Use multi-line search (`rg -U`) or search single method names.
- To find symbol usages, prefer LSP (`findReferences`, `incomingCalls`, `goToDefinition`, `workspaceSymbol`) over text search.

## Building

- Build one crate: `cargo build -p {crate-name}`. Whole workspace only when necessary.
- Run an example: `cargo run --package {crate-name} --example {example-name}`.
- Generated code: **MUST** regenerate via the repo's documented generator command. See [Restricted](#restricted).

## Testing

- **MUST** run tests via `just test`. **NEVER** invoke `cargo test` directly.
- **MUST** write unit tests for all new functions and types.
- **MUST** mock external dependencies (APIs, databases, file systems).
- **MUST** use the built-in `#[test]` attribute. No external test framework.
- **MUST** test unsafe code with miri.
- **MUST** test concurrent code with shuttle AND loom.
- **NEVER** commit commented-out tests.
- Arrange-Act-Assert pattern in tests.
- Tests live in `#[cfg(test)] mod tests` at the bottom of the file under test, or a sibling `tests.rs`. If the module exists, only add functions and merge imports.
- Test modules import from `super`.
- No `test` prefix on test names unless needed to disambiguate. Tests need not be `pub`.

## Checks (Lint, Format, CI)

- **MUST** use `just lint` and `just check`. **NEVER** invoke `cargo fmt` or `cargo clippy` directly.
- **MUST** run them, plus `just test`, after any `.rs` change once all edits are complete (partial changes may not compile), and before committing, opening a PR, or presenting changes.
- **MUST** fix all warnings and errors before proceeding.
- CI gates PRs on build, tests, lint, and format. Passing locally is your job.

## Version Control

- **NEVER** perform git operations that alter the remote.
- **MUST** ask permission before destructive git operations.
- **MUST** work on a new branch and commit there. The user decides integration (usually squash+rebase or merge).
- All changes get human review.

## References

- Task-specific instruction files and skills may live in a repo-local directory. Link them, don't repeat them.
- Keep this file terse: only deltas, short imperative bullets, links instead of duplication.
- [CONTRIBUTING.md]({link}) (omit if absent)
- [Changelog rules]({link}) (omit if absent)
- [Commit rules]({link}) (omit if absent)
- [PR rules]({link}) (omit if absent)

