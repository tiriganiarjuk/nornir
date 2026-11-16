# V2 Zone Architecture — FP Enforcement via Path Classification

## Why This Exists

LLMs generate code sequentially with no persistent working memory. Training data is dominated by OOP examples. When guardrails constrain obvious OOP patterns, the LLM distributes monolith relationships across many small functions that each individually look pure. The functions are clean. The call graph reconstructs the monolith topology. No single node or edge violates anything — but the architecture is fractured OOP in plain sight.

Rules and primers degrade as context fills. Heuristic guardrails catch structural violations but not paradigm-correct syntax with wrong semantics. The solution is an architecture where the directory structure itself makes violations immediately detectable through two simple, orthogonal classification checks.

## The Root Cause

The deepest source of drift is an architectural gap. An LLM with nowhere legal to put orchestration logic or transform logic distributes both across `logic/pure/` — the only overflow valve available. The fragmented monolith is the LLM solving real problems in the worst possible place.

This architecture gives orchestration and transformation zones of their own (orchestrate, transform) and enforces decomposition through cyclomatic complexity bands — not by instruction but by making the wrong thing more expensive than the right thing.

---

## Directory Structure

```
project/
├── structure/                            # L0 — data shape definitions
│   ├── gen/                              # Generated structure files
│   ├── model/                            # Pydantic models
│   ├── config/                           # Enums and static definitions
│   ├── example/                          # Sub-zone with its own matrix row
│   └── exception/                        # Exception classes
│
└── logic/
    ├── orchestrate/
    │   └── {module}/
    │       ├── orchestrate.py            # L7, pipeline wiring
    │       └── dispatch.py               # L6, routing tables
    │
    ├── transform/
    │   └── {module}/
    │       ├── primitive.py              # L1
    │       ├── ffi.py                    # L1, FFI bindings
    │       ├── simple.py                 # L2
    │       ├── dispatch.py               # L3, thin routing
    │       ├── composed.py               # L4
    │       └── assembled.py              # L5, thin composition
    │
    ├── impure/
    │   └── {module}/
    │       ├── primitive.py              # L1
    │       ├── ffi.py                    # L1
    │       ├── simple.py                 # L2
    │       ├── dispatch.py               # L3
    │       ├── composed.py               # L4
    │       └── assembled.py              # L5
    │
    └── pure/
        └── {module}/
            ├── primitive.py              # L1
            ├── ffi.py                    # L1
            ├── simple.py                 # L2
            ├── dispatch.py               # L3
            ├── composed.py               # L4
            └── assembled.py              # L5
```

Not every module needs all levels. The module directory name describes the action (e.g., `grant_expand/`, `section_regroup/`), not the pipeline stage it serves.

---

## Levels — Numbered, Immutable Positions

Levels are numbered positions in the import hierarchy. The number is the level — it never changes. Filenames map to levels, but a filename is not a level. The same filename can map to different levels depending on zone (dispatch.py is L3 in the pure, impure and transform zones, L6 in the orchestrate zone). Different filenames can map to the same level (primitive.py and ffi.py are both L1).

| Level | Filenames | Role |
|-------|-----------|------|
| L0 | *.py in structure/ | Data shape definitions only |
| L1 | primitive.py, ffi.py | Leaf functions — import only structure |
| L2 | simple.py | Building blocks with deps |
| L3 | dispatch.py (pure, impure, transform) | Thin routing over lower levels |
| L4 | composed.py | Multi-branch business logic |
| L5 | assembled.py | Thin composition of L4 functions |
| L6 | dispatch.py (orchestrate zone) | Routing tables for L7 |
| L7 | orchestrate.py | Pipeline wiring across zones |
| L8 | cli.py, __main__.py | Entry point, thinnest wrapper |

Each level from L1 up has a complexity band and its own length, nesting and parameter limits. Their one home is `gleipnir_statistics.toml`.

