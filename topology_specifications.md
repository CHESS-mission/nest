# Topology

## Todos:

- [ ] file location for defaults? maybe a NEST_COMPONENTS_PATH env?
- [ ] file naming scheme or + its upload procedure?
- [ ] should we really be using paths?
 
## Overview
A Topology file defines the topology for the simulation on the serverside of **nest**, and define all connections and components that the server must handle during the simulation.

It specifies:
- **Environement variables** used by the system
- **Components**, with their types, configurations and ports
- **Wiring** between the components

Default Topology config files should be placed at `./i/havent/decided/yet/please/help/` and is usually named after the situation it should set up such as `temperature_test.yaml`. This file is read at the start of any simulation.

## Structure Summary

```yaml
environment_variables:
  - 
    name: <string>
    type: <port_type>

components:
  - 
    name: <string>
    type: <component_type>      # QEMU or WASM
    config:
      <component_config> : <component_value>   
    ports:
      <port_group>:
        - name: <string>
          type: <port_type>

wires:                          # all wires
  -                             # a set of endpoints
    - component: <string>       # a single endpoint
      port: <port_type>
    - component: <string>       # name of one of the components described in this file
      port: <port_type>
```
> [!NOTE]
> YAML style lists are used since they are clearer to read than JSON style, especially in the case of the wires which contain nestled lists.

- Port types must be one of 
    - `int`
    - `float`
    - `bool`
    - `bytes`
    - `text`
    - `json`

- Port groups must be one of: 
    - `system_port`  
    - `environment_port`.

## Field Reference

### Top level keys

| Key                     | Type | Required | Description                                    |
| ----------------------- | ---- | -------- | ---------------------------------------------- |
| `environment_variables` | list | no        | Global variables accessible by components.     |
| `components`            | list | yes        | Defines each logical or physical component.    |
| `wires`                 | list | yes        | Specifies connections between component ports. |


### Environment Variables

| Field  | Type   | Required | Description                              |
| ------ | ------ | -------- | ---------------------------------------- |
| `name` | string | yes        | Name of the environment variable.        |
| `type` | port_type | yes        | Types that can be handled by Ports. |

#### Example

```yaml
environment_variables:
  - name: temperature
    type: float
  - name: altitude
    type: float
```

### Components

Defines a singular component of the simulation.

| Field    | Type   | Required | Description                                  |
| -------- | ------ | -------- | -------------------------------------------- |
| `name`   | string | yes        | Unique component identifier.                 |
| `type`   | string | yes        | Component type (e.g., `qemu`, `wasm`).       |
| `config` | map    | yes        | Component-specific configuration parameters. WASM components have the `module` config reserved for their location. |
| `ports`  | map    | yes        | Defines exposed communication interfaces.    |



#### Example
A QEMU component.
```yaml
components:
  - 
    name: QEMU
    type: qemu
    config:
      arch: arm
      mem_mb: 1024
      options: 
        - option: cpu
          value: cortex-a7
        - option: M
          value: virt
        - option: kernel
          value: ./image/zImage 
        - option: append
          value: rootwait root=/dev/vda console=ttyAMA0 
        - option: drive
          value: if=none,file=./image/disk.qcow2,id=hd0 
        - option: device
          value: virtio-blk-device,drive=hd0 
        - option: serial
          value: pty:/tmp/qemu-serial-pty 
        - option: nographic 
      i2c_timeout_ms: 10
      ngpio: 8
      gpio_names: 
        5: echo_out
        7: echo_in 
    ports:
      system:
        - name: i2c
          type: json
```

### Wires

Defines interconnections between component ports.

Each wire entry connects two components via matching port names and compatible types.

| Field       | Type   | Required | Description                     |
| ----------- | ------ |----------| ------------------------------- |
| `component` | string |    yes   | Name of the component endpoint. |
| `port`      | string |    yes   | Port name on that component.    |

> [!NOTE]
> The actual fields of `wires` are endpoints, which are lists of endpoints which are connected to each other.

#### Example

```yaml
wires:
  - 
    - component: QEMU
      port: i2c
    - component: TEMP
      port: i2c
  - 
    - component: QEMU
      port: gpio
    - component: UHF
      port: gpio
```



## Validation Rules

- Each component.name must be unique.

- Ports connected in wires must exist in both components.

- Port types must be one of 
    - `int`
    - `float`
    - `bool`
    - `bytes`
    - `text`
    - `json`

- Port groups are currently only composed of `system_port` and `environment_port`, both of which are composed of a unique name and a port type.


## Version History

| Version | Date       | Changes                                                     |
| ------- | ---------- | ----------------------------------------------------------- |
| v1.0    | 2025-10-10 | Initial structure for multi-component topology configuration. |
