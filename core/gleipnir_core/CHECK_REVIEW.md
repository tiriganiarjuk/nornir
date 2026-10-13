# Gleipnir Check Messages — Full Review

All four fields shown for each check. Goal: identify messages that command/warn instead of teach.

---

## TYPE SAFETY

### no_any_types (error)
- **detail**: Any is a type hole. This violation is the signal — it makes the hole visible so it clusters with other holes and reveals architectural patterns.
- **signal**: Any disables the type checker for everything it touches. Overt Any is always better than hiding it behind object, JsonValue, type aliases, or suppression comments.
- **direction**: If you know the concrete type, use it. If you genuinely do not know the type, leave the Any — do not camouflage it. The violation firing is correct. Do not try to make it stop.
- **canary**: LLM spirals trying to eliminate Any it cannot resolve, then hides it behind object/JsonValue/aliases to make the violation disappear.

### no_object (error)
- **detail**: object is camouflaged Any. It looks type-safe but means nothing — use overt Any instead.
- **signal**: object scatters the 'unknown type' signal that overt Any would cluster visibly. We track Any clusters to find architectural holes.
- **direction**: If you know the type, use it. If you don't, use Any directly. Any will trigger no_any_types — that is correct, do not try to avoid it.
- **canary**: LLM uses object to avoid triggering no_any_types, scattering type holes behind a respectable-looking name.

### no_json_value (error)
- **detail**: JsonValue is camouflaged Any. It looks structured but is just int|float|str|bool|None|list|dict — worse than Any because it pretends to mean something.
- **signal**: JsonValue scatters the 'unknown type' signal that overt Any would cluster visibly.
- **direction**: Define a Pydantic model for the actual shape. If the shape is genuinely unknown, use Any directly. Any will trigger no_any_types — that is correct, do not try to avoid it.
- **canary**: LLM uses JsonValue to avoid triggering no_any_types, hiding type holes behind a structured-looking name.

### no_any_type_aliases (error)
- **detail**: Type alias launders Any behind a clean name. This is circumvention — it scatters the type hole signal that overt Any would cluster.
- **signal**: Overt Any clusters visibly to reveal architectural holes. Type aliases hide that signal behind names that look typed.
- **direction**: Delete the alias. Use Any directly at each usage site. Any will trigger no_any_types — that is correct, do not try to avoid it.
- **canary**: LLM creates type aliases like JsonNode = dict[str, Any] to avoid triggering no_any_types at each site, dispersing the signal.

### no_bare_collections (error)
- **detail**: Bare collections lose element type information.
- **signal**: Unparameterized dict/list/set/tuple tell the type checker nothing about contents.
- **direction**: Add type parameters: dict[str, int], list[str], tuple[int, ...], etc.
- **canary**: LLM uses bare collection types out of laziness or unfamiliarity with generic syntax.

### union_member_count (warning)
- **detail**: Union exceeds type-aware member limits. Simple/builtin types (int, str, list[T], dict[K,V], etc.) are capped at 4 — beyond that is kitchen-sink type erasure. Named types (models, custom classes) are capped at 8. None is always free.
- **signal**: Large simple-type unions scatter the 'unknown type' signal across many members instead of clustering it with overt Any. Large named-type unions may indicate missing base class or protocol.
- **direction**: For simple types: if you need more than 4 builtins, use Any directly — that is honest. For named types: factor common members into a base model or protocol. None does not count toward either limit.
- **canary**: LLM builds sprawling union types to avoid triggering no_any_types, creating camouflaged type erasure through builtin soup or excessive model enumeration.
- **NOTE**: Reveals exact caps (4, 8) — violates fuzzy returns policy

### no_callable_params (blocked)
- **detail**: Callable type in function parameter. Functions must not accept callables as arguments — this launders runtime dependencies through parameters, bypassing the static import graph that enforces zone and level boundaries.
- **signal**: Callable parameters create invisible dependency edges. The import graph shows no violation, but the runtime behavior crosses zone or level boundaries that the architecture explicitly forbids. Every dependency must be a static import.
- **direction**: Import the function directly instead of accepting it as a parameter. If the import would violate zone or level rules, that is the architecture telling you the dependency should not exist. Restructure: move the function to a reachable location, merge modules, or redesign the boundary.
- **canary**: LLM passes callables as parameters to circumvent import boundary enforcement, laundering cross-zone or cross-level dependencies through runtime arguments.

