---
title: QEMU
---

# QEMU Component

A NEST component running [QEMU](https://www.qemu.org/) virtual machines, and
exposing their emulated hardware interfaces (GPIO lines, an I2C bus and serial
ports) to the rest of the simulation.

## Overview

The QEMU component is the easiest way to run guest software inside a standard
machine and operating system. Each instance spawns its own `qemu-system-<arch>`
process and boots a guest image, so that the bootloader, kernel, drivers and
application software under test are exactly the ones deployed on the target.
This is what makes NEST a *software-in-the-loop* framework: nothing inside the
VM is instrumented, patched or recompiled.

All a VM can reach inside the simulated system is exposed as **system ports**.
GPIO and I2C go through QEMU's vhost-user device interface: NEST runs a
vhost-user backend for each, and transparently translates the guest's VirtIO
GPIO and I2C requests into port messages, and the replies back into the guest.
Serial ports are exposed to QEMU as socket character devices, which NEST reads
from and writes to as raw byte streams.

In addition, the component gives scenarios direct access to QEMU's own [QEMU
Machine Protocol](https://qemu-project.gitlab.io/qemu/interop/qemu-qmp-ref.html)
(QMP) through the `qmp_command` command, which is useful to observe or control
the machine itself (power state, CPU and device state, etc.) beyond the
simulated hardware.

## Example

A complete topology with one Alpine Linux VM connected to an LED driver via
GPIO, to a temperature sensor via I2C, and with one serial port available for
raw communication. The sensor reads the environment's temperature.

```yaml title="topology.yaml"
environment_variables:
  - name: temperature
    type: float

components:
  - name: AlpineVM
    type: qemu
    config:
      arch: x86_64 # (1)!
      mem_mb: 512 # (2)!
      options:
        - option: drive
          value: file=path/to/alpine.qcow2
        - option: nographic
        - option: enable-kvm

        # Virtual console on /dev/hvc0, connected to a PTY on the host
        - option: device
          value: virtio-serial
        - option: chardev
          value: pty,path=/tmp/qemu-pty,id=char0
        - option: device
          value: virtconsole,chardev=char0
      ngpio: 16 # (3)!
      gpio_names: # (4)!
        0: led
      nuart: 1 # (5)!
      i2c_timeout_ms: 10 # (6)!
    ports: # (7)!
      system:
        - name: gpio0
          type: bool
        - name: uart0
          type: bytes
        - name: i2c
          type: i2c

  - name: LED
    type: wasm
    config:
      module: path/to/led.wasm
    ports:
      system:
        - name: gpio
          type: bool

  - name: TEMP_SENSOR
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

wires:
  -
    - component: AlpineVM
      port: gpio0
    - component: LED
      port: gpio
  -
    - component: AlpineVM
      port: i2c
    - component: TEMP_SENSOR
      port: i2c
```

1. Required. The VM's architecture, which must match the `qemu-system-<arch>`
   binary used to build the guest image.
2. Required. RAM size of the VM, in MiB. NEST also uses this value for the
   shared memory backing the vhost-user devices.
3. Optional. Number of lines of the VirtIO GPIO chip instantiated in the VM.
4. Optional. Names for some of the lines. Named lines are easier to use from
   within the guest (e.g. `gpioset led=1`), and unnamed lines are left empty.
5. Optional. Number of serial ports. Port *N* is bound to `uart<N>` and shows
   up in the guest as its *N*-th 16550 UART (usually `/dev/ttyS<N>`).
6. Optional. The I2C reply timeout. A lower value makes bus scans much faster,
   at the risk of false timeouts on slow hosts.
7. All ports used by a component must be declared, otherwise they cannot be
   wired. Lines and ports which are not declared still exist inside the VM, but
   are not visible to the simulation.

## Reference

### Configuration

| Name             | Type    | Default      | Description                                                              |
|------------------|---------|--------------|--------------------------------------------------------------------------|
| `arch`           | string  | — (required) | Target architecture; NEST runs `qemu-system-<arch>`                      |
| `mem_mb`         | integer | — (required) | Size of the VM's RAM, in MiB                                             |
| `options`        | list    | `[]`         | Extra QEMU command-line arguments, passed through as-is                  |
| `ngpio`          | integer | `32`         | Number of lines of the VirtIO GPIO chip                                  |
| `gpio_names`     | map     | `{}`         | Optional names for GPIO lines, as `<line>: <name>` pairs                 |
| `gpio_socket`    | string  | auto         | Path of the vhost-user GPIO backend socket                               |
| `nuart`          | integer | `0`          | Number of serial (UART) ports                                            |
| `uart_sockets`   | map     | `{}`         | Path of the socket backing each UART, as `<port>: <path>` pairs          |
| `i2c_socket`     | string  | auto         | Path of the vhost-user I2C backend socket                                |
| `i2c_timeout_ms` | integer | `100`        | Time to wait for a device to reply to an I2C request, in milliseconds    |

Each entry of `options` is a QEMU argument, given as an option name and an
optional value, both without the leading dash:

```yaml
options:
  - option: drive
    value: file=path/to/alpine.qcow2 # -drive file=path/to/alpine.qcow2
  - option: nographic # -nographic
```

!!! note

    The configuration is validated strictly: `arch` and `mem_mb` are mandatory
    and any unknown field makes the topology fail to load.

??? info "QEMU arguments added automatically"

    On top of the `options` given in the topology, NEST appends everything
    required to connect the VM to the simulation:

    - `-m <mem_mb>`
    - a `/dev/shm`-backed shared memory object and its NUMA node, required by
      vhost-user devices: `-object memory-backend-file,id=mem,...` and
      `-numa node,memdev=mem`
    - for each UART, `-chardev socket,id=uart<N>,server=on,wait=off` and
      `-serial chardev:uart<N>`
    - `-chardev socket,id=vgpio` and `-device vhost-user-gpio-pci,chardev=vgpio,id=gpio`
    - `-chardev socket,id=vi2c` and `-device vhost-user-i2c-pci,chardev=vi2c,id=i2c`
    - `-qmp unix:<path>,server,nowait`, used for `qmp_command` and for stopping
      the VM gracefully

    The `mem`, `vgpio`, `vi2c` and `uart<N>` device and character device
    identifiers are reserved; avoid them (and the options above) in your own
    `options`.

    The auto-generated socket paths are randomly named (`vuart0-<uuid>.sock`,
    `vgpio-<uuid>.sock`, `vi2c-<uuid>.sock`, `qmp-<uuid>.sock`) inside the
    system temporary directory (`$TMPDIR`). Each instance gets its own set of
    sockets. Use `gpio_socket`, `i2c_socket` and `uart_sockets` to pin those
    paths when you need them to be predictable, for example when debugging the
    communication of a VM with a hand-written device model.

### System ports

| Name      | Type  | Description                                                            |
|-----------|-------|------------------------------------------------------------------------|
| `gpio<N>` | bool  | GPIO line *N*, with `0 <= N < ngpio`                                   |
| `uart<N>` | bytes | Serial port *N*, with `0 <= N < nuart`; bi-directional byte stream     |
| `i2c`     | i2c   | The VM's VirtIO I2C bus, driven as master                              |

Port names are positional: `gpio7` is line *number* 7, not the seventh declared
port. Indexing is validated when the topology is loaded, and a port index
outside of the configured range makes loading fail.

!!! note

    Ports are bi-directional, including GPIO lines: a line driven by the guest
    sends its new value to all other ports on the wire, and a line written by
    the simulation is readable from the guest. As with any NEST wire, nothing
    prevents both sides from driving the same line at the same time, in which
    case the last value written wins.

### Environment ports

This component has no environment ports.

### Commands

| Name          | Args                                                           | Description                                                                |
|---------------|----------------------------------------------------------------|----------------------------------------------------------------------------|
| `qmp_command` | `{"execute": "...", "arguments": {…}}` (`arguments` optional) | Execute a QMP command in the VM, and emit its reply as a `qmp_response` event |

The payload is forwarded verbatim to QEMU's QMP interface.

!!! note

    QMP commands are executed synchronously on the component's thread: while
    the reply is awaited, the VM's ports are not served. Failures (a malformed
    payload, an unknown QMP command, a QMP error reply) are logged as errors
    and **no** event is emitted; the simulation keeps running.

