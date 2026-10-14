---
name: implement
description: Replace todo!() bodies of an approved skeleton with real logic, first as one small spike for the user to review, then unit by unit once approved. Load only when the user explicitly asks to implement (a unit, a spike, or "continue") or names this skill.
---

# implement

Green phase. AGENTS.md applies as usual. This skill only adds phase rules.

## Mode

No state file. Remaining work is whatever `todo!()` is left: `rg 'todo!\('`. The user's message picks the mode:

- Names a unit, says "spike", or nothing is implemented yet: **spike mode**.
- Says "continue" or "approved": **continue mode**.
- Anything else, with some bodies already implemented: ask which.

## Rules (both modes)

- **MUST** work on one atomic unit at a time (a function, or a small struct with its impls). Finish it (compiles, its tests pass) before starting the next.
- **MUST** work bottom-up: dependencies before dependents.
- **NEVER** change the public surface of the skeleton: signatures, bounds, generics, public types, exports. Private helpers and fields are yours. If a public change is needed, stop and write a friction report.
- **NEVER** fake it: no hardcoded values or placeholder returns to turn a test green.
- **MUST** write unit tests for each unit's contract (happy path plus the obvious failure). Edge cases, property tests and mutation hardening belong to `test`.
- **MUST** justify every atomic ordering and every `unsafe` block with a one-line why-comment.
- Leave `todo!()`s outside the current unit alone.
- After each unit run the check and test recipes from AGENTS.md.

## Spike mode

1. Pick the unit: the one the user named. Otherwise the smallest slice that gets part of a target test passing end to end, chosen to stress the riskiest interface decision. State which and why in one line, then go.
2. Implement only that.
3. Write the friction report. Attack the interface, don't defend it. Per point:
   - where (file, item)
   - what (lifetime, forced allocation, `Send`/`Sync` bound, lock or global state, awkward call site, missing method, leaked internal type)
   - why
   - proposed skeleton change
   - blast radius (roughly how many items change)

   If you found nothing, say what you looked at.
4. **Stop.** The user reviews. Don't continue on your own.

## Continue mode

Implement the remaining units in order, without stopping between them. Stop and report only when:

- a public change is needed (friction report as above),
- the RFC or target test is ambiguous or contradicts itself,
- a unit still fails after you've found the root cause and tried a real fix. No blind retries.

## Exit

Report: units done, tests added, check and test status, deviations from the RFC, friction found since the spike. Stop. Don't start `test` or `optimize`.
