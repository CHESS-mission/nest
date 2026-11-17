# The contract between a component and NEST

NEST reads nothing from a module but the names in its world. It looks up a few exports,
supplies one import per port, and instantiates. The same file loads on the machine that
built it and on the machine that runs it, with no adapter and no installation.

## Contents

- [What NEST looks for](#what-nest-looks-for)
- [Ports](#ports)
- [Port types](#port-types)
- [Options](#options)
- [Environment variables](#environment-variables)
- [Commands](#commands)
- [Events and logs](#events-and-logs)
- [Timers and time](#timers-and-time)
- [Host calls](#host-calls)
- [Execution model](#execution-model)
- [A world, complete](#a-world-complete)

## What NEST looks for

| Item | Direction | Called when | If missing |
|---|---|---|---|
| `init` | export | the simulation starts | the component fails to start |
| `on-command` | export | a client sends a command | the component fails to start |
| `<port>-in` | export | a message arrives on that wire | the component fails to start |
| `on-timeout` | export | a timer set with `set-timer` expires | nothing; timers are silently unused |

The lookup happens at instantiation, so a missing name is an error at that point rather
than a silent absence: the log says `failed to find function export` and the component
never runs.

## Ports

A system port named `p` in the topology is two functions, and both must exist:

```wit
export p-in: func(message: T);    // NEST calls this when something arrives
import p-out: func(message: T);   // the component calls this to send
```

Both use the same type `T`, which must match what the topology declares for the port; a
world which says `u32` where the topology says `int` fails with
`failed to convert function to given type` at start. Naming is exact and case-sensitive,
dash included: an export named `p_in` does not bind.

A wire joins two or more ports into a net, and every port of a net must declare the same
type; a topology which pairs mismatched types, repeats a pair, or puts two ports of the
same component on one wire is rejected when it loads.

The types a system port can carry:

| Topology type | WIT type | Carries |
|---|---|---|
| `bool` | `bool` | a level on a digital line |
| `int` | `i32` | an integer |
| `float` | `f32` | a number |
| `text` | `string` | text |
| `bytes` | `bytes`, which is `list<u8>` | an untyped block of bytes |
| `i2c` | `i2c-transaction` | an address, a register and data, as a request or a response |

`bytes` and the I²C types come from `nest:plugin/types`. Qualified and short spellings are
the same type to the loader. A component sees everything on the wire it is attached to,
including traffic addressed to someone else, because a net broadcasts.

QEMU components expose their serial ports as `bytes` and their GPIO lines as `bool`, which
is what those types exist for; a `qemu` component's own `uart` and `gpio` configuration
keys name which of its ports they are, not a port type of their own.

The I²C type is a variant of a request and a response, each carrying a payload which is
itself a variant:

```wit
variant i2c-transaction {
  request(i2c-request),      // addr: u16, op: write(list<u8>) | read(u32)
  response(i2c-response),    // op: write | read(list<u8>)
}
```

A request is what a master sends, a response what it gets back. Both reach every port on
the wire, so a component which is addressed rather than addressing compares `addr` against
its own and ignores the rest.

## Options

`init` receives the component's `config.options`, as pairs of strings:

```wit
export init: func(options: list<tuple<string, string>>) -> result<_, init-error>;
```

```yaml
- name: lm75a
  type: wasm
  config:
    module: path/to/lm75a.wasm
    options:
      - ["i2c_address", "0x48"]
```

Both key and value are strings, whatever the value means, so a component parses them and
answers with a message when it cannot:

```rust
Err(InitError::UnknownOption(format!("{key}")))
Err(InitError::InvalidValue(format!("{key}={value}")))
```

The host logs those as `Unknown option: …` and `Invalid value: …`. Note that a rejected
`init` does not stop the simulation: the component carries on in whatever state its
`init` left it in, which for a component that needs an address means answering nothing
forever. Prefer a clear failure to a quiet one.

## Environment variables

An environment port named `p` becomes `import read-<p>: func() -> T`, and refers to the
variable named by the port's `variable:` field:

```yaml
environment_variables:
  - name: ambient_temperature
    type: float

components:
  - name: lm75a
    type: wasm
    ports:
      environment:
        - name: temperature       # generates the import `read-temperature`
          variable: ambient_temperature
          type: float             # must equal the variable's type
```

The variables that exist, with the types they can have: `bool`, `int` (`i32`), `float`
(`f32`), `vec2`, `vec3`, `quaternion` and `tensor`, the last four records from
`nest:plugin/types`. A port whose type disagrees with its variable makes the topology
invalid at load, and an unknown variable name likewise.

The distinction that matters when designing a component: **a port carries what other
components produce; an environment port carries what the world is.** If the value has a
producer inside the simulation, it should be a wire. If it is a condition someone sets to
see what happens, it is an environment variable.

## Commands

A client command arrives as `on-command(name, args)`, where `args` is the string rendering
of the JSON value the client sent:

| client sends | `args` contains |
|---|---|
| `{"rate": 10}` | `"{\"rate\": 10}"` |
| `true` | `"true"` |
| `null` | `"null"` |
| `"mode"` | `"mode"` |

Parse it, and answer anything unrecognized with `Err(CommandError::UnknownCommand)`, since
that is what puts a line in the log instead of nothing.

## Events and logs

`emit(event, payload)` publishes to clients and the web app, with `payload` a string —
JSON by convention. Events are the component's public interface to scenarios: keep the
shape stable and document it in the README.

`info`, `warning` and `error`, from `nest:plugin/log`, take a string and cost nothing when
nobody is listening. They are also the only output a component has: standard output belongs
to WASI, and traps.

## Timers and time

```wit
// from nest:plugin/timeout-ext
type timer-id = u32;
export on-timeout: func(timer-id: timer-id);
import set-timer: func(duration-ms: u32) -> timer-id;

// from nest:plugin/nest-component
import get-time: func() -> f64;
```

`set-timer` arms a one-shot timer and returns an identifier, which `on-timeout` receives;
call `set-timer` again to repeat. Both are wall clock: 100 ms means about 100 ms of real
time, and `get-time` counts seconds since the simulation started. Simulated time and
component time are the same clock here, which is worth remembering when a scenario runs
faster or slower than real time.

A component which never includes `timeout-ext` gets no timers, and nothing says so; a
`set-timer` call from a world that did not import it cannot compile, so this fails early.

## Host calls

A component may call `nest:plugin` and its own ports. Anything else traps, by name:

```text
error while executing at wasm backtrace:
    ...
    wasi:cli/stdout@0.2.12#get-stdout is not implemented by the NEST host: components can
    only call the interfaces of the nest:plugin package, and imports of other interfaces,
    such as WASI, always trap
```

The common ways to reach that line are `println!` or `printf`, `thread::sleep`,
`std::time`, reading the environment, and opening a file. The host's equivalents are
`log`, `set-timer`, `get-time`, environment ports, and options.

Imports themselves are harmless: a `wasm32-wasip2` build declares a dozen WASI interfaces
that it never touches. Only a call traps, and after one the instance is poisoned: every
later call into that component fails with `cannot enter component instance` until the
simulation restarts.

## Execution model

Each loaded component runs on a dedicated thread that owns its store, so calls are
serialized: while a handler runs, no other message reaches that component. A handler that
returns normally leaves nothing pending; a component has no background of its own, so
periodic work is a `set-timer` in `init` and another in `on-timeout`.

A handler that never returns is not interrupted. There is no watchdog on a call: the
thread stops answering, wires drop rather than block, and the rest of the simulation
continues without it. If a component's last log line is the start of something, that is
what happened.

Memory grows as the module asks for it; there is no cap configured by the host, so a
component which allocates without bound takes what the machine has and then fails in its
own allocator.

## A world, complete

```wit
package example:components;

world lm75a {
  include nest:plugin/nest-component;         // init, on-command, emit, log, types, errors
  include nest:plugin/timeout-ext;            // on-timeout, set-timer

  use nest:plugin/types.{bytes, i2c-transaction};

  export i2c-in: func(msg: i2c-transaction);
  import i2c-out: func(msg: i2c-transaction);

  import read-temperature: func() -> f32;     // the environment port "temperature"
}
```

This is the world every language page in this skill implements: one port pair, one
environment read, and the `include` lines which bring in the rest.