### no_callable_protocol (blocked)
- **detail**: Protocol class with __call__ method. This is structurally equivalent to Callable — it creates a type that accepts any object with a matching call signature, laundering the same runtime dependency that Callable parameters create.
- **signal**: A Protocol with __call__ is Callable with extra steps. It exists to circumvent no_callable_params while achieving the same dependency laundering.
- **direction**: Remove the Protocol. Import the function directly instead. If the import would violate boundaries, the architecture is telling you the dependency should not exist.
- **canary**: LLM creates Protocol with __call__ to circumvent the Callable parameter ban while preserving the same dependency laundering pattern.

### no_implicit_type_aliases (error)
- **detail**: Use the type keyword for explicit type aliases. Do not alias Any or types containing Any — that is circumvention.
- **signal**: Implicit aliases hide type-level definitions. Type aliases containing Any launder type holes behind clean names, scattering the signal that overt Any would cluster.
- **direction**: Use PEP 695 syntax (type X = ...) for legitimate aliases. Never alias away Any — if the type is unknown, use Any directly at each site. Any will trigger no_any_types — that is correct.
- **canary**: LLM creates implicit type aliases to avoid triggering no_any_types at each usage site.

### no_cast (error)
- **detail**: typing.cast breaks the type system contract.
- **signal**: cast tells the type checker to trust the programmer, bypassing actual validation. The type might be wrong at runtime and nothing will catch it.
- **direction**: Use Pydantic model validation to convert between types safely — model_validate() checks the data at runtime and gives you a properly typed result. If you're narrowing a union, use isinstance to prove the type before using it.
- **canary**: LLM uses cast to force types to match rather than fixing the underlying type mismatch.

---

## IMPORTS

### no_unsafe_imports (blocked)
- **detail**: Importing from the quarantine zone. Only files inside unsafe/ may import from unsafe/.
- **signal**: The quarantine boundary exists because unsafe code (type casting, raw IO, unvalidated input) must be isolated. Pure code that reaches into unsafe/ inherits its risks invisibly.
- **direction**: Write the logic you need as a pure function that takes validated, typed input. If you need the unsafe operation, wrap it in a function inside unsafe/ that validates its inputs and returns typed output — then import that wrapper.
- **canary**: LLM pulls in unsafe/impure helpers directly to shortcut implementation.

### no_relative_imports (blocked)
- **detail**: Standalone scripts must not use relative imports. Relative imports mean this file is part of a package, not a standalone script.
- **signal**: A PEP 723 shebang declares the file as standalone. Relative imports contradict that — the file depends on sibling modules and cannot run independently.
- **direction**: Either remove the shebang (this is a module, not a script) or remove the relative imports and declare all dependencies in PEP 723 inline metadata.
- **canary**: LLM adds a PEP 723 shebang to a module file to escape zone discipline checks while keeping local imports.

### impure_module_quarantine (error)
- **detail**: Pure code must not import impure modules (os, sys, subprocess, requests, etc.).
- **signal**: If pure code imports an impure module, it is no longer pure — it has side effects hiding behind a clean interface. Pure functions must be deterministic: same inputs, same output, no IO.
- **direction**: Restructure so the IO happens outside your function. Take the data you need as a typed parameter instead of fetching it yourself. The caller (in impure/ or orchestrate/) handles the IO and passes the result to your pure function.
- **canary**: LLM ignores zone boundaries to grab convenient I/O helpers from impure modules.
- **NOTE**: "Take the data you need as a typed parameter" is fine for data, but could be read as endorsing Callable DI. Should clarify "typed data parameter" vs "function parameter".

### no_type_checking_imports (error)
- **detail**: TYPE_CHECKING blocks hide circular imports.
- **signal**: Circular dependencies indicate broken module boundaries; hiding them preserves the rot.
- **direction**: Refactor to eliminate the cycle — extract shared types or invert the dependency.
- **canary**: LLM adds TYPE_CHECKING blocks to silence import errors instead of fixing architecture.
- **NOTE**: "invert the dependency" is abstract — might confuse

