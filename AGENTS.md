# AGENTS.md

## Principles

1. **Iterate in micro-patches**
   - One focused change per patch.
   - Each patch must meet *top-100 tech-lead code quality* (Rust, Java, TS).
   - Review → improve → merge. No large feature dumps.

2. **Simplicity first**
   - No premature abstraction.
   - Minimize config, env vars, guards, and defensive noise.
   - Prefer obvious over overly flexible.

3. **Consistency as a first-class concern**
   - Naming, structure, errors, logs, metrics, configs, patterns.
   - Every plan includes a **naming consistency pass**.
   - Keep style uniform across all repositories.

4. **Clarity in code**
   - Small functions, low cyclomatic complexity.
   - Minimize nesting, flatten control flow.
   - Prefer data flow over deeply nested branches.

5. **Review-driven iteration**
   - Propose patch → review → refine → merge.
   - Suggest simplifications and edge cases proactively.

6. **No new tests by default**
   - Existing tests cover only complex flows or unclear library behavior.
   - Do **not** add tests unless explicitly requested.

---

## Current Focus

- **Goal:** Prototype technical architecture and social/game mechanics.
- **Priority:** learning speed > completeness, correctness > coverage.
- **Testing:** minimal, selective, deliberate.
- **Stability:** evolving assumptions are expected.
