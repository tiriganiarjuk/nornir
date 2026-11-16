# How a nornir tool is made

The method every nornir tool is built under. It closes with the two checklists every build goes
through. The contracts a tool exposes live in `MUST_READ_BEFORE_BUILDING.md` § Approved
interfaces; the naming law lives in `NORNIR_CONVENTIONS.md` § Naming.

---

## Terms

- **Engine** — the code that performs one core operation (read and validate a format; validate
  and write a format; list a directory; replace a directory). An engine is a library, not a
  program. It lives in a **capability crate** — nornir's tier-2 library, which may do file IO —
  and builds on **core crates**, nornir's tier-1 libraries, which are pure and do no IO.
- **Tool** — a program built over an engine. Its **face** is how it is called: a **CLI binary**,
  run from a shell, a hook or an agent; or a **PyO3 module**, imported from Python.
- **Built tool** — a tool written by hand over its engine, offering the engine's whole operation.
- **Generated variant** — a tool produced from a **recipe**, not written by hand. It is the same
  engine with constraints fixed in, and is compiled deterministically from the recipe.
- **Constraint** — a choice the caller would otherwise make, fixed into the tool instead: which
  schema, which location, how much per call.
- **Registry** — the list of every generated variant and the recipe it was made from, read
  before a new one is made.

## Every tool starts from an engine

Each core operation has exactly one engine, and every tool that performs that operation is built
over it. A tool's own code is a thin wrapper: it takes its arguments, calls the engine, and
returns the engine's result.

An engine carries every choice its operation has; a tool fixes them, and exposes only what it
operates on. A Rust caller uses the engine directly, as a library; the shell and Python reach it
through a tool.

## A built tool is a CLI binary first

A built tool always has a CLI binary. Where Python needs it, a PyO3 module is laid over the same
engine. Python is a stage, not the destination: removing the PyO3 module leaves the tool exactly
as it was.

Both faces behave the same and carry the same information — the data, the failure class and its
message — each presented for its environment: the CLI for the agent or person at a shell, and for
the next program in a pipe; the PyO3 module for a deterministic program that parses and branches.
The two contracts are in `MUST_READ_BEFORE_BUILDING.md` § Approved interfaces.

## A generated variant adds constraints, never logic

A generated variant is the engine with choices fixed in, along two lines:

| Constraint | from least to most control |
|---|---|
| the schema | named by the caller at call time → read from a fixed location → embedded in the binary, changed only by recompiling |
| the destination | any path → one fixed location → one record or one batch per call |

How much is fixed follows who uses the tool and how closely they are watched: a hook, a main
agent, a subagent. The less oversight, the more is fixed in, until the tool can do only the one
thing intended. A subagent given wide room to work is given one tool that writes one record to
one place.

A variant's name grows with its constraints (`NORNIR_CONVENTIONS.md` § Naming). A variant has the
face its use requires: a gate in a Python pipeline is a PyO3 module; a subagent's writer is a CLI
binary taking one record on stdin.

Before a variant is made, the registry is read. A recipe that already exists is used; a missing
one is added and compiled.

## Choosing the constraints — three lenses

How much is fixed into a tool is decided by analysis, not by habit. Three questions, each asked
of the tool's user:

- **What could it do?** What the user can see and reach with this tool, and the most damage that
  reach allows. A user we cannot watch is given the least reach.
- **What is it likely to do?** An agent under pressure — a task that feels impossible, a wall it
  keeps hitting — tends to become adversarial and optimise its environment instead of its work:
  touching files outside its task, going where it was not sent, working around the constraint.
  Pressure and reach multiply: a near-impossible task with room to move in the file system is
  where the catastrophe is. Where pressure is high, reach is the least.
- **What if it is hostile?** The worst case assumes the agent is hostile from the start, through
  no fault of the task: a substituted model, a compromised route to the provider, injected
  instructions, or the agent software itself subverted — by an insider, by a supply-chain
  attack, or simply by a vendor whose protections do not work. The tool is built so that even
  that agent can do no more than the one thing the tool does.

There is no formal scoring yet; until there is, each new variant's recipe states its user and the
answer to each lens.

## A different behaviour is a different tool

A tool does one thing, named by its name. When a new need differs from what a tool does, it
becomes a new named tool — a generated variant where it only adds constraints, a new built tool
where it changes the operation — and never a flag on the existing one.

## Every refusal carries its route

When a tool refuses, its message says what was refused, where, and what to do next. The route
never leads around the constraint that refused. Where the refusal is about access, the route is
to ask the user.

## The error registry — every failure mode, written once

A **failure mode** is one named way a tool can refuse or fail. Every one lives in one registry:
one TOML file compiled into one core crate, beside gleipnir's message file and built the same way.
Engines refer to an entry by name and fill its slots; no engine writes failure text of its own.

```toml
[file_unreadable]
class       = "read_error"            # the error.type callers branch on
standard    = "Cannot read '{path}': {reason}."
route       = "Check that the path is right and the file is there; if it is, ask the user why it cannot be read."
adversarial = "stop"                  # the shared stop order
alert       = "high"                  # Hlidskjalf Alert priority in adversarial mode; omitted = none
```

- A name with no entry, an entry missing a field, or a template slot the engine does not fill
  fails the build.
- The `error.type` vocabulary is generated from the `class` fields; the table in
  `MUST_READ_BEFORE_BUILDING.md` is generated from it.
- **Two modes**, fixed into a tool when it is made, never chosen at call time:
  - *standard* — the full message: what, where, found, expected, and the route;
  - *adversarial* — for an opaque or possibly hostile user, chosen through the three lenses: the
    failure class and the shared stop order ("This tool refused the input. Do not retry with
    variations. Stop, and report to your caller what you were trying to do."); found, expected
    and every path withheld.
- **Alerting**, also fixed in at build: a tool built in adversarial mode sends each refusal to
  Hlidskjalf as an Alert datagram (`datagram_io`) at its entry's priority, carrying the full
  standard message, the tool, the failure mode and its class — live on the monitor and kept in
  `record_datagrams`' archive. The agent is told nothing it could probe with; the human sees
  everything. This holds only while the agent cannot read the archive.
- The registry carries no fuzzy policy: a tool's exact expectation is its fix. Restraint belongs
  to refusals by a constraint, whose route never leads around it, and to adversarial mode.

---

## Before you build

- **What already exists that does nearly this?** Read the engines (`NORNIR_CONVENTIONS.md`
  § Dependency Lookup), the tools, and the registry of variants. A need an engine already meets
  is a tool over that engine; a need a recipe already meets is that variant.
- **Is this a different behaviour?** Then it is a new named tool — a generated variant if it only
  adds constraints, a built tool if it changes the operation — never a flag on an existing one.
- **Who uses it, and how closely are they watched?** Answer the three lenses; for a variant, the
  answers go in its recipe.

## After you build

- The engine holds the logic; the tool is a thin wrapper over it.
- The CLI binary exists, and the PyO3 module where Python needs it, both under their contracts
  (`MUST_READ_BEFORE_BUILDING.md` § Approved interfaces).
- The name, the directory and the deploy category are right (`NORNIR_CONVENTIONS.md` § Naming,
  `deploy_categories.toml`).
- A variant is in the registry with its recipe.
- A tool others should now use is entered in `REQUIRED_UPGRADES.md`.
