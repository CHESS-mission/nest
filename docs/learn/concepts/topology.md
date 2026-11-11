# System topology

In NEST, a simulated system is specified as a system topology (see [YAML
topology specification](../../reference/topology.md)). A topology is composed
of:

- **Components:** emulated devices such as [QEMU virtual
  machines](../../reference/components/qemu.md) or [custom GPIO, I2C or UART
  devices](../../reference/components/wasm.md). They are the basic building
  blocks of the system.
- **Wires:** virtual hardware connections between components.
- **Environment variables:** physical properties of the system's environment
  components can measure, such as temperature.

## Components

Components are the building blocks of the topology; they encapsulate
self-contained units of behavior within the simulated system. In practice, they
are meant to represent emulated devices, such as a processor or an I2C sensor,
communicating with each other in real-time via virtual hardware ports. When
instantiated inside the system, components may receive configuration options
defined in the topology (see
[Configuration](../../reference/topology.md#configuration) for how they are
declared and passed). NEST ships three component types: QEMU virtual machines,
WebAssembly devices, and the [Multiplexer](../../reference/components/mux.md),
which switches one port between two channels.

In NEST, each component runs on its own thread. What operations a component
performs is internal to that component; most of what NEST handles, in addition
to providing an execution runtime, is communication. A component has several
asynchronous channels over which it can send and receive data: **system
ports**, **environment ports**, **commands**, **events**, and **logs**.

### System ports

System ports are the mechanism through which components communicate with each
other within the simulated system. They are meant to model actual hardware
ports at the semantical level (i.e. they do not model electrical behavior or
physical wire properties). Like real hardware ports, system ports are
bi-directional, which means that a component can always both send and receive
data through a system port.

!!! note

    Notice the difference between the directionality of a hardware port (e.g. a
    pair of electrical wires) and the communication protocol (e.g. I2C) used
    through that port. In many cases, the communication protocol can be
    directional while the hardware port is not. It is the same in NEST: all
    system ports are bi-directional, but that doesn't prevent using directional
    protocols on top of them.

Finally, system ports are typed. This means that only ports of the same type can
be wired together. Currently, NEST supports the following system port types:

- **Integer:** 32-bit signed integer number
- **Float:** 32-bit floating point number
- **Boolean:** simple boolean value
- **String:** string of characters
- **Bytes:** stream of bytes
- **I2C:** request / reply objects for I2C communication

The **boolean**, **bytes**, and **I2C** types are most useful to represent
low-level hardware communication such as GPIO lines, UART or I2C buses, while
**integer**, **float** and **string** can be used to represent higher-level
interfaces between components in slightly more abstract systems. Which ports a
component accepts, and under which names, is specific to it; see [System
ports](../../reference/topology.md#system-ports) and each component's reference
page.

### Environment ports

Environment ports are the mechanism through which components read variables
from the environment. Inside a system topology, each environment port is bound
to a variable with matching type from the environment (See [Environment
variables](#environment-variables) for supported types, and
[Environment ports](../../reference/topology.md#environment-ports) for how they
are declared). Everytime a component reads through one of its environment port,
it receives the current value of the environment variable bound to that port.

### Commands & events

Commands and events are the only interface between a component and the exterior
of the simulation. Commands differ from system and environment port because
they get information in and out of the system, breaking the hermeticity of the
simulation. They are meant to enable a user (or script) to control the
simulation by dynamically changing the behavior of components. For example,
they can be used to realize specific test scenarios, such as a sensor
malfunction. On the other hand, events send machine-parsable information out of
the system for automatic validation; they provide observability but do not
interfere with the simulation itself. Commands and events are both
component-specific and consist of a name and free-form JSON payload. They are
exchanged with clients as the [`command`
action](../../reference/client-protocol.md#command) and the [`event`
update](../../reference/client-protocol.md#event).

### Logs

Components can also emit logs. Logs are purely intended for human feedback and
are not meant to be parsed programmatically for validation. They are useful to
monitor ongoing simulations and as a debugging tool. They reach clients as
[`log` updates](../../reference/client-protocol.md#log).

## Wires

Wires are simple connections between the system ports of components inside a
topology. A single wire can connect more than two ports with a many-to-many
relationship[^1], to better represent real hardware connections like GPIO lines
and I2C buses. Their syntax and the rules checked when a topology is loaded are
documented in [Wires](../../reference/topology.md#wires).

[^1]: When information is sent by any of the ports on the wire, all other ports
    on the wire receive it.

## Environment variables

As part of a simulated system, NEST also keeps track of a set of environment
variables inside a read-only[^2] key-value store called the environment.
Environment variables are user-defined, and are meant to store physical
properties of the system's environment that components can measure. Examples
include the current temperature, magnetic field, orientation of an arm, etc.
The point of environment variables is that they can be controlled from outside
the simulation, like [commands](#commands-events), to realize specific
scenarios. The supported environment variable types are:

- **Integer:** 32-bit signed integer number
- **Float:** 32-bit floating point number
- **Boolean:** simple boolean value
- **2D vector:** 2-dimensional floating point vector
- **3D vector:** 3-dimensional floating point vector
- **Quaternion:** floating point quaternion
- **Tensor:** N-dimensional floating point array

The default value each type starts at, and the exact encoding used when a
variable is written from outside, are given in [Environment
variables](../../reference/topology.md#environment-variables) and in
[`set_env`](../../reference/client-protocol.md#set_env).

[^2]: This only applies inside the simulation; they can obviously be written to
    from outside.

!!! tip

    If you need to represent temperature in more than one place in the system,
    you can simply use more than one environment variable for temperature.
