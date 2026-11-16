# .tomlx Format Specification

**Version:** 0.2.0
**Status:** Implemented in `format_core::tomlx`; read by the `read_tomlx_*` readers
**Authors:** Human-AI Collaboration

## Abstract

.tomlx (Extended TOML) is a superset of TOML that embeds semantic annotations in comments. It enables human-friendly configuration with machine-optimal output, preserving metadata for validation, documentation, and self-annotating constants.

## Design Philosophy

**Problem:** Human-readable and machine-optimal are often at odds.

```
Human wants:     session = 15  # minutes (scannable, intuitive)
Machine needs:   SESSION_TTL_SECONDS = 900 (efficient, typed)
```

**Solution:** Parse-time transformation with preserved metadata.

- Human writes scannable, intuitive values
- Parser converts to machine-optimal format
- Metadata travels with data (units, path bases)
- Downstream systems use metadata for validation, naming, documentation

**Core Principles:**

1. **Valid TOML** - Every .tomlx file is valid TOML (annotations are in comments)
2. **Zero ambiguity** - Sections are either annotated or not, never mixed
3. **Explicit over implicit** - All conversions require explicit annotations
4. **Helpful errors** - Parser guides users to fixes, not stack traces
5. **Metadata preservation** - Semantic information survives transformation

---

## 1. Syntax Specification

### 1.1 Section Annotations

Section headers may include annotations in trailing comments:

```toml
[section_name]  # key=value, key=value, ...
```

**Annotation format:**
- Starts after `#` following section header
- Key-value pairs separated by `,`
- Keys and values are case-sensitive
- Whitespace around `=` and `,` is ignored

**Required annotation:** At least one of `target=` or `type=`

| Key | Purpose | Description |
|-----|---------|-------------|
| `target=` | Processing mode | Unit conversion (`seconds`, `bytes`) or path expansion (`path`) |
| `type=` | Type declaration | JSON Schema generation (`int`, `str`, `bool`, `list[int]`) |

**Processing modes (target=):**

| Target | Mode | Description |
|--------|------|-------------|
| `seconds`, `ms`, `bytes`, etc. | Unit conversion | Numeric values with unit annotations |
| `path` | Path expansion | String paths with reference annotations |

**Type declarations (type=):**

| Type | JSON Schema | Description |
|------|-------------|-------------|
| `int` | `{"type": "integer"}` | Integer values |
| `float` | `{"type": "number"}` | Floating point values |
| `str` | `{"type": "string"}` | String values |
| `bool` | `{"type": "boolean"}` | Boolean values |
| `list[T]` | `{"type": "array", "items": {...}}` | Arrays of type T |

Sections may have `target=` only, `type=` only, or both.

### 1.2 Field Annotations

Fields within annotated sections must have trailing comment annotations:

```toml
field_name = value  # annotation=value
```

**Unit mode:** `# unit=<unit_name>`
**Path mode:** `# path=<reference_name>`

### 1.3 Inline Human Comments

Human-readable comments may follow annotations using ` // ` delimiter (space-padded):

```toml
session = 15    # unit=minutes // idle user timeout
heartbeat = 30  # unit=seconds // websocket keep-alive
cache = 4       # unit=hours   // CDN refresh interval
```

**Parsing rule:**
1. Split line on ` // ` (space, double-slash, space)
2. Left side: machine annotation (parsed)
3. Right side: human comment (discarded)

**Why space-padded?** Bare `//` appears in valid paths:
```toml
# These are safe - no false split
url = "http://example.com"      # path=base // API endpoint
share = "//network/share"       # path=absolute // Windows UNC
```

Paths don't contain ` // ` with spaces. Human comments naturally do.

**Rationale:** Preserves columnar scannability. Eyes scan: field names → values → units → descriptions. The ` // ` provides clear visual separation between machine and human content.

**Note:** Everything after ` // ` is discarded during parsing. These comments exist only for humans reading the .tomlx source.

### 1.4 Section Rules (Zero Ambiguity)

**Sections with `target=` (unit conversion or path expansion):**
- ALL fields MUST have appropriate annotations (`# unit=` or `# path=`)
- Missing annotation = ERROR

**Sections with `type=` only (no `target=`):**
- Fields do NOT require annotations
- Type applies to all values in section
- Values pass through unchanged (no conversion)
- JSON Schema generated for the section

**Sections with both `type=` and `target=`:**
- Fields MUST have annotations (for `target=` processing)
- Type annotation enables schema generation
- Values are converted AND schema is generated