### Complexity Bands — Gravity and Ceiling

A band is a lower and an upper bound on a function's cyclomatic complexity. Every function in a file must fall inside the band of that file's level.

- Below the band → **gravity violation**: the function is too simple for this level and belongs at a lower one
- Above the band → **ceiling violation**: the function is too complex for this level and belongs at a higher one, or needs decomposition

A level whose band starts at the simplest possible function has nothing that can fall below it; gravity fires at a level whose band starts higher. The statistics file shows which levels those are.

Gravity prevents the LLM from floating simple functions up to a higher level with looser limits. The question at every function is not "can this go here?" but "must this go lower?"

---

## Import Enforcement — Two Rules

A legal import must satisfy both rules:

### Rule 1: Level — higher imports lower

`source.level > target.level`

A file can only import from files at a strictly lower level number. Same-level imports are forbidden — this is the primary anti-monolith binding.

That's it. No lookup table. L6 can import from L5, L4, L3, L2, L1, and L0. L3 can import from L2, L1, and L0. The comparison is the rule.

### Rule 2: Zone visibility

Each zone has a fixed set of zones it can import from:

| Zone | Can import from |
|------|----------------|
| pure | pure |
| transform | transform |
| impure | impure, pure |
| orchestrate | orchestrate, pure, impure, transform |

Structure (L0) is accessible from all zones — it's below every logic level, and a structure target is judged by the level rule alone.

Structure files answer to a boundary of their own: they import one another freely, and from outside structure/ only a short allowed list of modules.

### That's the whole system

- **Pure** is walled — cannot reach impure, transform, or orchestrate.
- **Transform** is fully isolated — cannot reach any other zone, and only orchestrate can reach into it.
- **Impure** can reach into pure (at strictly lower levels, which falls out from the level rule) but cannot reach transform or orchestrate.
- **Orchestrate** sees everything.

### How the Two Rules Work Together

A legal import must pass both Rule 1 and Rule 2:

- `impure/L2` importing `pure/L1` → Level: 2 > 1 ✓, Zone: impure→pure ✓ → **Legal**
- `impure/L2` importing `pure/L2` → Level: 2 = 2 ✗ → **Illegal** (same-level)
- `transform/L2` importing `pure/L1` → Level: 2 > 1 ✓, Zone: transform→pure ✗ → **Illegal**
- `orchestrate/L7` importing `pure/L4` → Level: 7 > 4 ✓, Zone: orchestrate→pure ✓ → **Legal**
- `orchestrate/L6` importing `transform/L5` → Level: 6 > 5 ✓, Zone: orchestrate→transform ✓ → **Legal**
- `orchestrate/L7` importing `orchestrate/L6` → Level: 7 > 6 ✓, Zone: orchestrate→orchestrate ✓ → **Legal**
- `pure/L4` importing `impure/L1` → Level: 4 > 1 ✓, Zone: pure→impure ✗ → **Illegal**
- `impure/L4` importing `transform/L2` → Level: 4 > 2 ✓, Zone: impure→transform ✗ → **Illegal**
- `transform/L5` importing `transform/L4` → Level: 5 > 4 ✓, Zone: transform→transform ✓ → **Legal**

---

## Zone Definitions

