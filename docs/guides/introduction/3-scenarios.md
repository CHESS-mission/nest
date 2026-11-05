---
icon: lucide/calendar-chevrons-right
---

# Testing with scenarios

As demonstrated in the previous guides, NEST enables users to define and
orchestrate complete system-level simulations declaratively. But there is more,
because a full software-in-the-loop testing framework (which NEST aims to be)
incurs additional requirements:

1. **Interactivity**: to reliably create representative scenarios, edge-cases,
   and known failure modes, components need some amount of real-time
   interactivity beyond the simple configuration options received at
   initialization.
2. **Observability**: to understand what is happening inside the simulation and
   determine whether a test is failing, some amount of state needs to be
   observable from outside the simulation, beyond simple human-readable logs.
3. **Automatic validation**: to deterministically verify that tests are passing
   without human intervention, the framework needs a way to automatically
   perform simulations and validate their outcomes.

NEST addresses interactivity with **environment variables** and **commands**,
observability with **events** and automatic validation with **scenarios**. In
this guide, we will first make our LED and LM75A I2C sensor from the previous
guide interactive and observable, and then we will create a complete test
scenario to make sure our system works as expected.

## Introduction

We will simulate a system in which a script in our Alpine Linux VM (from
[Running VMs](./1-running-vms.md)) continuously reads the temperature from an
instance of our LM75A I2C sensor, and turns on an instance of our LED component
(both from [Writing Custom Devices](./2-custom-devices.md)) when the
temperature is above 80 °C. We will also add a feature of the LM75A sensor to
simulate a malfunction. Finally, we will write a scenario to automatically
verify that the LED is turned on only when the temperature is above the
threshold, and that when the LM75A sensor fails, the LED doesn't turn on
despite the temperature being above the threshold.

## The topology

We already have all we need to write the topology, so let's start with this.
Our simulated system consists of one instance of the Alpine Linux VM connected
to one LED via GPIO 0, and one LM75A temperature sensor via I2C at address
0x4A. It also has one environment variable of type float to track temperature.

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
          value: file=alpine.qcow2
        - option: nographic 
        - option: enable-kvm

        # Use virtual console to leave UART ports available
        - option: device
          value: virtio-serial
        - option: chardev
          value: pty,path=/tmp/qemu-serial-pty,id=char0
        - option: device
          value: virtconsole,chardev=char0
      ngpio: 16
      gpio_names:
        0: led
      i2c_timeout_ms: 10
    ports:
      system:
        - name: gpio0
          type: bool
        - name: i2c
          type: i2c

  - name: LED
    type: wasm
    config:
      module: ./led.wasm
    ports:
      system:
        - name: gpio
          type: bool

  - name: LM75A
    type: wasm
    config:
      module: ./lm75a.wasm
      options:
        - [ i2c_address, '0x4A' ]
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
    - component: LM75A
      port: i2c
```

## Adding a command to the LM75A sensor

Now, we need a way to tell our temperature sensor to start simulating a
failure. This is what *commands* are for in NEST. Commands represent inputs
that are external to the simulated system, and are the main interactivity
mechanism together with environment variables (which we covered in the previous
guide). A command has a name and a free-form JSON payload, which means that
components are responsible for defining and documenting what commands (and
payload) they accept.

??? question "What is the difference between commands and environment variables?"

    While they might seem redundant at first, commands and environment variables
    serve a different purpose. Commands are one-off, free-form signals delivered
    to exactly one component, without the component requesting it. Environment
    variables, on the other hand, are typed, updated and read asynchronously by
    the scenario and components (respectively), and can be bound to any number
    of components. Commands are suitable for one-off instructions, such as
    starting to simulate a failure or updating a configuration, while
    environment variables are suitable when the update frequency of the value
    being communicated benefits from being decoupled from the its reading
    frequency and when there might be several concurrent readers.

    To further illustrate, if one were to implement temperature updates with
    commands (which is possible to do), they would need to continuously send
    temperature update commands during the simulation to each sensor
    individually, and at their correct polling frequency. Worse, different
    temperature sensors implemented by different people might use different
    command names and payload formats. Sounds tedious, right?

In the LM75A sensor implementation, add a new `simulate_failure` field
defaulting to false:

```rust title="lm75a/src/lib.rs"
struct Lm75a {
    ...

    // When true, the sensor always reads a temperature of 0 °C
    simulate_failure: bool,
}

impl Lm75a {
    fn new() -> Self {
        Self {
            i2c_address: DEFAULT_I2C_ADDRESS,
            temperature_half_degrees: 0,
            conversion_timer: None,
            simulate_failure: false,
        }
    }

