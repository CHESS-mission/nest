---
title: YAML topology
icon: lucide/file-code
---

# YAML Topology

The system topology is a single YAML document listing the components of the
simulated system, the wires between them, and the environment variables they
can measure. This page documents every field of that document, its allowed
values, and how it is validated.

## Overview

A topology is made of three sections:

- **`components`**: the emulated hardware of the system, either built-in
  component types or custom devices loaded as WebAssembly plugins. Each
  component is configured here and declares the ports it exposes.
- **`wires`**: the physical connections between component ports. Unwired ports
  are simply disconnected.
- **`environment_variables`**: named and typed quantities describing the system's
  physical environment, which components read through environment ports.

The document is parsed and built into a running simulation in two steps:
*parsing* checks the shape of the document, then *loading* resolves names and
types, connects ports and instantiates every component. Both happen when the
topology is provided to the server, either as a file at start-up or uploaded by
a client (see [Client protocol](./client-protocol.md)).

## Structure

```yaml
environment_variables:        # (1)!
  - name: <string>
    type: <environment type>

components:                   # (2)!
  - name: <string>
    type: <component type>
    config:
      <component options>     # (3)!
    ports:
      system:                 # (4)!
        - name: <string>
          type: <system port type>
      environment:            # (5)!
        - name: <string>
          variable: <string>
          type: <environment type>

wires:                        # (6)!
  -
    - component: <string>
      port: <string>
    - component: <string>
      port: <string>
```

1. Optional. Describes every environment variable of the simulation.
2. Required. Every component of the system, each with a unique name.
3. Optional, and specific to each component type; see its reference page.
4. Optional. Hardware ports exposed by the component to the simulation.
5. Optional. Read-only bindings to environment variables.
6. Optional. Each wire is a list of at least two endpoints.

All keys are case-sensitive; all `type` values are not (`Float` and `" float"`
are both accepted). Strings used as names may contain any character except that
they must be non-empty, and they are compared as-is, so `CPU` and `cpu` are two
different components.

## Example

A system with two virtual machines sharing one I2C temperature sensor through a
multiplexer, one LED driven over GPIO, and a variable temperature.

```yaml title="topology.yaml"
environment_variables:
  - name: temperature
    type: float

components:
  - name: OBC
    type: qemu
    config:
      arch: x86_64
      mem_mb: 512
      options:
        - option: drive
          value: file=path/to/obc.qcow2
        - option: nographic
      ngpio: 16
      gpio_names:
        0: led
        1: mux-select
      i2c_timeout_ms: 10
    ports:
      system:
        - name: i2c
          type: i2c
        - name: gpio0
          type: bool
        - name: gpio1
          type: bool

  - name: PCD
    type: qemu
    config:
      arch: x86_64
      mem_mb: 512
      options:
        - option: drive
          value: file=path/to/pcd.qcow2
        - option: nographic
      i2c_timeout_ms: 10
    ports:
      system:
        - name: i2c
          type: i2c

  - name: BUS_MUX
    type: mux
    config:
      multiplexed_ports:
        - common: i2c
          channels:
            - i2c-obc
            - i2c-pcd
    ports:
      system:
        - name: select
          type: bool
        - name: i2c
          type: i2c
        - name: i2c-obc
          type: i2c
        - name: i2c-pcd
          type: i2c

  - name: TEMP_SENSOR
    type: wasm
    config:
      module: path/to/temp_sensor.wasm
      options:
        - ["i2c_address", "0x48"]
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
        - name: gpio
          type: bool

wires:
  # The two buses, switched by GPIO 1 of the OBC
  -
    - component: OBC
      port: i2c
    - component: BUS_MUX
      port: i2c-obc
  -
    - component: PCD
      port: i2c
    - component: BUS_MUX
      port: i2c-pcd
  -
    - component: BUS_MUX
      port: i2c
    - component: TEMP_SENSOR
      port: i2c
  -
    - component: OBC
      port: gpio1
    - component: BUS_MUX
      port: select
  # The LED on the OBC's GPIO 0
  -
    - component: OBC
      port: gpio0
    - component: LED
      port: gpio
```