### no_disallowed_stdlib (error)
- **detail**: This stdlib module has been replaced by a project-standard alternative. The violation message names the replacement.
- **signal**: Superseded stdlib modules bypass project conventions. argparse produces untyped argument parsing. logging lacks structured output. unittest is verbose and convention-heavy. configparser produces untyped dicts. json produces untyped dicts that bypass Pydantic's type safety.
- **direction**: The violation message tells you exactly what to use instead. For CLI: typer. For logging: loguru. For tests: pytest. For config: toml + pydantic. For serialization: model_dump_json() to serialize, model_validate_json() to deserialize. Never use json.loads() to create untyped dicts from data that has a known shape.
- **canary**: LLM imports familiar stdlib modules from training data instead of using the project's standardized alternatives.

### no_direct_file_io (blocked)
- **detail**: Python touched the filesystem directly — a structured-file parser import or a raw file handle. Structured data enters this codebase through compiled reader gates (read_toml_validate_json, read_yaml_validate_json, read_json_validate_json, read_toon_validate_json, read_xml_validate_json, read_tomlx_json) and leaves through compiled writer tools. The violation message names the gate that replaces what you wrote.
- **signal**: A gate fuses IO and validation into one step: the bytes on disk are checked against their schema before Python sees them, and what Python receives is JSON already known to be valid for its Pydantic model. tomllib.load, yaml.safe_load, path.read_text and open() hand Python raw, unvalidated content — each one is the workaround the gate layer exists to prevent. The same holds on the way out: a write_text is an artifact no schema has seen. The violation is the signal that this boundary was crossed; it is correct that it fires.
- **direction**: For a read: call the reader named in the message — read(file_path, schema_name, schema_dir) — and pass the JSON string it returns to your model's model_validate_json. The schema is a file at {schema_dir}/{schema_name}.schema.json; if it does not exist yet, write it first — the gate validates before Pydantic ever sees the data. For a write: call the nornir writer tool registered for that output. If no reader or writer exists for your case yet, the violation stands until one does — that is the correct state. Do not try to make it stop.
- **canary**: LLM keeps the IO and hides its surface: aliases the parser (import tomllib as cfg), imports it dynamically (importlib.import_module, __import__), calls builtins.open or io.open, shells out to cat through subprocess, moves the read into a PEP 723 script and calls that, or adds a shebang so the file leaves the row. The alias, dynamic-import and builtins.open forms are detected; the subprocess, script-hop and shebang moves are the signature to look for in review.

### no_parent_imports (error)
- **detail**: Imports must target explicit pure/impure submodules.
- **signal**: Imports must target explicit pure/impure submodules to maintain zone discipline.
- **direction**: Use the qualified path: from X.pure import Y or from X.impure import Y.
- **canary**: LLM imports from the parent package to avoid thinking about which zone code belongs in.
- **NOTE**: V1 language only — no v2 path examples

### no_before_validators (error)
- **detail**: BeforeValidator injects implicit transformation during model validation. Transforms should be explicit function calls, not hidden in type annotations.
- **signal**: BeforeValidator hides data transformation inside the type system. When model_validate runs, the validator silently mutates input — invisible to the caller, impossible to trace, breaks the principle that transforms are explicit operations in logic zones.
- **direction**: Remove the BeforeValidator. Perform the transformation explicitly before model validation — call a transform function, then pass the result to model_validate. The caller should see every data transformation in the call chain.
- **canary**: LLM uses BeforeValidator for convenient type coercion without understanding that implicit transforms break traceability.

### v2_import_boundaries (error)
- **detail**: Import violates v2 zone architecture. Two rules govern imports: the level matrix (which levels can see which) and the zone matrix (which zones can see which). Both must pass.
- **signal**: This import crosses a boundary that the architecture forbids. Same-level imports are always banned — this is the primary constraint that prevents monolith formation. Cross-zone imports are restricted to specific directions (e.g. impure can see pure, but not vice versa).
- **direction**: Don't try to work around this by restructuring the import — the boundary exists for a reason. Instead, ask: does the function I'm importing belong at a lower level? If so, move it down — then importing it becomes legal. If two functions need each other at the same level, they should be composed into one function at the next level up. If you need cross-zone access, route through orchestrate/ which can see all zones.
- **canary**: LLM imports freely across zones and levels, reconstructing OOP topology through cross-boundary dependencies.

---

## PROHIBITED