    ...
}
```

Then, update `perform_conversion` to read 0 °C when `simulate_failure` is true:

```rust title="lm75a/src/lib.rs"
impl Lm75a {
    ...

    fn perform_conversion(&mut self) {
        let environment_temperature_c: f32 = match self.simulate_failure {
            true => 0.0,
            false => read_temperature(),
        };

        self.temperature_half_degrees =
            Self::celsius_to_half_degrees(environment_temperature_c);
    }
    
    ...
}
```

Finally, parse the `simulate_failure` command in `on_command` to start and stop
simulating the failure:

```rust title="lm75a/src/lib.rs"
impl Guest for Lm75a {
    ...

    fn on_command(
        command: String,
        args: String,
    ) -> Result<(), CommandError> {
        let self_ = get_self();

        if command == "simulate_failure" {
            match args.as_str() {
                "\"true\"" => {
                    self_.simulate_failure = true;
                    log::info("Simulating sensor failure");
                    Ok(())
                }
                "\"false\"" => {
                    self_.simulate_failure = false;
                    log::info("Stopping to simulate sensor failure");
                    Ok(())
                }
                _ => Err(CommandError::InvalidValue(
                    "The argument of simulate_failure must be \"true\" or \"false\"."
                    .to_string()
                )),
            }
        } else {
            Err(CommandError::UnknownCommand)
        }
    }

    ...
}
```

## Sending events from the LED component

Next, we need a way to know when the LED is turned on or off. So far, we have
been using logs to manually verify it. However, logs are meant to be a
human-readable information and debugging channel, not a machine-parsable
observability mechanism. This is what *events* are for in NEST. Similarly to
commands, events have a name and a free-form JSON payload. They are reciprocal:
commands are sent to components while events are emitted by components for
observability and validation purposes.

Let's add a `changed_state` event to out LED component, with payload "on" or
"off", using the `emit(name, payload)` function from the `nest-component`
bindings. That's a single-line change! However, we will also make the LED state
persistent in order to only fire the event when the state changed:

```rust title="led/src/lib.rs"
#[allow(warnings)]
mod led;

use led::Guest;

use crate::led::{CommandError, InitError, emit, nest::plugin::log};

#[derive(Default)]
struct Led {
    state: bool,
}

static mut LED: Option<Led> = None;

fn get_self() -> &'static mut Led {
    unsafe {
        let sensor_ptr = core::ptr::addr_of_mut!(LED);
        if (*sensor_ptr).is_none() {
            *sensor_ptr = Some(Led::default());
        }
        (*sensor_ptr).as_mut().unwrap()
    }
}

impl Guest for Led {
    fn init(_config: Vec<(String, String)>) -> Result<(), InitError> {
        Ok(())
    }

    fn on_command(_command: String, _args: String) -> Result<(), CommandError> {
        Err(CommandError::UnknownCommand)
    }

    fn gpio_in(val: bool) -> () {
        let self_ = get_self();

        // Ignore updates that don't change the state
        if val == self_.state {
            return;
        }

        self_.state = val;
        let state = match val {
            true => "on",
            false => "off",
        };
        log::info(&format!("LED turned {}!", state));
        emit("changed_state", &format!("\"{}\"", state)); // (1)!
    }
}

led::export!(Led with_types_in led);
```

1. This emits the `changed_state` event, with payload `on` / `off`.

## Configuring the VM

We must to configure the VM to periodically read the temperature from the
sensor and turn on the LED when it exceeds 80 °C. Let's start by writing a
Python script in the VM named `temp_monitor.py` to perform this automatically
using `smbus2` and `gpiod`:

```python title="/root/temp_monitor.py"
import time
import smbus2
import gpiod
from gpiod.line import Direction, Value

LM75A_ADDRESS = 0x4A
TEMP_THRESHOLD = 80.0
GPIO_CHIP = "/dev/gpiochip0"
GPIO_LINE_NAME = "led"


def read_temp():
    with smbus2.SMBus(1) as bus:
        data = bus.read_i2c_block_data(LM75A_ADDRESS, 0, 2)

    raw_temp = (data[0] << 8 | data[1]) >> 5

    if raw_temp & (1 << 10):
        raw_temp -= (1 << 11)

    return raw_temp * 0.125


def set_led(state, request):
    request.set_value(GPIO_LINE_NAME, Value.ACTIVE if state else Value.INACTIVE)