## Top-level keys

| Key                       | Type | Required | Description                                       |
|---------------------------|------|----------|---------------------------------------------------|
| `environment_variables`   | list | no       | Environment variables the system exposes          |
| `components`              | list | yes      | Components of the system, each with its ports     |
| `wires`                   | list | no       | Connections between component system ports        |

## Environment variables

Each entry of `environment_variables` declares one variable:

| Name   | Type   | Required | Description                              |
|--------|--------|----------|------------------------------------------|
| `name` | string | yes      | Unique name of the variable              |
| `type` | string | yes      | One of the environment types, see below  |

| Type         | Value                                       | Default at start-up |
|--------------|---------------------------------------------|---------------------|
| `int`        | 32-bit signed integer                       | `0`                 |
| `float`      | 32-bit floating point number                | `0.0`               |
| `bool`       | boolean                                     | `false`             |
| `vec2`       | 2D vector, with fields `x` and `y`          | zero vector         |
| `vec3`       | 3D vector, with fields `x`, `y` and `z`     | zero vector         |
| `quaternion` | rotation, with fields `w`, `x`, `y`, `z`    | identity rotation   |
| `tensor`     | N-dimensional `float` array, as `shape` and row-major `data` | empty |

Environment variables are the only piece of state shared between components
outside of wires, and the only one writable from outside the simulation. They
are read-only to components, which observe them through [environment
ports](#environment-ports), and they are set by clients with the `set_env`
action (see [Client protocol](./client-protocol.md#actions)).

!!! note

    Variables are not initialized from the topology: they all start at the
    default value of their type, and stay there until a client sets them.
    Reading a variable before it is set is therefore valid, and gives that
    default.

## Components

Each entry of `components` declares one component:

| Name     | Type    | Required | Description                                          |
|----------|---------|----------|------------------------------------------------------|
| `name`   | string  | yes      | Unique name, used by wires, clients and logs         |
| `type`   | string  | yes      | One of `qemu`, `wasm` or `mux`                       |
| `config` | mapping | no       | Component options, specific to the type              |
| `ports`  | mapping | no       | The component's system and environment ports         |

| Type    | Description                                            |
|---------|--------------------------------------------------------|
| `qemu`  | A QEMU virtual machine (see [QEMU](./components/qemu.md)) |
| `wasm`  | A custom device loaded as a WebAssembly component (see [WebAssembly](./components/wasm.md)) |
| `mux`   | A two-channel multiplexer (see [Multiplexer](./components/mux.md)) |

A component runs on its own thread, and its name identifies it to clients when
sending commands and in the events and logs it produces. Names must be unique
within a topology.

### Configuration

`config` is converted to JSON and passed to the component as-is, which is what
each component's reference page documents field by field. The rules that apply
to every component are:

| YAML                        | Becomes                             | Notes                                            |
|-----------------------------|-------------------------------------|--------------------------------------------------|
| mappings                    | JSON objects                        | Keys must be scalars (strings, numbers, booleans) |
| lists                       | JSON arrays                         | Any nesting is allowed                           |
| `true` / `false`            | booleans                            |                                                  |
| integers, floats            | numbers                             |                                                  |
| quoted or plain strings     | strings                             | `0x4A` and `'0x4A'` are both strings             |
| empty value, `null`, `~`    | JSON `null`                         | Rejected unless the field is optional            |

### System ports

Each entry of `ports.system` declares one hardware interface of the component:

| Name   | Type   | Required | Description                              |
|--------|--------|----------|------------------------------------------|
| `name` | string | yes      | Unique within the component              |
| `type` | string | yes      | One of the system types, see below       |

| Type    | Values carried                                    |
|---------|---------------------------------------------------|
| `int`   | 32-bit signed integers                            |
| `float` | 32-bit floating point numbers                     |
| `bool`  | booleans, e.g. GPIO lines and status signals      |
| `bytes` | byte streams, e.g. serial lines and raw payloads  |
| `text`  | strings                                           |
| `i2c`   | I2C requests and replies                          |

System ports are bi-directional, and only ports of the same type can be wired
together. What a component accepts as port *names* is specific to it: the QEMU
component only accepts names of the form `gpioN`, `uartN` and `i2c`, the mux
component only accepts the names used in its configuration plus `select`, while
a WebAssembly device accepts any name, since its ports are defined by its own
interface.

### Environment ports

Each entry of `ports.environment` binds one environment variable to one port of
the component:

| Name       | Type   | Required | Description                                  |
|------------|--------|----------|----------------------------------------------|
| `name`     | string | yes      | Unique within the component                  |
| `variable` | string | yes      | Name of an existing environment variable     |
| `type`     | string | yes      | Must match the type of the variable          |

Several ports, of the same or of different components, may be bound to the same
variable. The `variable` field must refer to a variable declared in
`environment_variables`, and the declared `type` must be identical to the
variable's; anything else makes loading fail.

## Wires

`wires` is a list of nets. Each net is a list of at least two endpoints, and
each endpoint refers to a system port of a component:

| Name        | Type   | Required | Description                            |
|-------------|--------|----------|----------------------------------------|
| `component` | string | yes      | Name of a component of the topology    |
| `port`      | string | yes      | One of its declared system ports       |

A wire is not a point-to-point link but a net: every pair of endpoints of a
wire is connected in both directions, so a wire with three endpoints behaves
like a three-drop bus. This is how shared lines (I2C buses, GPIO lines) are
modeled; the component on each drop sees the same message.

!!! note

    Wiring the same pair of ports twice is rejected, whether by repeating a
    wire or by listing the same endpoint twice, since both would deliver every
    message twice. Two ports of the same component cannot be wired either: a
    wire connects components to each other, never a component to itself.

Unwired ports are allowed and simply do not carry anything: a message sent on a
port with no connection is dropped, and a port no component writes to never
receives anything. Within one wire, messages from a given port are delivered in
order; no guarantee is given across separate wires, or against messages coming
from different ports.

## Validation

Loading a topology runs the checks below: the parser first, then the simulation
builder. Any failure aborts loading, leaves the previously loaded topology in
place, and is reported in the server's own logs. When the topology is given
with `--topology`, an error aborts the server instead.

| Checked                                                  | Error                                                       |
|----------------------------------------------------------|--------------------------------------------------------------|
| syntax of the YAML document                              | `Invalid format`                                             |
| several YAML documents in the file                       | `a topology must contain a single YAML document`             |
| presence and shape of every field documented above       | `Missing field: "<path>"`, `Validation failed: <detail>`     |
| unknown type names                                       | `Invalid type: "Unknown component type"`, `... port type`, `... environment variable type` |
| unique component and environment variable names          | `Duplicate component name: <name>`, `Duplicate environment variable name: <name>` |
| unique port names within a component                     | `Duplicate port name <port> in component <name>`             |
| every wire endpoint names an existing component          | `Component <name> referenced in wire does not exist in topology.` |
| every wire endpoint names a declared port                | `Use of undeclared port <port> of component <name> in wire.`  |
| both ends of a wire have the same port type              | `Cannot wire <a>.<p> (type <t>) to <b>.<q> (type <u>): wired ports must have the same type` |
| wire endpoints belong to distinct components             | `All endpoints of a wire must belong to different components...` |
| each port pair is wired at most once                     | `Duplicate wire between <a> and <b>`                          |
| environment ports refer to a declared variable           | `No matching environment variable name for port <name>.<port>: <variable>` |
| environment port type matches its variable               | `Type mismatch for environment port <name>.<port>: ...`      |
| component-specific configuration                         | e.g. `missing field \`arch\``, `Unknown system port <name> for QemuComponent` |

Type mismatches between a component's declared ports and a device's own
interface cannot be detected at load time, and surface when the simulation
starts instead (see each component's reference page).