**Non-annotated sections (no `target=` or `type=`):**
- NO fields MAY have unit/path annotations
- Annotation present = ERROR (orphan annotation)
- Section passes through as regular TOML

---

## 2. Unit Conversion Mode

### 2.1 Section Header

```toml
[section_name]  # target=<target_unit>
```

### 2.2 Field Annotations

```toml
field = value  # unit=<source_unit>
```

The parser converts `value` from `source_unit` to `target_unit`.

### 2.3 Standard Unit Families

#### Time Units

| Unit | Aliases | Base (seconds) |
|------|---------|----------------|
| milliseconds | `ms`, `millisecond`, `millis` | 0.001 |
| seconds | `s`, `sec`, `second`, `secs` | 1 |
| minutes | `m`, `min`, `minute`, `mins` | 60 |
| hours | `h`, `hr`, `hour`, `hrs` | 3600 |
| days | `d`, `day` | 86400 |
| weeks | `w`, `week`, `wk` | 604800 |

**Valid targets:** `seconds`, `ms`, `milliseconds`

#### Size Units (Decimal)

| Unit | Aliases | Base (bytes) |
|------|---------|--------------|
| bytes | `b`, `byte` | 1 |
| kilobytes | `kb`, `kilobyte` | 1000 |
| megabytes | `mb`, `megabyte` | 1000000 |
| gigabytes | `gb`, `gigabyte` | 1000000000 |
| terabytes | `tb`, `terabyte` | 1000000000000 |

#### Size Units (Binary)

| Unit | Aliases | Base (bytes) |
|------|---------|--------------|
| kibibytes | `kib`, `kibibyte` | 1024 |
| mebibytes | `mib`, `mebibyte` | 1048576 |
| gibibytes | `gib`, `gibibyte` | 1073741824 |
| tebibytes | `tib`, `tebibyte` | 1099511627776 |

**Valid targets:** `bytes`, `b`

### 2.4 Conversion Rules

1. Source and target must be in same family (time→time, size→size)
2. Cross-family conversion = ERROR
3. Unknown unit = ERROR (with suggestions)
4. Conversion: `output = input × (source_base / target_base)`

### 2.5 Example

**Input:**
```toml
[ttl]  # target=seconds
session = 15      # unit=minutes
cache = 4         # unit=hours
heartbeat = 30    # unit=seconds

[sizes]  # target=bytes
max_cache = 500   # unit=mb
buffer = 64       # unit=kib
```

**Output:**
```json
{
  "ttl": {
    "session": 900,
    "cache": 14400,
    "heartbeat": 30
  },
  "sizes": {
    "max_cache": 500000000,
    "buffer": 65536
  },
  "section_units": {
    "ttl": "seconds",
    "sizes": "bytes"
  }
}
```

---

## 3. Path Expansion Mode

### 3.1 Document-Level Base Declaration

Path bases are **document-level constants**, declared once in the FIRST `target=path` section:

```toml
# FIRST target=path section: declares ALL bases for entire document
[paths]  # target=path, base=~/ai/phoenix/, logs=/var/log/app/, cache=/tmp/, expand=user
cli = "cli/"        # path=base
tools = "tools/"    # path=base

# SUBSEQUENT target=path sections: inherit bases, NO redeclaration
[logging]  # target=path
app = "app.log"     # path=logs
error = "error.log" # path=logs

[temp]  # target=path
session = "session/"  # path=cache
```

**Rationale:** Just as 60 seconds/minute is universal, `base=~/ai/phoenix/` is universal for the document. Redeclaration invites inconsistency and error.

### 3.2 Section Header Syntax

**First `target=path` section (declaration):**
```toml
[section_name]  # target=path, base=<path>, [name=<path>, ...], expand=<mode>
```

**Subsequent `target=path` sections (inheritance):**
```toml
[section_name]  # target=path
```

**Parameters (first section only):**
- `target=path` - Required, triggers path mode
- `base=<path>` - Primary base path (optional but common)
- `<name>=<path>` - Additional named bases (arbitrary names)
- `expand=<mode>` - Expansion mode (optional, default: `user`)

**Subsequent sections:**
- `target=path` only - bases inherited from first declaration
- Any base definition = ERROR (redeclaration not allowed)

**Expansion modes:**
| Mode | Behavior |
|------|----------|
| `user` | Expand `~` to home directory |
| `env` | Expand `$VAR` and `${VAR}` |
| `all` | Both user and env expansion |
| `none` | No expansion |

### 3.3 Field Annotations

```toml
field = "path/value"  # path=<reference>
```

### 3.3 Path References

**Built-in references (reserved):**

