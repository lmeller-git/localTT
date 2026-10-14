---
name: optimize
description: Improve performance of working, tested code through measured, one-at-a-time changes with no change to behavior or public API. Load only when the user explicitly asks for optimization or names this skill.
---

# optimize

AGENTS.md applies as usual. This skill only adds phase rules.

## Preconditions

- Test, lint and check recipes are green. If not, stop.
- A benchmark and a profiler are reachable through the repo's recipes. If there's no benchmark, you may write one in the repo's existing bench layout. Any new tooling or bench dependency needs approval.

## Rules

- **MUST** baseline first. Benchmark the target workload and record median and spread. For concurrent code, cover a range of thread counts, including contention and oversubscription.
- **MUST** profile before changing anything. Optimize what the profile shows, not what looks slow.
- **MUST** make one change at a time, re-measure, run the tests. Keep only changes with a clear win outside noise. Revert the rest, and keep each kept change independently revertable.
- **NEVER** change public API, semantics or documented guarantees. If it's needed, stop and report.
- **NEVER** weaken an atomic ordering, add `unsafe`, or drop an invariant check without explicit approval for that specific change. Ask with the justification and a short correctness argument.
- **NEVER** trade correctness or fairness for throughput silently. Report any tail-latency or fairness regression you notice.
- Prefer structural wins (layout, false sharing, allocation removal, batching, algorithm) before micro-tweaks.
- After any change touching atomics or `unsafe`, re-run the repo's concurrency verification recipes.

## Exit

Report:
- a table: change, before, after, tests status, risk
- rejected attempts with reasons
- remaining hotspots

Stop.
