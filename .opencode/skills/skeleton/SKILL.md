---
name: skeleton
description: Turn an approved RFC and target test into compiling Rust types, traits and signatures with todo!() bodies, then confirm the target tests fail at todo!(). Load only when the user explicitly says we are in the skeleton phase or names this skill.
---

# skeleton

Red phase. Declarations only. AGENTS.md applies as usual. This skill only adds phase rules.

## Inputs

Approved RFC and target test. If either is missing, stop and ask.

## Scope

- **MUST** write only declarations: module layout, types, traits, impl blocks, signatures, derives, feature gates, doc comments on public items.
- **MUST** use `todo!()` as every function body. No logic, no early returns, no real constructors, no default values beyond derives.
- **NEVER** edit the target test. If it can't compile against a reasonable skeleton, that's a finding for the report.
- **NEVER** silence warnings with `allow` attributes. Warnings caused only by `todo!()` bodies (unused params, dead code) are exempt from AGENTS.md's fix-all-warnings rule in this phase. Everything else stays clean.

## Steps

1. Lay out modules and declarations from the RFC's API sketch. Where the sketch is silent, take the most conservative option (private, narrower bounds, fewer generics) and record it.
2. Make it compile using the check recipe from AGENTS.md.
3. Run the tests via the test recipe. Each target test **MUST** fail by panicking with the `not yet implemented` message from `todo!()`.
   - Fails another way (assertion, compile error, different panic): test or skeleton is wrong. Find out which and report.
   - Passes: the test is vacuous. Report it, don't fix it.
4. Review your own skeleton for friction a client or implementer will hit: awkward lifetimes, forced allocations, `Send`/`Sync`/`'static` bounds, complexity leaking into public types. Fix what's within the RFC. Report the rest.

## Exit

Report:
- files created
- decisions where the RFC was silent
- one line per target test: name, how it failed
- friction findings

Stop. Don't start `implement`.