| Reference | Meaning |
|-----------|---------|
| `home` | User's home directory (`~`) |
| `absolute` | No prefix, path used as-is |
| `config` | Relative to config file's directory |

**User-defined references:**
- Any name defined in section header: `base`, `logs`, `cache`, etc.
- Reference to undefined name = ERROR

### 3.4 Path Processing

1. Resolve the reference to its base path
2. If `home`: base = user's home directory
3. If `absolute`: base = "" (empty)
4. If `config`: base = directory containing the .tomlx file
5. If user-defined: base = value from section header
6. Apply expansion (~ and/or $VAR based on `expand` setting)
7. Concatenate: `base + field_value`
8. Normalize path (resolve `..`, remove duplicate `/`)

### 3.5 Example

**Input:** (file at `/project/config.tomlx`)
```toml
[paths]  # target=path, base=~/ai/phoenix/, logs=/var/log/myapp/, expand=user
cli = "cli/"              # path=base
tools = "tools/"          # path=base
app_log = "app.log"       # path=logs
rc = ".myapprc"           # path=home
etc = "/etc/myapp"        # path=absolute
local = "data/"           # path=config
```

**Output:**
```json
{
  "paths": {
    "cli": "/Users/johnny/ai/phoenix/cli/",
    "tools": "/Users/johnny/ai/phoenix/tools/",
    "app_log": "/var/log/myapp/app.log",
    "rc": "/Users/johnny/.myapprc",
    "etc": "/etc/myapp",
    "local": "/project/data/"
  },
  "section_paths": {
    "paths": {
      "base": "/Users/johnny/ai/phoenix/",
      "logs": "/var/log/myapp/",
      "expand": "user"
    }
  }
}
```

---

## 4. Type Declaration Mode

Type declarations enable JSON Schema generation from .tomlx files, allowing type-safe configuration loading without separate schema files.

### 4.1 Section Header

```toml
[section_name]  # type=<type>
[section_name]  # type=<type>, target=<target>
```

The `type` annotation declares the type for ALL values in the section.

### 4.2 Supported Types

| Type | Aliases | JSON Schema |
|------|---------|-------------|
| `int` | `integer` | `{"type": "integer"}` |
| `float` | `number` | `{"type": "number"}` |
| `str` | `string` | `{"type": "string"}` |
| `bool` | `boolean` | `{"type": "boolean"}` |
| `list[int]` | - | `{"type": "array", "items": {"type": "integer"}}` |
| `list[float]` | - | `{"type": "array", "items": {"type": "number"}}` |
| `list[str]` | - | `{"type": "array", "items": {"type": "string"}}` |
| `list[bool]` | - | `{"type": "array", "items": {"type": "boolean"}}` |

Type names are case-insensitive.

### 4.3 Type-Only Sections

When a section has only `type=` (no `target=`):
- Fields do NOT require annotations
- Values pass through unchanged
- JSON Schema is generated for downstream use

```toml
[features]  # type=bool
dark_mode = true
notifications = false
auto_save = true
```

### 4.4 Combined Type and Target

When a section has both `type=` and `target=`:
- Fields MUST have annotations (for target processing)
- Values are converted according to target
- JSON Schema is also generated

```toml
[ttl]  # type=int, target=seconds
session = 15      # unit=minutes
cache = 4         # unit=hours
heartbeat = 30    # unit=seconds
```

Output: Values converted to seconds, schema declares integer type.

### 4.5 Example

**Input:**
```toml
[limits]  # type=int
max_connections = 100
max_retries = 3
buffer_size = 8192

[flags]  # type=bool
debug = false
verbose = true

[tags]  # type=list[str]
categories = ["web", "api", "backend"]
```

**Output:**
```json
{
  "limits": {"max_connections": 100, "max_retries": 3, "buffer_size": 8192},
  "flags": {"debug": false, "verbose": true},
  "tags": {"categories": ["web", "api", "backend"]},
  "schema": {
    "type": "object",
    "properties": {
      "limits": {
        "type": "object",
        "properties": {
          "max_connections": {"type": "integer"},
          "max_retries": {"type": "integer"},
          "buffer_size": {"type": "integer"}
        }
      },
      "flags": {
        "type": "object",
        "properties": {
          "debug": {"type": "boolean"},
          "verbose": {"type": "boolean"}
        }
      },
      "tags": {
        "type": "object",
        "properties": {
          "categories": {"type": "array", "items": {"type": "string"}}
        }
      }
    }
  },
  "section_types": {"limits": "int", "flags": "bool", "tags": "list[str]"}
}
```

### 4.6 Type Validation

When `type=` is declared, the parser validates that values match the declared type:

