---
name: review
description: Adversarially review an implementation and its tests for bugs, missing logical coverage, footguns, unidiomatic patterns and forced workarounds, verifying every claim with evidence and reporting findings without fixing them. Load only when the user explicitly asks for an adversarial review or names this skill.
---

# review

Adversarial, read-only pass over code that is already implemented. AGENTS.md applies as usual. This skill only adds phase rules.

## Stance

- The implementation and its tests may share the same misunderstanding. A green test is evidence about its author, not about the code.
- **MUST** assume defects exist until disproven. Your job is to falsify, and to report what survives with the command that proved it.
- **NEVER** fix, refactor or "improve" anything. Findings go to the user; fixes belong to `implement`, extra tests to `test`.

## Inputs

- What is under review, and its baseline. Default to the uncommitted diff or `HEAD..working tree`; if unclear, ask.
- The frozen oracle (target test), the spec (RFC, decisions log) and AGENTS.md. If the author edited the spec or decisions during implementation, treat those edits as claims to verify, not as truth.

## Scope

- **NEVER** edit the frozen target test. If it looks wrong, say why and stop that thread.
- **NEVER** run git operations that change history or the content of the working tree. Leave it byte-identical.
- Probes: prefer a temporary test over a source edit. Keep probes in one clearly named, untracked file, run them, then delete them. **NEVER** edit an existing file to prove a point; describe the minimal patch instead.
- **MUST** cite evidence: a command and its output, or a probe you actually ran. Anything unproven is labelled a hypothesis.
- Targeted invocations are fine while probing. Run the full recipes for the closing verdict.

## Hunt

For every guarantee the spec states, ask: which test fails if this line is subtly wrong? "None" is a finding. Then:

1. **Bugs.** Correctness, atomicity, races, TOCTOU, resource and handle lifetimes, error-class selection, integer and path conversion, encoding and locale, cross-platform behaviour, silent data loss, unreachable or never-produced branches.
2. **Logical coverage gaps.** Contracts tested only as refusals, state written but never read back, branches no test reaches, tests that assert a helper against itself, `expect` messages that overclaim, missing positive halves.
3. **Footguns.** API shapes that invite misuse, surprising fault classes, wall-clock or name collisions, operations that are atomic only under an unenforced assumption.
4. **Unidiomatic or AGENTS-violating code.** Public-API rules, error handling, panic paths, lint workarounds, visibility, dead code, missing doc comments on public items.
5. **Forced workarounds.** Patterns callers or tests must adopt to cope with the design; each marks a leak.
6. **Doc drift.** Spec, decisions and architecture versus code; comments that claim more than the code proves.
7. **Test hygiene.** Fixture duplication, dropped-binding bugs, wall-clock/cwd/PID dependence, order-dependent passes, assertions too loose to fail, flaky assumptions, tooling gates that silently skip targets (scoped package lists, excludes, feature sets).

## Steps

1. Read the diff and the code it touches. Derive the intended contract from the frozen oracle and the spec, independently of the implementation.
2. Apply the hunt list above to the actual tree. For each item: confirm, disprove, or mark hypothesis.
3. Probe the confirmed ones: the smallest test or command that demonstrates the behaviour.
4. Check spec edits and disclosed deviations against the tree. Any behaviour not in the spec and not disclosed is itself a finding.
5. Run the format, lint, check and test recipes. Record whether the tree was green before and after your probes.

## Exit

Report, and stop:

- scope: files read, commands run, what you could not exercise
- verdict in one sentence, plus a ranked top five
- findings grouped BLOCKER / SHOULD-FIX / NIT; each with location, what is wrong, why it matters, evidence, a suggested fix or the test that would catch it, and confidence (confirmed or hypothesis)
- attempted attacks that held, with the command
- reproduction appendix: probe commands and the cleanup you performed

Done means: the working tree is exactly as found, every finding is evidence-backed or explicitly a hypothesis, and the check and test recipes pass.