with gpiod.request_lines(
    GPIO_CHIP,
    consumer="temp-monitor",
    config={
        GPIO_LINE_NAME: gpiod.LineSettings(direction=Direction.OUTPUT)
    },
) as request:
    while True:
        temp_c = read_temp()
        led_on = temp_c > TEMP_THRESHOLD

        set_led(led_on, request)

        print(
            f"Temperature: {temp_c:.3f} °C, "
            f"LED={'HIGH' if led_on else 'LOW'}"
        )

        time.sleep(0.25)
```

Then, to start it automatically at boot, create and OpenRC service at
`/etc/init.d/temp-monitor`:

```bash title="/etc/init.d/temp-monitor"
#!/sbin/openrc-run

name="LM75A Temperature Monitor"
command="uv"
command_args="run -w smbus2 -w gpiod /root/temp_monitor.py"
command_background="yes"
pidfile="/run/${RC_SVCNAME}.pid"

depend() {
    need localmount
}
```

!!! warning

    This service assumes the script exists at `/root/temp_monitor.py`. Tweak it
    to your needs.

Finally, make the service executable and add it to the default runlevel to make
it automatically start at boot:

```bash
chmod +x /etc/init.d/temp-monitor
rc-update add temp-monitor default
```

Now, if you restart the simulation (or just reboot the VM), you should see
I2C activity from the LM75A sensor in NEST's logs without having to connect to
the VM and manually start the script!

## Writing a test scenario

In NEST, a scenario is a reproducible unit of execution of a simulated system
in a controlled environment over a time interval. It is specified by the
combination of a topology and a control sequence: the topology defines the
system under test, made from a set of components, wires and environment
variables, while the control sequence consists of a scripted timeline of
commands, environment variable updates and expected or forbidden events for
validation. Typically, many control sequences will be written for one topology,
and they are not portable across topologies (because they refer to components
and environment variables by name).

The [NEST Python client](https://github.com/CHESS-mission/nest-python-client)
is both a Python library and a command line tool to write and execute
scenarios. Refer to the project's README for installation instructions.

Let's start by writing a scenario using the `nest_client` library, which
exposes a DSL-like syntax on top of Python to write sequences and scenarios:

```python title="scenario.py"
from nest_client import *
from nest_client.time import Time


@sequence # (1)!
def check_qemu(component_name: str):
    with timeline() as t: # (2)!
        t[0 * s] = command(component_name, "qmp_command", { "execute": "query-status" }) # (3)!
        t[0 * s] = event(component_name, "qmp_response") \ # (4)!
            .where(lambda payload: payload['running'] == True and payload['status'] == 'running') \
            .expect(within=500 * ms)


@sequence
def init():
    with timeline() as t:
        # Wait for the VM to boot
        t[10 * s] = check_qemu("AlpineVM") # (5)!


@sequence
def temperature_ramp(from_: float, to: float, over: Time):
    with timeline() as t:
        t[0 * s] = set_env("temperature", EnvVarType.FLOAT, from_) # (6)!
        t[0 * s] = set_env("temperature", EnvVarType.FLOAT, to, lerp_over=over)


@sequence
def test_led_turns_on_above_80(): # (7)!
    with timeline() as t:
        BELOW_80_RAMP_TIME = 10 * s
        ABOVE_80_RAMP_TIME = 3 * s
        t[0 * s] = temperature_ramp(20, 79, BELOW_80_RAMP_TIME)
        t[0 * s] = event("LED", "changed_state").forbid(within=BELOW_80_RAMP_TIME)
        t[BELOW_80_RAMP_TIME] = temperature_ramp(79, 82, ABOVE_80_RAMP_TIME)
        t[BELOW_80_RAMP_TIME] = event("LED", "changed_state") \
            .where(lambda payload: payload == "on") \
            .expect(within=ABOVE_80_RAMP_TIME)


@sequence
def test_led_does_not_turn_on_when_sensor_fails(): # (8)!
    with timeline() as t:
        RAMP_TIME = 3 * s
        t[0 * s] = command("LM75A", "simulate_failure", "true")
        t[0 * s] = temperature_ramp(79, 82, RAMP_TIME)
        t[0 * s] = event("LED", "changed_state") \
            .where(lambda payload: payload == "on") \
            .forbid(within=RAMP_TIME)


@scenario(topology_file="./topology.yaml") # (9)!
def temperature_monitor():
    with timeline() as t:
        t[0 * s] = init() >> \ # (10)!
                   test_led_turns_on_above_80() >> \
                   test_led_does_not_turn_on_when_sensor_fails()