- `type=int` requires integer values (not strings, not floats)
- `type=float` accepts integers or floats
- `type=str` requires string values
- `type=bool` requires boolean values (`true`/`false`)
- `type=list[T]` requires array where all items match type T

Type mismatch = ERROR with educational message.

---

## 5. Output Schema

### 5.1 JSON Structure

```json
{
  "<section_name>": {
    "<field_name>": "<converted_value>",
    ...
  },
  ...
  "schema": {
    "type": "object",
    "properties": { ... }
  },
  "section_types": {
    "<section_name>": "<type>",
    ...
  },
  "section_units": {
    "<section_name>": "<target_unit>",
    ...
  },
  "section_paths": {
    "<section_name>": {
      "<reference_name>": "<expanded_base>",
      ...
      "expand": "<mode>"
    },
    ...
  }
}
```

### 5.2 Metadata Sections

**`schema`:** Generated JSON Schema (if any type annotations present).
- Only present when at least one section has `type=`
- JSON Schema draft-07 format
- Enables downstream: Pydantic model generation, type validation

**`section_types`:** Maps section names to their declared types.
- Only includes sections with `type=` annotations
- Values are the type string as declared (e.g., `"int"`, `"list[str]"`)
- Enables downstream: type checking, documentation

**`section_units`:** Maps section names to their target units.
- Only includes sections with unit conversion targets
- Enables downstream: constant naming, semantic validation

**`section_paths`:** Maps section names to their path configuration.
- Only includes sections with `target=path`
- Contains all expanded base paths and the expand mode
- Enables downstream: documentation, re-relativization

### 5.3 Non-annotated Sections

Sections without `target=` or `type=` annotations:
- Pass through unchanged as regular TOML→JSON
- Do NOT appear in `schema`, `section_types`, `section_units`, or `section_paths`

---

## 6. Error Conditions

### 6.1 Error Message Format

All errors follow this structure:

```
ERROR: <file>:<line>
  <What is wrong>

  Rule: <The constraint that was violated>

  Fix: <Actionable guidance>
       <Suggestions or alternatives>
```

### 6.2 Syntax Errors

**Orphan unit annotation:**
```
ERROR: config.tomlx:7
  Field 'timeout' has unit annotation but section [settings] has no target.

  Rule: Unit annotations require a section target declaration.

  Fix: Add target to section: [settings]  # target=seconds
       Or remove the annotation if no conversion needed.
```

**Missing required annotation:**
```
ERROR: config.tomlx:12
  Section [ttl] has target=seconds but field 'retry_count' has no unit annotation.

  Rule: All fields in targeted sections must have annotations.
        All units must be convertible to the section target.

  Fix: Add time unit: retry_count = 3  # unit=seconds
       Or move to a non-targeted section if this field doesn't need conversion.
```

### 6.3 Unit Errors

**Unknown unit:**
```
ERROR: config.tomlx:8
  Unknown unit 'mins' on field 'timeout'.

  Did you mean: min, minutes, m?

  Supported time units: ms, s, sec, seconds, m, min, minutes, h, hr, hours, d, days, w, weeks
```

**Incompatible conversion:**
```
ERROR: config.tomlx:15
  Field 'max_size' has unit=mb but section [ttl] has target=seconds.

  Rule: Unit must be convertible to section target.

  Size units (bytes, kb, mb, gb, tb) cannot convert to time units.
  Move this field to a section with target=bytes.
```

### 6.4 Type Errors

**Invalid type syntax:**
```
ERROR: config.tomlx:5
  Section [data] has invalid type annotation 'dictionary'.

  Supported types:
  - Scalars: int, float, str, bool
  - Arrays: list[int], list[float], list[str], list[bool]

  Fix: Use a supported type: [data]  # type=str
```

**Type mismatch:**
```
ERROR: config.tomlx:8
  Field 'timeout' in section [settings] has value that doesn't match declared type.

  Expected type: int
  Actual value: "fifteen"

  Rule: All values in a typed section must match the declared type.

  Fix: Either change the value to match type=int
       Or change the section type annotation.
```

**Array item type mismatch:**
```
ERROR: config.tomlx:12
  Field 'ports' in section [network] has array with items that don't match declared type.

  Expected item type: int
  Found item: "http" (string)

  Rule: All array items must match the declared element type in list[T].

  Fix: Ensure all items are integers: ports = [80, 443, 8080]
```

### 6.5 Path Errors

