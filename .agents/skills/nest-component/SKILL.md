---
name: nest-component
description: Write a custom WebAssembly component for NEST, in Rust, C, C++, JavaScript or Python, from a datasheet or an interface specification. Use when creating or porting or debugging a component.
---

# Custom WebAssembly components for NEST

A component is a WebAssembly module that NEST loads into a running simulation
and treats as one participant in it: it receives messages on its ports, answers
commands from clients, and sends messages that other components receive. Most
often it models a piece of hardware — a sensor, a transceiver, a regulator —
whose behavior the simulation needs but whose real code cannot run there.

This skill covers what such a component must look like, and how to prove it works.

## The pieces

**NEST** simulates a system: a topology of *components* connected by *wires*,
plus an *environment* of named variables. A topology is YAML; it declares each
component, its type, its configuration and its ports, and pairs ports into
wires. See [the topology reference](../../../docs/reference/topology.md).

**Components** of type `qemu` are virtual machines, and they are where the real
software of the system runs. Components of type `wasm` are this page's subject:
they load a `.wasm` file and expose it to the simulation. Two meanings of
"component" sit next to each other, so be precise where it matters: NEST's
`wasm` component is the loader, and what it loads is a custom WebAssembly
component. Each loaded component gets a thread of its own, and its calls are
serialized on it.

**Wires carry messages, and are typed.** A wire joins two or more ports of the
same type — I²C, a byte stream, a boolean line — into a net, and every one of
them is connected to every other, in both directions. A message put on any port
of the net arrives at all the others, which is what a bus means here.
Components do not address each other; they speak into their ports, and the
topology decides who hears. This is what lets the same component appear twice
in a system, or move to another bus, without a change of code.

**Environment variables are the world outside the model.** Ambient temperature,
supply voltage, a vibration level, the light on a sensor: things no component
produces, but which a component should react to. A client can change one at
runtime (`set_env`), so a scenario can move the world and watch the component
respond. A component reads them through *environment ports*, which are named
and typed in the topology.

## The contract

NEST binds a component by **name**, from its WebAssembly Interface Type (WIT)
world. Nothing inspects the module otherwise, so a name that is wrong by one
character is a port that does not exist, and the failure surfaces when the
simulation starts rather than when the code is written.

| The world declares | NEST uses it for |
|---|---|
| `export init: func(options) -> result<_, init-error>` | called at start, with the component's configuration options |
| `export on-command: func(name, args) -> result<_, command-error>` | a client sent a command |
| `export <port>-in: func(message)` | a message arrived on that wire |
| `import <port>-out: func(message)` | how the component sends on that wire |
| `import read-<name>: func() -> T` | an environment port, reading the variable it names |
| `export on-timeout: func(id)` + `import set-timer` | timers, from the `timeout-ext` world |
| `import emit: func(event, value)` | publishing an event to clients |
| `import log`, `types`, `errors` from `nest:plugin` | logging, the shared types, the error variants |

Everything comes from the `nest:plugin` package: include `nest-component` for
the first four rows, and `timeout-ext` if the component schedules anything. The
types a port may carry, and how each maps to WIT, are in
[references/contract.md](references/contract.md).

One rule has no exception: **a component may call only `nest:plugin` and its
own ports.** A build for `wasm32-wasip2` imports WASI interfaces whether they
are used or not, and calling into one traps with a message naming it. Anything
else a component reaches for — the clock, the filesystem, standard output,
sleeping — has to come from the host.

## Working through a component

### 1. Write down what it does, before writing code

Fill the register table of `README.md` from the datasheet or interface
specification: addresses, reset values, bit meanings, and the conversion from
raw bytes to a physical unit, each with a source. Plausible and wrong are
indistinguishable here, and they both run. When two tables of a document
disagree, or when it says *typical* where the model needs *exact*, ask rather
than choose.

### 2. Scaffold

```shell
.agents/skills/nest-component/scripts/check-env.sh
.agents/skills/nest-component/scripts/new-component.sh --language rust lm75a
```

This writes a project with the WIT of `nest:plugin` copied from the NEST
checkout, so the component is built against the interfaces of this tree rather
than a version fetched from the network. Edit `wit/<name>.wit` until its ports
and reads match the table, and keep the table in the README's front matter:
`verify.sh` diffs that against the compiled world.

### 3. Implement

Rust in `src/lib.rs`, C in `<name>.c`, C++ in `<name>.cpp`, JavaScript in
`app.js`, Python in `app.py` — one page each, linked at the end. The shape is
the same in all of them: state kept in one place, `init` parsing the options,
`<port>-in` matching on the incoming message, `on-command` parsing the
arguments, and every reply leaving through `<port>-out`.

### 4. Build

```shell
.agents/skills/nest-component/scripts/build-component.sh lm75a
```

It prints the artifact and the world's exports and imports. Read that list: a missing
port is cheaper to spot there than in a simulation log.

### 5. Verify

```shell
.agents/skills/nest-component/scripts/verify.sh lm75a
```

This loads the component into `nest-server` next to a harness that drives its
ports, and reports whether it binds, initializes, answers on the bus and
accepts its documented commands. A component that has not passed it is not
finished. What each line means, and what makes it fail, is in
[references/verification.md](references/verification.md).

### 6. Put it in a topology

Copy an example from
[docs/reference/topology.md](../../../docs/reference/topology.md) and add the
component: `type: wasm`, `config.module` pointing at the artifact, `options`,
and the ports it declares, wired to whatever shares their type. The component's
README is what tells whoever writes that file what to expect.

## Choosing a language

| Language | Builds with | WASI imports | Size | Boot | Watch out for |
|---|---|---|---|---|---|
| Rust | `--language rust` | present, trapped if called | 50-80 KB | ms | `thread::sleep` and `std::time`, which are not the clock here |
| Rust, no WASI | `--language rust --route no-wasi` | none | 20-30 KB | ms | `std` time and stdout abort, and code after them is dropped as unreachable |
| C, C++ | `--language c`, `--language cpp` | present, trapped if called | ~270 KB | ms | linking as a *reactor*, not a command |
| JavaScript | `--language js` | none, with `-d all` | 12 MB | ~2 s | an escaping exception traps; throw the error variant |
| Python | `--language python` | none, with `--stub-wasi` | 18 MB | ~2 s | state must be module level; no exception may escape |

Two properties decide whether a language works here. Whether its runtime
reaches for WASI before the first call returns — which is why Python and
JavaScript need their flags, and why a bare build of either does not run. And
how its bindings expose a function imported at the *root* of a world, since
that is the shape a port has; each language page says what that looks like.

## Files

- `scripts/check-env.sh` · `new-component.sh` · `build-component.sh` · `verify.sh`
- `assets/` — world, README and per-language templates, and `assets/harness/`, a component
  that drives buses so that others can be tested without a virtual machine
- `references/contract.md` — ports, types, options, environment variables, timers, limits
- `references/rust.md` · `references/c.md` · `references/js.md` · `references/python.md`
- `references/verification.md` — what `verify.sh` checks and how to read it
- `references/troubleshooting.md` — symptom, then cause
