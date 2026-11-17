# Scenarios

A [system topology](./topology.md) is all NEST requires to simulate a system,
but it is not sufficient to perform automated testing of the system. For that,
NEST introduces the concept of a scenario as the reproducible unit of execution
of a simulated system in a controlled environment over a time interval. A
scenario is specified by the combination of a system topology and a control
sequence. The topology instantiates the system, while the control sequence
specifies the value of environment variables, the behavior of components, and
the events that are expected at all times, as well as when the scenario is
complete.

In practice, a scenario is executed by a NEST client, such as [NEST's Python
client](https://github.com/CHESS-mission/nest-python-client), using environment
variable updates, sending commands and receiving events at specific points in
time: the `set_env` and `command` actions, and the `event` updates of the
[client protocol](../../reference/client-protocol.md#actions). Which commands
and events a component understands is component-specific, and documented on the
reference page of that component. See the [testing with
scenario](../../guides/introduction/scenarios.md) guide for a concrete example.