```

1. The `@sequence` decorator is used to make the decorated function a define a
   sequence.
2. Inside a sequence, the timeline context manager (accessed with
   `timeline()`), enables constructing the sequence's timeline by attaching
   schedulable items to the timeline object with syntax `t[TIME] = schedulable`.
   All timings are relative to the start of the sequence (`0 * s`).
3. This schedules a command at time 0 (relative to this sequence). The
   command's name is `qmp_command`, with payload `{ "execute": "query-status"
   }`, and it is addressed to the component name passed to the sequence. Notice
   that sequences are still Python functions, so they can take arguments just as
   usual! It is a command of the QEMU component, which forwards the payload to
   the running QEMU VM via the QEMU Machine Protocol (QMP).
4. This an event validation range. Here, we expect to receive a `qmp_response`
   event from the component passed as argument within 500ms from the start of
   the sequence. Events can either be expected (`.expect()`) or forbidden
   (`.forbid()`), depending on whether they should be emitted or not. Additional
   validation can also be performed on the event's payload with `.where()`.
5. Sequences are composable, which means that a sequence can schedule one or
   more subsequences! Because timings are relative, time 0 for `check_qemu`
   will correspond to 10s for `init` here. This makes Sequences powerful
   primitives for building modular scenarios.
6. The last schedulable item type is environment variable updates with
   `set_env()`. Here, we first set the `temperature` environment variable, of
   type float, to a given temperature, and then we ramp it up (or down) to
   another temperature over a certain period of time. This is achieved using
   the `lerp_over` parameter, which automatically performs linear interpolation
   between the current value and target value on the server, over the specified
   period.
7. Here, we combine several sequences to create a more interesting test sequence
   to verify that the LED turns on when passing the 80 °C threshold (and not
   before).
8. And here, we create a sequence to verify that the LED doesn't turn on when
   passing the threshold while the temperature sensor is failing (reading 0 °C).
9. Finally, we combine everything into a scenario with the `@scenario`
   decorator. Scenarios are regular sequences, except they are bound to a
   topology file (by path). Therefore, before executing the scenario, the NEST
   client can automatically load the corresponding topology onto the server.
10. A nice way to chain sequences without knkowing their exact length in
    advance is with the `>>` operator, which returns a new sequence consisting
    of the first one immediately followed by the second.

We can now use `nest-client` to output the full resolved timeline of the
scenario and make sure it matches our intent:

```bash
nest-client show scenario.py:temperature_monitor
```

??? success "Output"

    ```text
    Scenario: temperature_monitor (duration: 26500ms)
    [     10s] (temperature_monitor / init / check_qemu) Command(AlpineVM.qmp_command, args={"execute": "query-status"})
    [     10s] (temperature_monitor / init / check_qemu) Event(expect AlpineVM.qmp_response within 500ms)
    [ 10500ms] (temperature_monitor / test_led_turns_on_above_80 / temperature_ramp) SetEnv(temperature, type=EnvVarType.FLOAT, value=20)
    [ 10500ms] (temperature_monitor / test_led_turns_on_above_80 / temperature_ramp) SetEnv(temperature, type=EnvVarType.FLOAT, value=79, lerp_over=10s)
    [ 10500ms] (temperature_monitor / test_led_turns_on_above_80) Event(forbid LED.changed_state within 10s)
    [ 20500ms] (temperature_monitor / test_led_turns_on_above_80 / temperature_ramp) SetEnv(temperature, type=EnvVarType.FLOAT, value=79)
    [ 20500ms] (temperature_monitor / test_led_turns_on_above_80 / temperature_ramp) SetEnv(temperature, type=EnvVarType.FLOAT, value=82, lerp_over=3s)
    [ 20500ms] (temperature_monitor / test_led_turns_on_above_80) Event(expect LED.changed_state within 3s)
    [ 23500ms] (temperature_monitor / test_led_does_not_turn_on_when_sensor_fails) Command(LM75A.simulate_failure, args="true")
    [ 23500ms] (temperature_monitor / test_led_does_not_turn_on_when_sensor_fails / temperature_ramp) SetEnv(temperature, type=EnvVarType.FLOAT, value=79)
    [ 23500ms] (temperature_monitor / test_led_does_not_turn_on_when_sensor_fails / temperature_ramp) SetEnv(temperature, type=EnvVarType.FLOAT, value=82, lerp_over=3s)
    [ 23500ms] (temperature_monitor / test_led_does_not_turn_on_when_sensor_fails) Event(forbid LED.changed_state within 3s)
    ```

!!! tip

    You can recursively discover all existing scenarios inside a directory with
    `nest-client list <dir>`, which can be helpful if you don't remember their
    `path:name` identifiers!

We can now finally get to executing the scenario with `nest-client run
<scenario>`! Make sure `nest-server` is running and no other simulation is
currently taking place. You do not need the web app at all for running scenario
from the Python client, although it might be helpful to monitor logs in real
time. For example, you could try logging the temperature from the LM75A sensor
so see how it evolves in real time!

```bash
nest-client run scenario.py:temperature_monitor
```

Once the scenario is finished, you should see the following output:

```text
Running scenario temperature_monitor
[14:18:58] INFO: Connecting to NEST at ws://localhost:3000/ws
[14:18:58] INFO: Connected.
[14:19:08] INFO: [     10s] (temperature_monitor / init / check_qemu) Command(AlpineVM.qmp_command, args={"execute": "query-status"})
[14:19:08] INFO: Received event: {'type': 'event', 'timestamp': 10.042532247, 'component': 'AlpineVM', 'event': 'qmp_response', 'data': {'running': True, 'status': 'running'}}
[14:19:09] INFO: [ 10500ms] (temperature_monitor / test_led_turns_on_above_80 / temperature_ramp) SetEnv(temperature, type=EnvVarType.FLOAT, value=20)
[14:19:09] INFO: [ 10500ms] (temperature_monitor / test_led_turns_on_above_80 / temperature_ramp) SetEnv(temperature, type=EnvVarType.FLOAT, value=79, lerp_over=10s)
[14:19:19] INFO: [ 20500ms] (temperature_monitor / test_led_turns_on_above_80 / temperature_ramp) SetEnv(temperature, type=EnvVarType.FLOAT, value=79)
[14:19:19] INFO: [ 20500ms] (temperature_monitor / test_led_turns_on_above_80 / temperature_ramp) SetEnv(temperature, type=EnvVarType.FLOAT, value=82, lerp_over=3s)
[14:19:20] INFO: Received event: {'type': 'event', 'timestamp': 22.0420937, 'component': 'LED', 'event': 'changed_state', 'data': 'on'}
[14:19:22] INFO: [ 23500ms] (temperature_monitor / test_led_does_not_turn_on_when_sensor_fails) Command(LM75A.simulate_failure, args="true")
[14:19:22] INFO: [ 23500ms] (temperature_monitor / test_led_does_not_turn_on_when_sensor_fails / temperature_ramp) SetEnv(temperature, type=EnvVarType.FLOAT, value=79)
[14:19:22] INFO: [ 23500ms] (temperature_monitor / test_led_does_not_turn_on_when_sensor_fails / temperature_ramp) SetEnv(temperature, type=EnvVarType.FLOAT, value=82, lerp_over=3s)
[14:19:22] INFO: Received event: {'type': 'event', 'timestamp': 23.801810957, 'component': 'LED', 'event': 'changed_state', 'data': 'off'}
[14:19:25] INFO: Disconnecting from NEST.
[14:19:25] INFO: Disconnected.