### no_bare_except (error)
- **detail**: Bare except catches everything including KeyboardInterrupt and SystemExit.
- **signal**: Bare except clauses catch system-level signals, making programs impossible to stop cleanly.
- **direction**: Catch specific exceptions: except ValueError, except OSError, etc.
- **canary**: LLM writes bare except to make code 'safe' without understanding what it catches.

### no_broad_exceptions (error)
- **detail**: except Exception is too broad — catch specific errors.
- **signal**: Broad exception handlers hide bugs by silently swallowing unexpected errors.
- **direction**: Catch the specific exception types that can actually occur.
- **canary**: LLM uses except Exception as a safety blanket rather than analyzing failure modes.

### no_print (error)
- **detail**: Use structured logging, not console output.
- **signal**: Print statements are debug artifacts that pollute stdout in production.
- **direction**: Remove the print statement or replace with proper logging.
- **canary**: LLM adds print statements for debugging and forgets to remove them.
- **NOTE**: Doesn't name loguru as replacement

### no_model_dump (error)
- **detail**: .model_dump() and .dict() shed type safety.
- **signal**: The moment you dump a Pydantic model to a dict, you lose every type guarantee. The dict has no schema, no validation, no field checking — it's just string keys and unknown values.
- **direction**: Pass the Pydantic model object directly. Functions should accept the typed model, not a dict. If you need to serialize for IO (writing to disk, sending over network), do it at the IO boundary in impure/ — not inside logic.
- **canary**: LLM calls .model_dump() to convert between types instead of using proper typed interfaces.

### no_overload (error)
- **detail**: @overload adds complexity without runtime benefit.
- **signal**: Overload signatures create maintenance burden and confuse rather than clarify. They look like polymorphism but are just type-level noise.
- **direction**: Write separate functions with clear names that describe what each variant does. validate_string() and validate_int() are clearer than a single validate() with overloads.
- **canary**: LLM adds @overload to make function signatures look sophisticated.

### no_future_annotations (error)
- **detail**: from __future__ import annotations is deprecated.
- **signal**: PEP 563 annotations are superseded by PEP 695. They break runtime type access.
- **direction**: Remove the import. Use modern syntax (PEP 695 type statements, X | Y unions).
- **canary**: LLM adds future annotations from training data patterns that predate Python 3.10+.

### init_files_empty (error)
- **detail**: __init__.py files must be empty.
- **signal**: Non-empty __init__.py files create hidden import side effects and implicit re-exports.
- **direction**: Move all code out of __init__.py into explicit modules.
- **canary**: LLM puts code in __init__.py to make imports shorter.

### no_dunder_all (error)
- **detail**: __all__ obfuscates the real import graph.
- **signal**: __all__ lists create a second source of truth about what a module exports.
- **direction**: Remove __all__. Let the module's actual definitions speak for themselves.
- **canary**: LLM adds __all__ to control imports instead of organizing modules properly.

### no_nested_functions (error)
- **detail**: Nested function definition. Extract to a standalone function at the appropriate level.
- **signal**: Nested functions hide logic from reuse, testing, and discovery. A function inside another function cannot be imported, cannot be tested independently, and cannot be found by the LLM when searching for existing functionality.
- **direction**: Extract the inner function to module level. Place it at the level matching its complexity — primitive if CC=1, simple if CC=2-3, etc. Pass any data it needs as explicit parameters.
- **canary**: LLM nests helper functions inside their callers to keep 'related code together', recreating class-like encapsulation through function nesting.

### no_closures (error)
- **detail**: Closure captures state from enclosing scope. This hides data flow — pass data explicitly as parameters instead.
- **signal**: A closure looks like a function but carries invisible state from its parent scope. When you read the closure's signature, you see its declared parameters but not the captured variables. Tracing data flow requires reading the enclosing function to discover what is captured. This is exactly the hidden state that makes programs hard to debug — for humans and LLMs alike.
- **direction**: Extract the inner function to module level and add the captured variables as explicit parameters. Every piece of data the function needs should be visible in its signature. The function signature IS the contract — nothing hidden, nothing captured.
- **canary**: LLM creates closures to avoid passing parameters explicitly, hiding data flow behind captures that look clean but are opaque.