**Undefined path reference:**
```
ERROR: config.tomlx:20
  Field 'log_dir' uses path=logs but 'logs' is not defined in section header.

  Defined references: base, cache
  Built-in references: home, absolute, config

  Fix: Add to section header: [paths]  # target=path, base=..., logs=/var/log/app/
       Or use a defined reference: # path=base
```

**Invalid path reference:**
```
ERROR: config.tomlx:22
  Field 'data' uses reserved reference 'section_units' which is not a valid path reference.

  Built-in references: home, absolute, config
  User-defined: base, cache (from section header)
```

**Base redeclaration:**
```
ERROR: config.tomlx:25
  Section [logging] attempts to redefine 'base' but path bases are document-level constants.

  Rule: Path bases must be declared in the FIRST target=path section only.
        Subsequent target=path sections inherit these bases.

  'base' was defined at line 5: base=~/ai/phoenix/

  Fix: Remove 'base=...' from this section header.
       Use: [logging]  # target=path
```

### 6.6 TOML Parse Errors

When underlying TOML is malformed, provide best-effort location:

```
ERROR: config.tomlx:5
  Invalid TOML syntax: unterminated string

  [paths]  # target=path
  cli = "cli/
        ^ string not closed

  Fix: Close the string: cli = "cli/"
```

---

## 7. Use Cases

### 7.1 Self-Annotating Constants

**Input:**
```toml
[ttl]  # target=seconds
session = 15  # unit=minutes
```

**Generated constant:**
```rust
const SESSION_TTL_SECONDS: u64 = 900;
```

The constant name encodes: field name + section unit = self-documenting.

### 7.2 Semantic Validation

**Schema definition:**
```json
{
  "ttl": {
    "session": {"max": 604800}
  }
}
```

**Validation with context:**
- User writes: `session = 55  # unit=days` (typo, meant 5)
- Converts to: `4752000` seconds
- Schema max: `604800` (7 days in seconds)
- Validation: FAIL - "session (4752000) exceeds max (604800)"

The schema validates in target units, catching the typo.

### 7.3 Path Consistency

**Input:**
```toml
[paths]  # target=path, base=~/ai/phoenix/, expand=user
cli = "cli/"      # path=base
tools = "tools/"  # path=base
data = "data/"    # path=base
```

**Benefits:**
- Base defined once → typo-proof
- All paths visually consistent → easy to scan
- Change base → all paths update

---

## 8. Implementation Notes

### 8.1 Parser Requirements

1. Parse as valid TOML first
2. Extract section annotations from comments
3. Extract field annotations from comments
4. Validate annotation rules (no mixing, no orphans)
5. Perform conversions/expansions
6. Build output JSON with metadata sections

### 8.2 Comment Preservation

The parser MUST handle:
- Annotations in end-of-line comments only
- Multiple annotations separated by commas
- Whitespace flexibility around `=` and `,`

The parser SHOULD:
- Preserve non-annotation comments in some form (future consideration)

### 8.3 Platform Considerations

**Paths:**
- Always output with forward slashes (`/`) for JSON portability
- Handle platform-specific expansion (`~` on Unix, `%USERPROFILE%` on Windows)
- Normalize path separators during expansion

**Environment variables:**
- `$VAR` and `${VAR}` syntax (Unix-style)
- Consider `%VAR%` for Windows compatibility (future)

---

## 9. Future Considerations

- **Strict path validation mode:** Verify paths exist at parse time
- **Custom unit definitions:** Extensible unit families
- **Nested section annotations:** Inherit target from parent
- **Include directives:** Reference other .tomlx files
- **Windows environment variable syntax:** `%VAR%` support

---

## Appendix A: Quick Reference

### Section Annotations

```toml
# Type declaration only (no conversion)
[limits]  # type=int
[flags]   # type=bool
[tags]    # type=list[str]

# Unit conversion
[section]  # target=seconds

# Combined type and target
[ttl]  # type=int, target=seconds

# Path expansion - FIRST occurrence declares all bases (document-level)
[paths]  # target=path, base=~/.app/, logs=/var/log/, expand=user

# Path expansion - SUBSEQUENT sections inherit bases
[logging]  # target=path
```

### Field Annotations

```toml
# Units (must be convertible to section target)
field = 15  # unit=minutes
field = 30  # unit=seconds

# Paths
field = "subdir/"     # path=base
field = ".config"     # path=home
field = "/etc/app"    # path=absolute
field = "local/"      # path=config
```

### Output Structure

```json
{
  "section": {"field": "converted_value"},
  "schema": {"type": "object", "properties": {...}},
  "section_types": {"section": "int"},
  "section_units": {"section": "target_unit"},
  "section_paths": {"section": {"base": "/expanded/", "expand": "user"}}
}
```
