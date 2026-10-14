---
name: spec
description: Write the design RFC and the client-facing target test for a subsystem before any library code exists. Load only when the user explicitly says we are speccing a module or names this skill.
---

# spec

Outside-in design: start from how a client uses the subsystem, derive the API from that.
AGENTS.md applies as usual. This skill only adds phase rules.

## Scope

- **MUST** produce exactly two things: an RFC (Markdown) and a target test.
- **NEVER** write library code. No stubs, no `todo!()` (that's `skeleton`).
- **NEVER** edit a target test the user supplied. It's the fixed point. If it looks wrong, say why and stop.
- **NEVER** settle a design question silently. Put it under Open Questions with options and tradeoffs, and recommend one.

## Steps

1. Read the user's directive and the existing code/RFCs it touches. Nothing else.
2. Target test first. If the user supplied one, skip. Otherwise draft realistic client code for the end-state behavior, including at least one failure case, and one contention case if the subsystem is concurrent. It won't compile yet. Don't add scaffolding to change that.
3. RFC at the path the user gave (default `docs/rfcs/<module>.md`), with these sections:
   - Goals / Non-goals
   - Client usage (points at the target test)
   - Public API sketch: traits, types, signatures as Rust code blocks
   - Guarantees: safety, ordering/visibility, progress (blocking / lock-free / wait-free), error model, `Send`/`Sync` expectations
   - Internal structure: components and their interaction, only as far as needed to judge the API
   - Open Questions
4. Cross-check: every API element the test uses is in the sketch. Every element of the sketch is used by the test or justified. Cut the rest.

## Exit

Report what you wrote, which Open Questions block `skeleton`, and anything in the directive that was ambiguous. If you drafted the target test, say so. Stop. Don't start `skeleton`.