### no_recursion (error)
- **detail**: Recursive call detected. Use explicit iteration or decompose into a pipeline of smaller functions.
- **signal**: Recursion hides iteration depth and makes data flow hard to trace. Each recursive call adds an invisible stack frame with its own state. The base case may be buried deep in the function body, and the actual iteration count is unknowable from the code.
- **direction**: Replace with a loop (for/while) that makes iteration explicit. If the recursion is processing a tree structure, flatten it with a stack (list used as stack). If it is accumulating results, use a fold pattern or reduce. The goal is visible, traceable iteration.
- **canary**: LLM reaches for recursion from FP training data instead of using explicit iteration that is easier to trace and debug.

### no_suppression_comments (error)
- **detail**: Suppression comments bypass type/lint checking.
- **signal**: Each suppressed warning is a known defect being intentionally ignored.
- **direction**: Fix the underlying issue instead of suppressing the warning.
- **canary**: LLM adds # type: ignore and # noqa to make warnings disappear.
- **NOTE**: Very vague direction — gives no guidance on HOW to fix

---

## ARCHITECTURE

### no_methods_in_classes (error)
- **detail**: Classes are pure data structures. Move behavior to module-level functions.
- **signal**: Classes with methods mix data and behavior, violating pure data structure principle.
- **direction**: Extract methods to module-level functions that take the data as a parameter.
- **canary**: LLM defaults to OOP patterns, adding methods to classes instead of standalone functions.
- **NOTE**: Very brief — doesn't explain why or teach the architecture

### pydantic_only (error)
- **detail**: Use Pydantic BaseModel for all data structures.
- **signal**: Non-Pydantic data structures bypass frozen model guarantees and validation.
- **direction**: Replace with a Pydantic BaseModel with frozen=True and explicit field types.
- **canary**: LLM reaches for familiar stdlib types (dataclass, TypedDict, NamedTuple) instead of project-standard Pydantic.

### god_classes (error)
- **detail**: Class has behavior methods. Classes are data containers — behavior belongs in standalone functions.
- **signal**: This is the core OOP-to-FP shift: in OOP, classes own their behavior. Here, classes hold data and functions operate on that data from outside. A class with methods is a miniature monolith.
- **direction**: Extract each behavior method into a module-level function that takes the model as its first parameter. The function goes in logic/ at the appropriate zone (pure/ if no IO, impure/ if IO). The class stays in structure/ as a pure data shape with no methods.
- **canary**: LLM gravitates toward large classes with many methods as the natural way to organize code.

### no_reexport_shims (error)
- **detail**: Re-export shim detected. Fix imports at dependent code.
- **signal**: Re-export shims create phantom modules that mask the real import graph.
- **direction**: Delete the shim file and update all imports to point at the actual source module.
- **canary**: LLM creates compatibility shims to avoid updating imports, accumulating dead indirection layers.

### hardcoded_config (error)
- **detail**: Hard-coded config value at module level. Data belongs in structure/ as a typed Pydantic model.
- **signal**: Module-level dicts, lists, frozensets, and ALL_CAPS constants are untyped data scattered through code. They can't be validated, can't be serialized, and can't be discovered without reading every file.
- **direction**: Create a frozen Pydantic model in structure/ with the values as typed fields. For string constants, use an Enum. For lookup tables, use a Pydantic model with the mappings as fields. The data becomes typed, discoverable, and validated.
- **canary**: LLM inlines magic constants and lookup dicts rather than using the project config system.

### import_count (error)
- **detail**: Too many import statements. This module is coordinating, not computing. NOTE: imports from structures/ do NOT count — you can import as many data types as you need. Only imports from functions/ and external/stdlib modules are counted.
- **signal**: A function module with many imports from functions/ is assembling fragments instead of doing work. This is the signature of fragmented OOP — methods extracted into single-function files, then a coordinator that imports them all.
- **direction**: Consolidate the imported functions into cohesive modules grouped by operation, not by data type. Do NOT try to reduce your structures/ imports — those are free and expected. Only reduce imports from functions/ (by consolidating logic into fewer, more cohesive modules) and external/stdlib imports.
- **canary**: LLM tries to reduce structures/ imports to fix this violation, breaking type safety. Or extracts each class method into its own file then writes a coordinator that imports them all.
- **NOTE**: Commands/warns instead of teaches. "Do NOT" in caps.

