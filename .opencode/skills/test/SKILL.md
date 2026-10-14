---
name: test
description: Harden implemented code with edge-case unit tests, property tests, mutation testing and concurrency checks, or add tests to existing code. Load only when the user explicitly asks for test hardening or names this skill.
---

# test

Blue phase. AGENTS.md applies as usual, including all testing methodology. This skill only adds phase rules.

## Scope

- **NEVER** change production code. If you find a bug, stop that thread and hand over a failing test plus a short description.
- **NEVER** delete, weaken or `#[ignore]` an existing test. **NEVER** change an assertion to fit current behavior without saying so.
- **NEVER** write pass-through tests that only check a call happened or returned `Ok`. Every test asserts on observable state, output, or an invariant from the RFC.
- **NEVER** use sleeps or timing for synchronization.

## Steps

1. Read the RFC guarantees and the existing tests. List the invariants that have no test.
2. Edge cases: boundaries (0, 1, max, empty, full), error paths, drop and cleanup, ordering.
3. Property tests (`proptest`): one per stateful or algebraic invariant. Prefer a model check against a simple reference implementation. Keep runtime reasonable.
4. Concurrency: if the code has atomics, locks or `unsafe`, run every concurrency verification recipe the repo provides (Miri, loom, shuttle, ...). If there are none, don't add tooling. List what's missing in the report.
5. Mutation testing: use the repo's mutation recipe, scoped to the target modules, never the whole workspace. If there's no recipe, skip and report. Classify each survivor:
   - **Missed behavior:** write a targeted test that fails with the mutation applied and passes on real code. Re-run that mutant to confirm it's killed.
   - **Equivalent** (no observable difference): don't test it. List it with the reasoning.
   - **Timeout or flaky:** list it. Don't paper over it.

   If the only way to kill a mutant is to assert on an implementation detail, list it for the user instead.
6. Run the full test, lint and check recipes. All clean.

## Exit

Report:
- tests added, grouped by invariant
- mutation results per file (killed / survived) with each survivor classified
- bugs found, each with its failing test
- missing tooling

Stop.