### Events

| Name           | Type                                | Description                                   |
|----------------|-------------------------------------|-----------------------------------------------|
| `qmp_response` | the `return` field of the QMP reply | Reply to a `qmp_command`, emitted on success  |

 QEMU's own asynchronous QMP events are discarded, and only the reply to the
 command that was sent is forwarded to the simulation.

## Accessing the guest

The QEMU component does not create a console or a network link by itself; you
configure them through `options`, as described in the [Running VMs
guide](../../guides/introduction/running-vms.md). The recommended setup is a
VirtIO serial console connected to a host PTY, which leaves the UART ports
free for simulation traffic:

```bash
picocom -b 115200 /tmp/qemu-pty
```

!!! tip

    Without an explicit `-serial` assignment, `-nographic` sends the guest
    console to QEMU's standard output, which NEST forwards to the component's
    logs. It is convenient to watch a guest boot without configuring anything,
    but it is noisy and unrelated to any port.

Inside a Linux guest, the emulated interfaces are visible as regular devices:

```bash
gpioinfo                               # the VirtIO GPIO chip, with the configured names
gpioset -t 0 led=1                     # write line 0
gpiomon led                            # monitor line 0
i2cdetect -l                           # find the i2c_virtio adapter number
i2cdetect -y <bus>                     # scan the bus for connected devices
i2ctransfer -y <bus> w1@0x4A 0x00 r2   # read from a device
cat /dev/ttyS0                         # receive bytes on uart0
```

Note that the VirtIO GPIO and I2C drivers may not be loaded by default
(`virtio-gpio`, `i2c-virtio`, `i2c-dev`), and that the user-space tools
(`libgpiod`, `i2c-tools`) must be installed in the guest. See [Enabling
virtualization features](../../guides/introduction/running-vms.md#enabling-virtualization-features)
for the setup used in the guides.

## Limitations

- **One disk per instance**: each VM needs its own disk image; two instances
  cannot share the same writable disk.
- **One GPIO chip and one I2C bus per VM**: they are always instantiated, even
  when the corresponding ports are not declared or wired. Multiple buses per VM
  is not currently supported.
- **Requires a recent QEMU build**: with the `vhost-user-gpio-pci` and
  `vhost-user-i2c-pci` devices (check with `qemu-system-<arch> -device help`).
- **vhost-user devices need shared memory**: NEST backs guest RAM with
  `/dev/shm`, which must be large enough for the total RAM of all running VMs
  (e.g. Docker's default `--shm-size` of 64 MB is too small).