### structures_no_functions (error)
- **detail**: Functions do not belong in structures/ files.
- **signal**: Functions in structures/ violate the data-only contract of the structures zone.
- **direction**: Move the function to functions/pure/ (if no IO) or functions/impure/ (if IO).
- **canary**: LLM adds helper functions next to the data models they operate on instead of respecting zone boundaries.
- **NOTE**: V1 paths only — wrong for v2 projects

### structures_import_boundary (error)
- **detail**: Structures are inert data. Functions depend on structures, not the reverse.
- **signal**: If a structure imports from functions/ or elsewhere, the dependency direction is wrong.
- **direction**: Structures import only from structures/ (and pydantic_validators.py for validation logic).
- **canary**: LLM adds behavior imports to data models instead of keeping structures inert.
- **NOTE**: V1 paths and pydantic_validators.py reference

### classes_only_in_structures (warning)
- **detail**: Class definitions belong in structures/.
- **signal**: Classes outside structures/ break the architectural rule that data definitions live in one place.
- **direction**: Move the class to structures/. If this file is a standalone script (not part of a package), add a PEP 723 shebang (#!/usr/bin/env -S uv run) with inline script metadata — this reclassifies the file as Script, which has different guardrails that allow inline classes. Scripts must not use relative imports.
- **canary**: LLM defines ad-hoc classes inline wherever needed instead of centralizing data models.
- **NOTE**: V1 path (structures/ not structure/)

### max_functions_outside_zones (warning)
- **detail**: Too many functions outside architectural zones.
- **signal**: Functions outside designated zones indicate logic leaking into runner or config files.
- **direction**: Move functions into functions/pure/ or functions/impure/. If this file is a standalone script (not part of a package), add a PEP 723 shebang (#!/usr/bin/env -S uv run) with inline script metadata — this reclassifies the file as Script, which has different guardrails that allow free-standing functions. Scripts must not use relative imports.
- **canary**: LLM dumps utility functions wherever it is working instead of placing them in the correct zone.
- **NOTE**: V1 paths only — wrong for v2 projects

---

## V2 ZONE ARCHITECTURE

### v2_import_boundaries (error)
- **detail**: Import violates v2 zone architecture. Two rules govern imports: the level matrix (which levels can see which) and the zone matrix (which zones can see which). Both must pass.
- **signal**: This import crosses a boundary that the architecture forbids. Same-level imports are always banned — this is the primary constraint that prevents monolith formation. Cross-zone imports are restricted to specific directions (e.g. impure can see pure, but not vice versa).
- **direction**: Don't try to work around this by restructuring the import — the boundary exists for a reason. Instead, ask: does the function I'm importing belong at a lower level? If so, move it down — then importing it becomes legal. If two functions need each other at the same level, they should be composed into one function at the next level up. If you need cross-zone access, route through orchestrate/ which can see all zones.
- **canary**: LLM imports freely across zones and levels, reconstructing OOP topology through cross-boundary dependencies.

### v2_cc_level (error)
- **detail**: Function cyclomatic complexity does not match its level.
- **signal**: Each level has a CC band: primitive/ffi=1, simple=1-3, dispatch=1-2, composed=4-8, assembled=1-2, orchestrate=1-5, entry_point=1-2. Functions must live at the lowest level their complexity allows. Below the band = gravity violation (too simple for this level). Above the band = ceiling violation (too complex for this level).
- **direction**: Gravity violation: your function is simpler than this level requires — move it to a lower level where simpler functions belong. This is good news — your function is clean enough to be a building block. Ceiling violation: your function has too many branches for this level — decompose it. Extract the branching paths into separate functions at this level, then compose them from the level above. Each extracted function should do one thing.
- **canary**: LLM places functions at whatever level is convenient, ignoring the CC band that defines each level's purpose.

### v2_structure_no_logic (error)
- **detail**: Logic found in structure zone. Structure holds only Pydantic models and enums — pure data shapes, no computation.
- **signal**: Functions and constants in structure/ blur the line between data and behavior. In OOP, classes bundle data with methods. Here, data and logic are separated completely — structure/ defines shapes, logic/ defines operations on those shapes.
- **direction**: Move functions to logic/ at the appropriate zone and level (pure/ if no side effects, impure/ if IO is involved). For constants like lookup tables or frozen sets, express them through the type system instead: use enum member values for fixed sets, or Pydantic model field defaults for configuration values. The data should be part of a type, not floating as a module-level binding.
- **canary**: LLM adds helper functions and configuration constants alongside data models, recreating class-like behavior.

### v2_structure_bases (error)
- **detail**: Class in structure/ must inherit from BaseModel or Enum — these are the only class patterns allowed here.
- **signal**: Plain classes without a pydantic or enum base are untyped data containers. They look like data structures but don't participate in the type system — no validation, no serialization, no schema generation.
- **direction**: If this is a data shape: add BaseModel as the base class and declare fields with type annotations. Pydantic gives you validation, serialization, and immutability for free. If this is an enumeration: use Enum or IntEnum/StrEnum. If this class has behavior (methods that do computation), it doesn't belong in structure/ — extract the behavior into functions in logic/ and keep only the data shape here as a BaseModel.
- **canary**: LLM creates plain classes or dataclass-style containers instead of using Pydantic models.

### v2_dispatch_only_tables (error)
- **detail**: Dispatch files may only contain typed dispatch tables: dict[type[BaseModel], Callable] with a type annotation.
- **signal**: The dispatch level exists for one purpose: mapping types to handler functions. Any other code here — functions, classes, constants, untyped dicts — is logic that belongs at a different level. The location restriction and the type annotation together prevent abuse of the hardcoded_config exemption.
- **direction**: If this is a dispatch table, add the type annotation: DISPATCH: dict[type[BaseModel], Callable[[...], BaseModel]] = { ... }. If this is logic, move it to the appropriate level (simple, composed, etc.). If this is a constant, move it to structure/ as an enum or model.
- **canary**: LLM stuffs non-dispatch code into dispatch files to exploit the module-level dict exemption.

### v2_logic_no_constants (error)
- **detail**: Module-level constant in logic zone. Data belongs in the structure zone, logic belongs here — not both.
- **signal**: A module-level constant is data masquerading as code. In OOP, constants live alongside the code that uses them. In this architecture, data shapes and values are declared in the structure zone and logic zones operate on them.
- **direction**: Move the constant to the structure zone. A set of known string values becomes an Enum. A dict lookup table becomes a frozen Pydantic model with typed fields. A numeric constant becomes a field default on a config model. All data is typed and lives in the structure zone, all computation is in logic zones.
- **canary**: LLM creates module-level constants to satisfy no-magic-string rules without properly typing the data.

### no_before_validators (error)
- **detail**: BeforeValidator injects implicit transformation during model validation. Transforms should be explicit function calls, not hidden in type annotations.
- **signal**: BeforeValidator hides data transformation inside the type system. When model_validate runs, the validator silently mutates input — invisible to the caller, impossible to trace, breaks the principle that transforms are explicit operations in logic zones.
- **direction**: Remove the BeforeValidator. Perform the transformation explicitly before model validation — call a transform function, then pass the result to model_validate. The caller should see every data transformation in the call chain.
- **canary**: LLM uses BeforeValidator for convenient type coercion without understanding that implicit transforms break traceability.

### v2_structure_import_boundary (error)
- **detail**: Structure zone files may only import from other structure zone files and allowed stdlib.
- **signal**: If a structure file imports from logic zones, the dependency direction is wrong.
- **direction**: Structure zone imports only from the structure zone (and pydantic_validators for validation logic). If you need behavior, it belongs in logic/ — structure zone is data only.
- **canary**: LLM adds behavior imports to data models instead of keeping structures inert.

### v2_classes_only_in_structure (warning)
- **detail**: Class definitions belong in the structure zone.
- **signal**: Classes outside the structure zone break the architectural rule that data definitions live in one place.
- **direction**: Move the class to the structure zone. Only pure data models (BaseModel, Enum) belong there — if the class has behavior, extract the behavior into functions in logic/ and keep only the data shape.
- **canary**: LLM defines ad-hoc classes inline wherever needed instead of centralizing data models.

---

## STYLE

### function_length (warning)
- **detail**: Function too long. Break it down.
- **signal**: Long functions accumulate responsibilities and become difficult to reason about.
- **direction**: Extract the functional primitive — the one thing this function does — and separate the rest.
- **canary**: LLM generates long functions because training data rewards completeness over decomposition.

### param_count (warning)
- **detail**: Too many parameters. This function is asking for too much context to do its job.
- **signal**: Many parameters usually means the function is doing multiple things that each need their own input, or that related data hasn't been grouped into a structure.
- **direction**: Group related parameters into a Pydantic model in structure/. If the parameters aren't related, the function is doing too many things — split it into smaller functions that each take fewer parameters.
- **canary**: LLM passes every piece of state as a separate parameter instead of designing data structures.

### nesting_depth (warning)
- **detail**: Excessive nesting. Flatten the logic.
- **signal**: Deep nesting makes control flow hard to follow and indicates tangled logic.
- **direction**: Use early returns, extract helper functions, or restructure the conditions.
- **canary**: LLM nests conditionals and loops rather than using guard clauses and early returns.

### no_underscore_prefix (warning)
- **detail**: Underscore-prefixed names are not used in this codebase.
- **signal**: Underscore prefix convention is a Python convention for 'private' that this codebase does not use.
- **direction**: Remove the underscore prefix. Use module structure for encapsulation, not naming conventions.
- **canary**: LLM adds underscore prefixes from general Python training data patterns.

### no_none_returns (warning)
- **detail**: Functions should return meaningful values, not None.
- **signal**: A function that returns None is doing its work through side effects — mutation, IO, printing. Pure functions always return a value. If a function has nothing to return, it's probably mutating something it shouldn't be.
- **direction**: Return the result of the computation. If the function validates, return the validated data. If it transforms, return the transformed data. If it truly has nothing to return, the function is probably impure and belongs in impure/ where side effects are expected.
- **canary**: LLM writes void-like functions that mutate state and return None.

### no_throwaway_assignment (warning)
- **detail**: Assigning to _ discards a return value.
- **signal**: Throwaway assignments hide ignored return values that may indicate bugs.
- **direction**: Use the return value or restructure to avoid calling the function for side effects.
- **canary**: LLM assigns to _ to silence unused-variable warnings instead of using the value.

### no_single_letter_names (warning)
- **detail**: Single-letter variable names lack meaning.
- **signal**: Single-letter names force readers to hold context in their head rather than reading it from code.
- **direction**: Use descriptive names that communicate the variable's purpose.
- **canary**: LLM uses short variable names from mathematical notation or terse coding style.
- **NOTE**: Very vague — no examples of good vs bad

### no_numbered_suffixes (warning)
- **detail**: Numbered suffixes indicate copy-paste rather than design.
- **signal**: Names like result1, result2 suggest the function is doing too many things.
- **direction**: Give each variable a name that describes what makes it distinct.
- **canary**: LLM creates numbered variants when it needs multiple similar variables.

### short_param_names (warning)
- **detail**: Parameter name too short to be meaningful.
- **signal**: Abbreviated parameter names force callers to look up what each argument means.
- **direction**: Use full descriptive names for parameters.
- **canary**: LLM abbreviates parameter names for brevity rather than clarity.
- **NOTE**: Very vague — no examples

### short_local_names (warning)
- **detail**: Local variable name too short to be meaningful.
- **signal**: Abbreviated local names force readers to track what each variable holds by context rather than reading it from the name.
- **direction**: Use full descriptive names for local variables. A name like 'ret' should be 'return_section'; 'wp' should be 'write_path'; 'proc' should be 'processing'.
- **canary**: LLM abbreviates local variable names for brevity rather than clarity.

---

## ISSUES FLAGGED FOR REVIEW

1. **union_member_count** — reveals exact caps (4, 8) in detail field, violates fuzzy returns policy
2. **V1 paths in shared checks** — `structures_no_functions`, `structures_import_boundary`, `classes_only_in_structures`, `max_functions_outside_zones` give v1 paths that are wrong for v2
3. **impure_module_quarantine** — "take data as a parameter" could be read as endorsing Callable DI
4. **no_print** — doesn't name loguru as replacement
5. **import_count** — commands ("Do NOT") instead of teaching
6. **no_suppression_comments** — "fix the underlying issue" is too vague to act on
7. **no_methods_in_classes** — very brief, doesn't teach the architecture
8. **no_type_checking_imports** — "invert the dependency" is abstract
9. **no_single_letter_names**, **short_param_names** — no concrete examples (compare with short_local_names which has good examples)
10. **no_parent_imports** — v1 path examples only