**`structure/`** — Data shape definitions only. Its sub-directories:
- **gen/** — Generated structure files.
- **model/** — Pydantic models (BaseModel, RootModel). Typed data shapes.
- **config/** — Enums and static type definitions. Configuration expressed through the type system.
- **example/** — A sub-zone with its own row in the check matrix.
- **exception/** — Exception classes. Error types that carry typed diagnostic information.

gen/, model/, config/ and example/ are sub-zones, each with its own row in `GLEIPNIR_MATRIX.md`. exception/ and every other path under structure/ take the general structure row.

Every class inherits from a Pydantic model base or an Enum base; in exception/ a standard exception base is accepted as well. No functions, no methods, and no module-level data constants (dicts, sets, lists, tuples, frozen sets, lookup tables) — all data is expressed through the type system as enum member values or model field defaults.

**`logic/pure/`** — Pure business logic. Deterministic, no side effects, no IO. Purity is enforced by the import graph: a function that imports from impure/ at any level is by definition impure and doesn't belong here.

**`logic/impure/`** — Logic that touches IO, external calls, system state, or any effect. Impurity is declared at the L1 level and propagates upward through the DAG naturally.

**`logic/transform/`** — Shape conversion. Takes typed structure in, produces typed structure out. No business logic, no IO. Fully isolated — cannot import from pure/ or impure/, and neither can import from transform/. Only orchestrate/ can reach transform/. This isolation prevents the LLM from using transforms as building blocks for monolith reconstruction in other zones.

Transforms operate on already-validated typed data, NOT raw input. The pattern is: schema validate first (permissive entry, union types), then transform (normalize to strict internal type). Pydantic BeforeValidators are banned; coercion is an explicit transform call made from orchestrate/.

At the levels listed under `[v2.transform.*]` in `gleipnir_statistics.toml`, transform functions carry their own complexity ceiling and nesting limit — never looser than pure/impure have at the same level. They are applied over the base level's values; everything they do not name is the base level's. A transform should be a clean shape-to-shape mapping — if it needs deep nesting or complex branching, the logic belongs in pure/ and orchestrate calls both. The tighter bounds prevent transforms from accumulating business logic disguised as shape conversion.

**`logic/orchestrate/`** — Pipeline coordination. Knows the sequence — which zones are called in what order, how data moves between them. Contains no business logic, no shape definitions, no IO, no transformation. Every line is a function call, a variable binding, a conditional branch, or a return.

Two levels:
- **L7 — orchestrate.py** — Pipeline wiring. Calls functions from across the entire stack.
- **L6 — dispatch.py** — Routing tables for orchestration. Typed dispatch dicts mapping keys to callables. No functions, no classes — just imports and typed table assignments. Can reference functions from any level below (L5 and down) because it sits above the logic zone ceiling.

**`logic/*/ffi.py`** — PyO3 Rust functions and other FFI bindings. The internals are a black box from Python's perspective — purity cannot be inferred and must be asserted by placement (pure/ffi.py vs impure/ffi.py vs transform/ffi.py). FFI files are at L1, the same level as primitive — both feed into L2 (simple).

---

## Classification

### Which Files Are Classified

saga chooses which of gleipnir's two entry points a Python file goes to; gleipnir receives no version. The zone rules are the default for Python project code. A project listed in `capability/saga_runner/v1_legacy_projects.toml` (embedded into saga at compile time) takes the V1 rules instead.

In any project, a script (first line `#!/usr/bin/env -S uv run`) and a test file (`conftest.py`, `test_*.py`, `*_test.py`) take the V1 rules.

### File → Level

Classification is deterministic from (filename, zone). Layout: `logic/{zone}/{module}/{level}.py`.

| Filename | Where | Level |
|----------|-------|-------|
| cli.py | any path | L8 |
| __main__.py | any path | L8 |
| *.py | structure/ | L0 |
| primitive.py | pure, impure, transform | L1 |
| ffi.py | pure, impure, transform | L1 |
| simple.py | pure, impure, transform | L2 |
| dispatch.py | pure, impure, transform | L3 |
| composed.py | pure, impure, transform | L4 |
| assembled.py | pure, impure, transform | L5 |
| dispatch.py | orchestrate | L6 |
| orchestrate.py | orchestrate | L7 |
| __init__.py | pure, impure, transform, orchestrate | none |
| any other name | pure, impure, transform, orchestrate | none |
| *.py | no zone | none |

Entry points are recognised by filename, wherever the file lies.

