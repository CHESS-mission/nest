# Verification

A component can compile, load, and still never answer. `verify.sh` checks the last part: it
starts a real `nest-server`, puts the component on a bus next to a harness, drives every port
it can, and reads what comes back.

```shell
.agents/skills/nest-component/scripts/verify.sh <component-directory>
.agents/skills/nest-component/scripts/verify.sh --wasm path/to/lm75a.wasm --no-build
```

It needs a built server (`cargo build --release -p nest-server`) and finds the checkout
through `NEST_REPO`, which defaults to the directory containing the skill. It needs no
virtual machine, leaves nothing running, and prints one line per claim:

```text
[ok  ] port i2c  (i2c, bidirectional)
[FAIL] i2c port answers on the bus  (the harness sent a transaction and heard nothing back)

FAILED: 11/12 checks
```

Failures are collected rather than fatal, so a component with three problems shows three
lines.

## What each line means

| Check | A failure says |
|---|---|
| `build`, `artifact exists` | the build failed, or wrote nothing to `build/<name>.wasm` |
| `port <name>` | the world lacks `<name>-in` or `<name>-out`, or the two disagree on a type |
| `imports are all recognized` | the world imports something that is neither `nest:plugin`, a port, an environment read, nor WASI |
| `component README exists` | no front matter to check the rest against |
| `README documents port <name>`, `README agrees with the binary on port <name>`, `README port <name> exists in the binary` | the documentation and the artifact disagree |
| `environment port <name>` | declared in the README, but no `read-<name>` in the world |
| `loads into NEST` | instantiation failed, most often a port type which does not match the topology |
| `accepts the options it documents` | `init` did not return `Ok` for the README's sample options |
| `no host call traps during start`, `… while handling messages`, `… during commands` | the component reached an import NEST does not implement, and the instance is poisoned |
| `<type> port answers on the bus` | the harness sent a real transaction and got nothing back |
| `command <name> is accepted` | the README advertises a command the component rejects |
| `nothing went wrong during the run` | an error surfaced at some point, including late, during an interpreter's startup |

The static checks — names, types, imports, README — need no simulation and are the cheap
ones to run first.

## The run

verify writes a topology in a temporary directory with two `wasm` components, `DEVICE` and
`HARNESS`, and wires them where the port types match:

| The component has | The harness has | Driven by |
|---|---|---|
| an `i2c` port | `i2c` | `i2c-read`, then `i2c-reply` expected back |
| a `bool` port | `lines` | `lines-set` |
| a `bytes` port | `serial` | `serial-send` |

Ports of type `int`, `float` and `text` have no harness counterpart yet, so they are checked
for names and types but not driven; wire those by hand, or extend
`assets/harness/wit/driver.wit` and rebuild.

The harness is an ordinary component, in Rust, in `assets/harness/`:

| Command | Arguments | Effect |
|---|---|---|
| `i2c-read` | `{"addr": 72, "register": 0, "len": 2}` | sends a read request to that address |
| `i2c-write` | `{"addr": 72, "register": 1, "data": [0, 1]}` | sends a write request |
| `i2c-poll` | `{"addr": 72, "register": 0, "len": 1, "period_ms": 100, "count": 5}` | repeats a read on a timer |
| `lines-set` | `{"value": true}` | drives the binary port |
| `serial-send` | `{"bytes": [0, 1]}` | sends bytes on the serial port |

and it emits what it sees: `i2c-reply`, `lines-received`, `serial-received`, and `poll-done`
when a poll runs out. Rebuild it when the WIT of the plugin package changes:

```shell
.agents/skills/nest-component/assets/harness/build.sh
```

It builds on the no-WASI route, so a harness cannot itself trap — which matters, because a
trapping harness reports healthy components as broken.

## The manifest

`README.md` carries YAML front matter, and `verify.sh` reads it: the ports decide what is
diffed against the world, the options are passed to `init`, the commands are sent. A field
nobody reads is a field nobody notices being wrong, so whatever the component does belongs
there.

```yaml
---
name: lm75a
type: temperature sensor
world: example:components/lm75a
tested: rust/wasip2
datasheet: "NXP LM75A, rev. 6, 10 Jun 2020"
ports:
  - name: i2c
    type: i2c
    direction: both
environment:
  - name: temperature
    variable: ambient_temperature
    type: float
options:
  - name: i2c_address
    required: true
    format: "integer, either 0x48 or 72"
    sample: "0x48"
commands:
  - name: set-temp
    arguments: "float, in kelvin"
    sample: 300.0
events:
  - name: temperature
    payload: "float, in degrees Celsius"
---
```

`sample` is what the verifier sends, so an unrealistic sample proves nothing.

## Reading a failure

The report quotes the host rather than guessing:

- `failed to find function export` — the topology and the world disagree about a name
- `... is not implemented by the NEST host: ...` — the component called out of bounds, and
  the import named is the one it called
- `cannot enter component instance` — a trap happened earlier in this run; fix the first one

The rest is in [troubleshooting.md](troubleshooting.md).
