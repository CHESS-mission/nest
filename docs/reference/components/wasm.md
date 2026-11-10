---
title: WebAssembly
---

# WebAssembly Component

A NEST component that loads custom device models compiled to
[WebAssembly](https://webassembly.org/), so that emulated hardware can be
written in any language with a WebAssembly target and dynamically loaded into a
simulation without rebuilding NEST.

## Overview

Adding an arbitrary number of device types to NEST's source has several
drawbacks:

1. **Adding devices is more difficult**, because it requires some familiarity with
   NEST's codebase.
2. **Sharing custom devices is more difficult**, because it requires merging code
   upstream or maintaining forks.
3. **Each device addition requires rebuilding the framework.**

Furthermore, not every device running in NEST needs to care about the
framework's internals. To address these issues, NEST has bindings exposing a
simpler, synchronous event-driven interface for WebAssembly binaries which is
sufficient for most use cases. Those binaries are not component types of their
own: they are loaded by the WebAssembly component (type `wasm`), and executed
by [Wasmtime](https://wasmtime.dev/) in a sandboxed environment. The `wasm`
component type thus covers an arbitrary number of custom devices, such as
sensors, actuators, or microcontrollers. The same binary can even be
instantiated several times, each instance being fully independent and holding
its own state.

The contract between NEST and a device is the `nest:plugin/nest-component` [WIT
world](https://component-model.bytecodealliance.org/design/wit.html#worlds),
compliant with the [WebAssembly component
model](https://component-model.bytecodealliance.org/introduction.html). Because
devices have a varying number of ports, each device *extends* that world with
its own, instead of implementing `nest-component` directly:

```typescript title="wit/temp_sensor.wit"
package my-components:temp_sensor;

world temp_sensor {
  include nest:plugin/nest-component; // (1)!

  use nest:plugin/types.{i2c-transaction};

  // System port "i2c"
  export i2c-in: func(msg: i2c-transaction); // (2)!
  import i2c-out: func(msg: i2c-transaction);

  // Environment port "temperature"
  import read-temperature: func() -> f32; // (3)!
}
```

1. Every device world includes `nest:plugin/nest-component`, which defines the
   functions common to all devices.
2. A system port is one exported and one imported function, named
   `<port>-in` and `<port>-out`.
3. An environment port is a single imported function, named `read-<port>`.

NEST then satisfies that interface dynamically, at instantiation time, using
nothing but those naming conventions: it looks up the `<port>-in` export of
every system port declared in the topology, and binds each `<port>-out` import
and `read-<port>` import to the corresponding port. This keeps dispatch logic
out of device models, and gives them plain typed function calls.

Execution is single-threaded and event-driven: NEST calls into an instance when
a message arrives on one of its ports, when a command is sent to it, or when
one of its timers expires.

Writing a device from scratch, in Rust, is covered by the [Writing custom
devices](../../guides/introduction/custom-devices.md) guide. This page
documents what the WebAssembly component expects from the binaries it loads.

## Example

Two instances of the same sensor binary, with different I2C addresses, plus an
LED driver. Both sensors read the same environment variable.

```yaml title="topology.yaml"
environment_variables:
  - name: temperature
    type: float

components:
  - name: AlpineVM
    type: qemu
    config:
      arch: x86_64
      mem_mb: 512
      options:
        - option: drive
          value: file=path/to/alpine.qcow2
        - option: nographic
        - option: enable-kvm
      ngpio: 8
      gpio_names:
        0: led
    ports:
      system:
        - name: i2c
          type: i2c
        - name: gpio0
          type: bool

  - name: THERMO_A
    type: wasm
    config:
      module: path/to/temp_sensor.wasm # (1)!
      options:
        - ["i2c_address", "0x49"] # (2)!
    ports:
      system:
        - name: i2c # (3)!
          type: i2c
      environment:
        - name: temperature # (4)!
          variable: temperature
          type: float

  - name: THERMO_B
    type: wasm
    config:
      module: path/to/temp_sensor.wasm
      options:
        - ["i2c_address", "0x4A"]
    ports:
      system:
        - name: i2c
          type: i2c
      environment:
        - name: temperature
          variable: temperature
          type: float

  - name: LED
    type: wasm
    config:
      module: path/to/led.wasm
    ports:
      system:
        - name: gpio # (5)!
          type: bool

wires:
  -
    - component: AlpineVM
      port: i2c
    - component: THERMO_A
      port: i2c
    - component: THERMO_B
      port: i2c
  -
    - component: AlpineVM
      port: gpio0
    - component: LED
      port: gpio
```

1. Path to the WebAssembly component binary.
2. Device-specific options, forwarded verbatim to the device's `init` function.
3. System ports must be declared with the same name as in the device's world,
   and a type compatible with its function signatures.
4. Environment ports bind a port name to an environment variable of the same
   type; here both sensors read the same variable.
5. Port names are per-component: `THERMO_A`'s port is `i2c` and `LED`'s is
   `gpio`, and both simply match their worlds.

## Reference

### Configuration

| Name      | Type   | Default      | Description                                                  |
|-----------|--------|--------------|--------------------------------------------------------------|
| `module`  | string | — (required) | Path to the WebAssembly component binary                     |
| `options` | list   | `[]`         | Key-value pairs passed to the device's `init` function       |

The `module` path is resolved by the NEST server process, which means relative
paths are relative to the server's working directory, not to the topology file.
Remote URLs are not supported yet. The file must be a WebAssembly **component**
(a binary implementing a WIT world), not a bare core module.

The `options` list contains two-element lists, each holding the option name and
its value as **strings**:

```yaml
config:
  module: path/to/temp_sensor.wasm
  options:
    - ["i2c_address", "0x4A"]
    - ["startup_delay_ms", "250"]
```

!!! note

    Option names, formats and semantics are entirely defined by the device,
    which is responsible for parsing them and for rejecting unknown options or
    invalid values.

### System ports

Arbitrary in number, name and type: what a device exposes is defined by its
world. For each system port named `<name>` in the topology, NEST requires the
module to export `<name>-in` (called when a message arrives on the port) and to
import `<name>-out` (called by the device to send a message). Both take a
single argument of the same type, given below.

| NEST type | WIT type                                          |
|-----------|---------------------------------------------------|
| `int`     | `i32`                                             |
| `float`   | `f32`                                             |
| `bool`    | `bool`                                            |
| `bytes`   | `types.bytes` (`list<u8>`)                        |
| `text`    | `string`                                          |
| `i2c`     | `types.i2c-transaction`, from `nest:plugin/types` |

!!! note

    Port names and types are matched at start-up, not at topology load, and a
    mismatch prevents the component from starting. A missing export reports
    `failed to find function export`, and an incompatible signature reports
    `failed to convert function to given type`. Both are reported to clients as
    component errors.

### Environment ports

Arbitrary in number and name. Each environment port named `<name>` must
correspond to one imported function `read-<name>`, taking no argument and
returning the current value of the bound environment variable.

The type declared for the port in the topology must match the type of the
variable it binds to; it is the variable's type, not the port's, that decides
which signature NEST provides for `read-<name>`.

| NEST type    | WIT type            | Notes                                                  |
|--------------|---------------------|--------------------------------------------------------|
| `int`        | `i32`               |                                                        |
| `float`      | `f32`               |                                                        |
| `bool`       | `bool`              |                                                        |
| `vec2`       | `types.vec2`        | fields `x`, `y`                                        |
| `vec3`       | `types.vec3`        | fields `x`, `y`, `z`                                   |
| `quaternion` | `types.quaternion`  | fields `a`, `b`, `c`, `d` (a.k.a. `w`, `x`, `y`, `z`)  |
| `tensor`     | `types.tensor`      | `shape` and row-major `data` of `f32`                  |

Reads return a copy of the variable's current value, as seen at the exact
moment of the call. If a scenario drives the variable with linear
interpolation, a device simply observes the interpolated value, which is how
time-varying environments are modeled.

### Commands

| Name                | Args         | Description                                            |
|---------------------|--------------|--------------------------------------------------------|
| *device-defined*    | any JSON     | Forwarded as-is to the device's `on-command` function  |

The WebAssembly component accepts any command name and forwards it, together
with the **JSON text** of its arguments, to the device. A device is expected to
match on the name and return `command-error` for what it does not understand.

!!! note

    Arguments arrive as their JSON serialization, which is not always what it
    looks like on the sending side. Sending the string `"true"` calls
    `on-command` with `"\"true\""` (including the quotes), while sending the
    boolean `true` calls it with `true`. Both are valid, as long as the device
    and the scenario agree on which one they use.

Unknown commands and invalid values are logged as errors (`Unknown command:
...`, `Invalid value for command ...: ...`) without interrupting the
simulation.

### Events

| Name                | Type        | Description                                                  |
|---------------------|-------------|--------------------------------------------------------------|
| *device-defined*    | any JSON    | Emitted by the device through the `emit` function            |

Events are pure pass-through: whatever a device emits is published to clients
with the component's name attached. The payload must be valid JSON text;
otherwise the emission is turned into a component error.

## The `nest:plugin` interface

The following functions are provided by, or required from, a device by the
`nest:plugin/nest-component` world.

| Kind    | Signature                                                            | Description |
|---------|----------------------------------------------------------------------|-------------|
| export  | `init: func(options: list<tuple<string, string>>) -> result<_, init-error>` | Called once, when the simulation starts |
| export  | `on-command: func(command: string, args: string) -> result<_, command-error>` | Called for every command sent to the component |
| import  | `emit: func(event: string, value: string)`                             | Emits an event, with a JSON payload |
| import  | `get-time: func() -> f64`                                            | Seconds elapsed since the start of the simulation |
| import  | `log.info` / `log.warning` / `log.error: func(msg: string)`           | Component logs, visible in the web app and clients |

Devices may additionally include the optional `nest:plugin/timeout-ext` world,
which is what makes periodic and time-based behavior possible in an
event-driven model:

| Kind   | Signature                                    | Description |
|--------|----------------------------------------------|-------------|
| import | `set-timer: func(duration-ms: u32) -> timer-id` | Schedule a one-shot callback, in milliseconds |
| export | `on-timeout: func(timer-id: timer-id)`       | Called when a timer expires |

```typescript title="wit/temp_sensor.wit"
world temp_sensor {
  include nest:plugin/nest-component;
  include nest:plugin/timeout-ext; // (1)!
  // ...
}
```

1. Including the world is enough to get timers; the `on-timeout` export is
   detected by NEST at start-up.

!!! warning

    Timers cannot be cancelled. Expiring timers must be identified by their ID
    in `on-timeout`, so that stale timers are ignored, and repeating behavior
    must reschedule itself from within the callback.

## Host calls

A device can call the functions NEST provides, and nothing else: the interfaces
of the `nest:plugin` package, and the imports of its own ports.

??? info "Why components import more than they can call"

    Toolchains emit imports for interfaces the code may never reach, WASI in
    particular. A component built for the `wasm32-wasip2` target, whatever the
    language it is written in, imports most of `wasi:cli`, `wasi:io` and
    `wasi:clocks` because its standard library and the adapter reference them.
    Since such imports are impossible to get rid of and harmless when unused,
    NEST accepts components which carry them, and only the code paths which do
    call one of them fail.

In practice, the following common operations are therefore unavailable to a
device:

| Operation                                              | Interface it reaches     | Use instead                                  |
|--------------------------------------------------------|--------------------------|----------------------------------------------|
| Printing (`println!`, `printf`)                        | `wasi:cli/stdout`        | `log.info`, `log.warning`, `log.error`       |
| Reading environment variables or arguments             | `wasi:cli/environment`   | the options passed to `init`                 |
| Exiting the process                                    | `wasi:cli/exit`          | return an error from `init`                  |
| Reading the time, sleeping, waiting on I/O             | `wasi:clocks`, `wasi:io` | `get-time`, and `set-timer` to wake up later |
| Reading or writing files                               | `wasi:filesystem`        | there is no storage available                |
| Drawing random numbers                                 | `wasi:random`            | an option, or an environment port            |

The trapped call logs an error which names the import it reached:

```text
Failed to call export i2c-in: error while executing at wasm backtrace:
    0:     0x2f1 - lm75a!<wasm function 61>: wasi:clocks/monotonic-clock@0.2.12#now
       is not implemented by the NEST host: devices can only call the interfaces of
       the nest:plugin package, and imports of other interfaces, such as WASI,
       always trap
```