A file with no level has no complexity band. `__init__.py` in a logic zone keeps its zone's checks. Any other filename a logic zone does not use — `orchestrate.py` in pure/, `composed.py` in orchestrate/, a name of the author's own — is an unknown file in that zone and a layout finding.

### Zone from Path

The first of these markers the path contains gives the zone:

| Path contains | Zone |
|---------------|------|
| /logic/pure/ | Pure |
| /logic/impure/ | Impure |
| /logic/transform/ | Transform |
| /logic/orchestrate/ | Orchestrate |
| /structure/gen/ | Structure (gen) |
| /structure/model/ | Structure (model) |
| /structure/config/ | Structure (config) |
| /structure/example/ | Structure (example) |
| /structure/ | Structure |
| none of these | Outside |

A file in no zone takes the `v2.outside` row of `GLEIPNIR_MATRIX.md`: the layout finding, and the checks that need no zone.

---

## How This Defeats the Fragmented Monolith

**Level ordering** — The LLM cannot build object-like clusters because same-level imports are impossible. `source > target` is a total order — the monolith topology cannot form because it requires lateral edges. Structure at L0 is naturally accessible from every logic level (L1+) without special cases.

**Orchestrate/ as formal zone** — Orchestration logic has a legal home of its own. Its complexity band allows necessary wiring while preventing logic accumulation.

**Transform isolation** — Shape conversion code is severed from both pure/ and impure/. The LLM cannot use transforms as building blocks for monolith reconstruction because they're behind a wall only orchestrate/ can reach.

**Complexity bands** — Every function's complexity must fall inside its level's band. The ceiling keeps branching logic out of the low levels; the floor keeps trivial functions out of a level whose band starts higher.

**Two-rule simplicity** — Instead of memorizing a lookup table, the LLM internalizes one numeric comparison and a small zone visibility table. The rules are simple enough to follow without degradation over long contexts.

---

## Gleipnir Enforcement Checklist

The checks that carry this architecture. Which check runs on which classification, and at what severity, is in `GLEIPNIR_MATRIX.md`.

1. **Level import violation** — target level is not strictly lower than the source level
2. **Zone import violation** — the source zone does not reach the target zone (the Rule 2 table): pure importing outside pure, transform importing outside transform, impure importing transform or orchestrate. A structure target passes on level alone
3. **Layout violation** — a file in no zone, a file in a logic zone under a filename that zone does not use, or an import of a module that lies in a zone at no level
4. **Ceiling violation** — function complexity is above the level's band
5. **Gravity violation** — function complexity is below the level's band (function must be pushed down)
6. **Structure violation** — a function, a method or a module-level data constant in structure/, a class without a permitted base, or an import from outside the structure boundary
7. **Constant in logic violation** — module-level data constant in a logic module. A dispatch file holds typed dispatch tables and nothing else

---

## Key Design Decisions

**Numbered levels, not named levels** — The same filename (dispatch.py) maps to different levels depending on zone. Different filenames (primitive.py, ffi.py) map to the same level. Numbered levels make the import rule a single comparison and eliminate the mental mapping entirely.

**No Pydantic BeforeValidators** — Transforms are called explicitly: `model.model_validate_json(data)` then `transform(typed_data)`. This makes every boundary crossing a visible node in the call graph. Schema validates first (permissive entry with union types), transform normalizes second (typed dispatch to strict internal type).

**Dispatch and Assembled as symmetric thin layers** — Both are thin wiring at different positions in the hierarchy. Dispatch (L3) routes between simples (L2). Assembled (L5) composes from composed (L4). Orchestrate-zone dispatch (L6) routes for orchestrate (L7). The pattern repeats at three points in the hierarchy.

**Orchestrate reaches every lower level** — L7 imports any level below it. An IO boundary function (a gate, a registry) sits at the level its own complexity gives it, and orchestrate calls it there.

**No constants in structure/** — All data expressed through the type system. Frozen sets and lookup tables become enum member values.