Scenario: temperature_monitor

────────────────────────────────────────────────────────────────────────────────
temperature_monitor / init / check_qemu                         [10.0s -> 10.5s]
────────────────────────────────────────────────────────────────────────────────

  [OK] EXPECT AlpineVM.qmp_response [10.0s -> 10.5s]

────────────────────────────────────────────────────────────────────────────────
temperature_monitor / test_led_turns_on_above_80                [10.5s -> 23.5s]
────────────────────────────────────────────────────────────────────────────────

  [OK] FORBID LED.changed_state [10.5s -> 20.5s]
  [OK] EXPECT LED.changed_state [20.5s -> 23.5s]

────────────────────────────────────────────────────────────────────────────────
temperature_monitor / test_led_does_not_turn_on_when_sensor_fails [23.5s -> 26.5s]
────────────────────────────────────────────────────────────────────────────────

  [OK] FORBID LED.changed_state [23.5s -> 26.5s]

────────────────────────────────────────────────────────────────────────────────
Summary:
  4 passed
  0 failed

Result: SUCCESS
```

As you can see, the scenario successfully completed, with all four event ranges
validated! Now, can you modify the scenario to make it fail?

## Conclusion

In this guide, we learned how to control a simulation by making components
interactive, observable, and by scripting and running an end-to-end test
scenario using `nest-client`.

This was the last guide of the 3-parts introduction series. You should now have
a good understanding of NEST's capabilities and how to use it for your use case
by developing custom components and running them alongside Linux VMs in
controlled and reproducible test scenarios.

Congratulations for making it to the end! :party_popper:
