# Gleipnir Philosophy

## What Gleipnir Is

Gleipnir is a guardrail system that helps LLMs write functional Python code. It runs on every file write via the saga/syn pipeline, using tree-sitter to analyze source code and return violations. Pure computation — no IO.

But the checks are the backstop, not the primary mechanism. The primary mechanism is the architecture itself.

## The Core Problem

LLMs are trained predominantly on OOP code. When asked to write functional code, they default to OOP patterns — classes with methods, mutable state, inheritance hierarchies, defensive exception handling. Instructions and primers degrade as context fills. The LLM drifts back to what its training rewards.

Telling the LLM "write functional code" works for about three exchanges. Then it reverts. The architecture has to make OOP patterns structurally impossible, not merely discouraged.

## Constraints as Pathfinding

The v2 zone architecture reduces the number of valid solutions. An LLM with unlimited options chases its tail — tries one approach, hits a wall, backs up, tries another, cycles. An LLM with 2-3 valid paths converges fast because there aren't enough wrong options to get lost in.

The strict zone and level rules weed out the most obvious dead ends. What remains is a smaller solution space that's much closer to the right answer. We're helping the LLM stack the odds in its favor.

This is not about enforcement. It's about navigation.

## Passing Guardrails Is Not Good Code

```
passing_guardrails != good_code
not_passing_guardrails == bad_code
```

The guardrails are failure detectors, not success metrics. Passing all checks means "didn't trigger any detectors." It does not mean the code is good. The only way to write good code is by trying to write good code. The guardrails catch when the LLM fails at that — they don't certify when it succeeds.

## Educational Violations

When the LLM defaults to knee-jerk OOP and hits a guardrail, the violation doesn't just say "NO." It says "No, that isn't the right way... if you do it this way instead I think you will succeed."

Every violation carries the check's own message for the line it fired on, and four fields from `gleipnir_messages.toml`: detail, signal, direction and canary. The header of that file defines them.

The direction is the critical field. It must be written from the perspective of an OOP-trained LLM that keeps hitting walls and is getting frustrated. Give it exactly one clear path forward. Don't give multiple options — the LLM will pick the easiest one, which is usually wrong. Tell it what to do, not what it could do.

A frustrated LLM goes one of two ways: adversarial (gaming the system) or helpless (giving up). The direction field prevents both by making the right path obvious and achievable.

## The Architecture Guides, The Checks Enforce

The directory structure is the primary teaching tool:

- **structure/** holds data shapes (Pydantic models, enums). No logic, no functions, no constants.
- **logic/pure/** holds deterministic computation. No IO, no side effects.
- **logic/impure/** holds IO operations. Side effects are explicit and isolated.
- **logic/transform/** holds shape conversion. Isolated from both pure and impure.
- **logic/orchestrate/** sequences function calls. No logic, no IO, just wiring.

The LLM can't put a method on a class in structure/ — the check catches it. It can't import os in pure/ — the check catches it. It can't build a long function in primitive.py — the check catches it. Each constraint eliminates a class of OOP patterns.

But more importantly: the LLM learns WHERE things go. After enough violations, it internalizes "data shapes go in structure/, pure computation goes in logic/pure/, IO goes in impure/." The architecture becomes the mental model.

## Level System and Decomposition Pressure

The level system creates decomposition pressure through cyclomatic complexity bands and function length limits. A file's name gives its level, and each level has its own band and limits in `gleipnir_statistics.toml`:

- **ffi**: Black-box FFI bindings. Peers with primitive — neither imports the other.
- **primitive**: Does exactly one thing. Imports only structure.
- **simple**: Combines primitives with light branching.
- **dispatch**: Thin routing tables. Typed dicts mapping keys to callables.
- **composed**: Multi-branch business logic.
- **assembled**: Thin composition of composed functions.
- **orchestrate**: Sequences zone calls. Wiring, not logic.
- **entry point**: cli.py or __main__.py, recognised by filename. Thinnest wrapper.

A function's complexity must fall inside its level's band. Above the band is a ceiling violation — the function belongs higher, or needs decomposition. Below the band is a gravity violation — the function is too simple for its level and must move down. Gravity fires at a level whose band starts above the simplest possible function, and it prevents the LLM from floating everything up to a level with looser limits.

Same-level imports are banned. This is the primary anti-monolith binding. The LLM cannot build object-like clusters because functions at the same level cannot import each other. If two functions need each other, they must be composed at the next level up.

**Callable parameters are banned.** Passing functions as arguments launders dependencies through runtime parameters, bypassing the static import graph. If a function needs another function, it must import it directly. If the import would violate zone or level rules, the architecture is saying the dependency should not exist.

## Type Safety at the Boundary

The system enforces strict typing at every boundary. Every v2 zone holds typed data only. Every data source has a known shape:

- JSON generated by us → validate with the same Pydantic model on ingestion
- External data → build a Pydantic model with the expected shape
- Passes validation → typed, safe, enters the system
- Fails validation → rejected at the door

This is binary. No middle ground, no "partially typed," no coercion. `.model_dump()` is banned because it sheds types into untyped dicts. `import json` is banned throughout project code — scripts and test files are the exception, and `GLEIPNIR_MATRIX.md` has the row — because `json.loads()` produces untyped dicts. The only serialization path is `.model_dump_json()` → JSON string → `model_validate_json()` at the other end. Types are preserved because the data never exists as a manipulable untyped dict.

## Why "Resilient" Code Is The Enemy

LLM training rewards code that handles everything gracefully — catching exceptions, providing defaults, coercing types, handling None. This produces code that never crashes. Which means bugs are invisible.

Change a field name three levels up. In "resilient" code, nothing breaks. Every layer catches the mismatch, provides a default, and passes wrong data downstream. The bug surfaces as subtly wrong output with no traceback.

In strict Pydantic validation, the same change causes an immediate, loud failure at the exact boundary where the shape mismatch hits. The break IS the diagnostic signal. Fail fast, fail loud, fail at the right place.

Real resilience is crashing at the right place with the right information — not absorbing errors until the system silently corrupts.

## Natural Clustering and LLM Strengths

LLMs are bad at connecting things across files (limited context, no persistent working memory). But they're good at recognizing patterns within visible context.

The v2 architecture exploits this asymmetry. Dependency ordering in structure/ forces simpler shared models to float above the complex ones that use them. Similar models at the same abstraction level cluster in the same file. The LLM sees them together and naturally extracts shared bases.

We don't need a machine to detect overlap. The architecture makes the overlap visible. The LLM does the pattern recognition it's naturally good at.

## Thresholds as Tunable Data

All numeric enforcement parameters live in `gleipnir_statistics.toml` — embedded at compile time, locked with `chflags schg` so LLMs cannot modify their own constraints. The human changes thresholds, rebuilds, observes effects, tunes. The LLM never gets to negotiate.

The numbers are empirical, not theoretical. They'll be wrong. But they create pressure in the right direction, and experience will tell us where to tighten or loosen.
