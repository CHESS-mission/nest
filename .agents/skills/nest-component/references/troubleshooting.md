# Things which go wrong, and what they mean

Ordered by the message you see. Most of these are invisible until the component runs, which
is what `verify.sh` is for.

## `failed to find function export`

The topology declares a port the component does not export, or the world is missing `init`
or `on-command`. The component's thread stops there, so nothing at all is logged by it, and
the component looks dead rather than wrong.

Compare the `system:` list of the topology with the `export <port>-in` lines of
`wasm-tools component wit build/<name>.wasm`. A port named `i2c` in YAML needs `i2c-in` and
`i2c-out`; an export named `i2c_in`, with an underscore, does not match.

## `failed to convert function to given type`

The name matched, the signature did not: the host calls a port's export with the type the
topology declares, and the world declared another. `int` is `i32`, not `u32`; `bytes` and
`list<u8>` are the same type, so spelling is never the cause. The table is in
[contract.md](contract.md).

## `... is not implemented by the NEST host`

The component called outside `nest:plugin` and its own ports. The message names the import it
reached, and the frame above it says which call got there:

```text
error while executing at wasm backtrace:
    0:   0x782d - <unknown>!<wasm function 165>
    5:   0x2415 - <unknown>!<wasm function 41>: wasi:cli/stdout@0.2.12#get-stdout
       is not implemented by the NEST host: components can only call the interfaces of
       the nest:plugin package, and imports of other interfaces, such as WASI, always trap
```

What reaches it: `println!` or `printf`, `thread::sleep`, `std::time`, reading the process
environment, opening a file. What replaces each: `log`, `set-timer`, `get-time`, options, and
nothing.

A `wasm32-wasip2` build imports a dozen WASI interfaces whether they are used or not. An
import is not a call, and nothing is wrong until code reaches for one.

## `cannot enter component instance`

An earlier call trapped and the instance is poisoned. This is a symptom: scroll up to the
first error, which holds the cause. It lasts until the simulation restarts, so an `init`
which panics makes the rest of the run silent rather than failing loudly.

## The component loads, and nothing happens

In order of likelihood:

1. **The address filter.** A component answers only transactions aimed at its address. Log
   the address parsed in `init` and the address of each incoming transaction, once. This
   catches `0x48` versus `0x90` — 7-bit against 8-bit — the most common single error in an
   I²C model.
2. **A dangling read.** A `Read` request carries an address and a length; the bytes come from
   the component's own pointer state, so if nothing ever set that pointer the reply is empty.
3. **`on-timeout` missing from the world.** `set-timer` returns an id and the host keeps the
   appointment, but a component which does not export `on-timeout` never hears about it, and
   that is not an error. Include `nest:plugin/timeout-ext` *and* implement the export.
4. **The component loops.** Nothing watches a call: one which never returns simply stops
   answering while the rest of the simulation carries on. If the last log line is the
   beginning of something, this is what happened.
5. **The reply was never sent.** A handler which computes and returns without calling
   `<port>-out` is well typed and mute.

## `Unknown option: <name>`, or `Invalid value: <name>`

`init` rejected the topology's options, and the component goes on with whatever state it
reached. A component which needs an address and does not get one will answer nothing forever,
so a missing option deserves a refusal rather than a warning. Names come from YAML, so
`i2c-address` and `i2c_address` are two different options.

## A wire is refused when the topology loads

Three messages, three mistakes, all of them about the wire rather than the component:

- `Cannot wire <a>.<p> (type <t>) to <b>.<q> (type <u>): wired ports must have the same
  type` — two endpoints of a net disagree. A wire joins every port listed on it, so the
  check runs across all pairs, and the reported pair need not be adjacent in the YAML.
- `All endpoints of a wire must belong to different components, but component <x> appears
  twice in the same wire` — a loop back onto one component, which nothing routes.
- `Duplicate wire between <a> and <b>` — the same pair on two wires. Nets are sets of
  connections, not chains, so a second wire over the same pair adds nothing.

## Commands arrive as strings

`on-command` receives the *string rendering* of the JSON the client sent: `{"rate": 10}`
arrives as a string with braces and quotes, `10` as `"10"`, a bare string as itself. Parse
with that in mind, and answer anything unrecognized with `Err(UnknownCommand)` so the log
says `Unknown command: <name>` instead of nothing.

## The environment reads the wrong thing

`read-<port>` is bound by name to the variable in the port's `variable:` field. Two distinct
mistakes: the port's type and the variable's type disagree, which the loader rejects with a
clear message; or the WIT return type of `read-<name>` disagrees with the variable's type,
which fails at instantiation. The types available are `bool`, `int`, `float`, `vec2`, `vec3`,
`quaternion`, `tensor` — there is no text variable, so a name or a mode belongs in an option
or a command.

## A port vanished from the world

An import which no live code calls is not in the component. Two ordinary edits do it:
commenting out a reply while debugging, or an early `return` in front of it. In the
`wasm32-unknown-unknown` route there is a third, since a `std` call which aborts makes
everything after it unreachable.

Either way the build succeeds, and the component cannot speak, since it does not import
`<port>-out`. `verify.sh` reports it twice: a port which does not declare both directions,
and a bus which heard nothing back.

## `imu_test.wasm`, not `imu-test.wasm`

Cargo and wit-bindgen both turn dashes in a package or world name into underscores in the
artifact and the generated symbols. `build-component.sh` accounts for it; a hand-written `cp`
usually does not.

## A JavaScript component answers once and then says nothing

An exception which escapes an export is not a `result` error: the engine prints a traceback,
printing touches the filesystem, and `-d all` removed the filesystem, so the call dies without
a message and the instance is poisoned. Report failures by throwing the variant,
`throw { tag: 'unknown-command' }`, and catch everything else inside the handler. See
[js.md](js.md#errors).

## A Python component traps at init, or dies silently in a handler

Two causes, both in [python.md](python.md): the build left out `--stub-wasi`, so the
interpreter reached `wasi:cli/environment` before any code ran; or an exception escaped a
handler and the traceback printer touched the filesystem. The second is why every body of a
Python component is wrapped.
