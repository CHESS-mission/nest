---
icon: lucide/file-box
---

# Writing custom devices

NEST only has very few component types out of the box. The previous guide
covered the *QEMU component*, which enables running QEMU virtual machines
inside simulations. This guide covers the second most important component: the
*WebAssembly (Wasm) component*, which enables writing portable custom devices
and load them in the simulation.

## The WebAssembly component

Devices loaded by the Wasm component are WebAssembly binaries, executed by NEST
via [Wasmtime](https://wasmtime.dev/) in a sandboxed environment. Therefore,
they can be written in any programming language with a WebAssembly compilation
target (e.g. C/C++, C#, Go, JavaScript, Rust, Python, etc.). The interface
between NEST and the WebAssembly devices uses the [WebAssembly Component
Model](https://component-model.bytecodealliance.org/), an interoperability
standard for WebAssembly binaries and host programs. It introduces a binary
format to encapsulate WebAssembly modules behind typed interfaces, called
[WIT](https://component-model.bytecodealliance.org/design/wit.html) worlds,
which define a set of imported and exported function signatures.

The `nest-component` WIT world serves as a contract between NEST and the custom
WebAssembly components, granting them a strict set of capabilities to implement
the behavior of most physical devices.  

!!! note

    This guide covers the process of creating custom components in **Rust**,
    which has excellent support for WIT. However, the process is very similar
    for other programming languages, and detailed information can be found in
    the Component Model's [documentation](https://component-model.bytecodealliance.org/language-support.html).

## Creating an LED component

Let's start with a simple example: create an LED component that can be turned
on and off via a GPIO line. Instead of emitting actual light, the component
will log its state changes, which we will be able to see in the console of
NEST's web interface.

### Requirements

Before getting started, make sure `rustup` is installed on your machine (see
[Installing Rust](https://rust-lang.org/learn/get-started/)), with the
`wasm32-wasip2` toolchain:

```bash
rustup target add wasm32-wasip2
```

Then, also install
[`wasm-tools`](https://github.com/bytecodealliance/wasm-tools),
[`wit-bindgen-cli`](https://github.com/bytecodealliance/wit-bindgen) and
[`wit-deps`](https://github.com/bytecodealliance/wit-deps) (recommended):

```bash
cargo install --locked wasm-tools wit-bindgen-cli wit-deps-cli
```

### Project setup

Start by creating a new Rust library project called `led`:

```bash
cargo new --lib led
```

Enter the newly created `led` directory, and set the crate type to C dynamic
library in `Cargo.toml`:

```toml title="Cargo.toml"
[lib]
crate-type = ["cdylib"]
```

Then add the `wit_bindgen` dependency:

```bash
cargo add wit_bindgen
```

#### Adding the NEST component world

Create a directory called `wit` with a file `deps.toml` and add the
`nest-plugin` dependency, which provide the `nest-component` WIT world and
related types:

```toml title="wit/deps.toml"
nest-plugin = "https://github.com/CHESS-mission/nest/archive/main.tar.gz"
```

Then install the dependencies using `wit-deps`:

```bash
wit-deps
```

!!! info

    NEST's WIT definitions are automatically fetched by `wit-deps` from github
    and written to `wit/deps/nest-plugin/`. Consider taking a look at
    `nest-component.wit` and `types.wit` as you progress through this guide to
    better understand how the pieces fit together!

### Creating the LED world

In NEST, WebAssembly plugins don't directly implement the `nest-component` WIT
world. Instead, they implement their own custom world which extends
`nest-component`. This means that we must first create a WIT world for our LED
component, which we will call `led`.

??? question "Wait, why?"

    This may sound unusual, and it arguably is. Under typical circumstances,
    Wasm components directly implement the world provided by the host software,
    because it describes the exact static interface between them. However, in
    NEST, components can have an arbitrary number of ports, each with their own
    API surface for maximum performance and to avoid dealing with dispatching
    logic inside the components. Therefore, NEST's approach is to let the
    components extend the minimal `nest-component` world with the exact API
    surface they require, and dynamically satisfies that interface at runtime
    via introspection and a few naming conventions.

Create the WIT file `wit/led.wit` with the `led` world definition:

[//]: # (Note: WIT is not yet supported by Pygments, using typescript instead)
[//]: # (See: https://github.com/pygments/pygments/pull/3017)
```typescript title="wit/led.wit"
package my-components:led; // (1)!

world led { // (2)!
  // Component (mandatory)
  include nest:plugin/nest-component; // (3)!

  // gpio port (4)
  export gpio-in: func(val: bool);
  import gpio-out: func(val: bool);
}
```

1. Every WIT world and interface must be part of a package. You may freely name
   the package in the form `<namespace>:<package>`.
2. Create a world called `led`.
3. Include the `nest-component` world provided by the `nest:plugin` dependency.
   It defines the set of imports and exports common to all components.
4. Create a port called `gpio`. Each port must correspond to exactly one
   exported / imported function pair named `<port_name>-in` / `<port_name>-out`
   (respectively) and each taking a single argument of the same supported type
   (`i32`, `f32`, `bool`, `bytes`, `string` and `i2c-transaction`).

!!! question "Why `bool`?"

    Notice how NEST doesn't have a specialized GPIO port type. In fact, the
    framework itself doesn't have an opinion about how GPIO ports should be
    modeled. However, the QEMU component does; it models GPIO lines as boolean
    ports. Therefore, any component that requires compatibility with the QEMU
    component must follow this convention.

### Generating bindings

Now, use [`wit-bindgen`](https://github.com/bytecodealliance/wit-bindgen) to
generate the Rust bindings for the `led` world:

```bash
wit-bindgen rust wit --generate-all --out-dir src
```

The generated bindings should be written at `src/led.rs`. Do not manually edit
this file.

!!! tip

    The `wit-bindgen` crate also offers a `wit_bindgen::generate!` macro for
    generating the bindings on the fly at compile time in Rust. It avoids the
    manual generation step and ensures the bindings remain in sync with the WIT
    files. See the
    [docs](https://component-model.bytecodealliance.org/language-support/building-a-simple-component/rust.html#4-generating-bindings-for-the-adder-interface)
    for more information.

### Implementing the component

It is now time to implement the LED component in `src/lib.rs`:

```rust title="src/lib.rs"
#[allow(warnings)]
mod led;

use led::Guest;

use crate::led::{CommandError, InitError, nest::plugin::log};

struct Led; // (1)!

impl Guest for Led { // (2)!
    fn init(_config: Vec<(String, String)>) -> Result<(), InitError> { // (3)!
        Ok(())
    }

    fn on_command(_command: String, _args: String) -> Result<(), CommandError> { // (4)!
        Err(CommandError::UnknownCommand)
    }

    fn gpio_in(val: bool) -> () { // (5)!
        let state = match val {
            true => "on",
            false => "off",
        };
        log::info(&format!("LED turned {}!", state)); // (6)!
    }
}

led::export!(Led with_types_in led); // (7)!
```

1. The component must exist as a struct. In this case, we leave it empty.
2. The component's struct must implement the `Guest` trait, automatically
   generated as part of the bindings. It forces the component to comply with
   the `led` world (i.e. implement `init`, `on_command` and `gpio_in`).
3. The `init` function is called once at the very start of a simulation. It
   receives configuration options as key-value pairs of strings. These options
   are set by the user when instantiating the component in the topology file.
4. The `on_command` function is called when the component receives a command
   during a simulation, typically when running a scenario. Components define
   what command they accept, with their arguments. In this case, we do not
   accept any command.
5. The `gpio_in` command was generated for the GPIO port we added. It will be
   called every time the component receives a message via that port.
6. The `log::info` function enables logging text from the component. NEST
   supports 3 different logging levels: `info`, `warning`, and `error`. Note
   that this is about all a device can ask of its host: it has no clock of its
   own, no filesystem, and no standard output. Printing with `println!` instead
   of logging, or waiting with `thread::sleep`, reaches a WASI function which
   the host does not implement, and traps. See
   [Host calls](../../reference/components/wasm.md#host-calls).
7. It is necessary to export the `Led` struct using the auto-generated
   `led::export!` macro.

!!! question "What about `gpio_out`?"

    Notice that we didn't use `gpio_out`. This is because our LED model never
    writes to its GPIO line of course, like the real thing! But then, you may
    wonder why we still had to declare the `gpio-out` import in `wit/led.wit`. And
    the answer is simply that ports are always bi-directional in NEST, by design,
    even if only one direction ends up being used. This mimics actual wires, where
    nothing prevents two devices from driving the same GPIO line. ...But in this
    case, prepare for unexpected behavior, both in NEST and actual hardware. :wink:

### Building the component

Let's now build the component with Cargo. Make sure to use the `wasm32-wasip2`
target:

```bash
cargo build --target wasm32-wasip2 --release
```

The resulting Wasm binary will be found in
`target/wasm32-wasip2/release/led.wasm`.

### Running the component in NEST

!!! warning

    This section assumes that you followed the previous guide and have a properly
    configured `alpine.qcow2` disk image in the working directory.

We can finally run our LED component in NEST!

Let's start by writing a topology with one VM and two LEDs connected to GPIO
lines 0 and 1:

```yaml title="topology.yaml"
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
        0: led1
        1: led2
    ports:
      system:
        - name: gpio0
          type: bool
        - name: gpio1
          type: bool

  - name: LED1
    type: wasm # (1)!
    config:
      module: ./led.wasm # (2)!
    ports:
      system:
        - name: gpio # (3)!
          type: bool

  - name: LED2
    type: wasm
    config:
      module: ./led.wasm
    ports:
      system:
        - name: gpio
          type: bool

wires:
  - 
    - component: AlpineVM
      port: gpio0
    - component: LED1
      port: gpio
  - 
    - component: AlpineVM
      port: gpio1
    - component: LED2
      port: gpio
```

1. To load our LED component, we create instances of the WebAssembly component
   (type `wasm`).
2. This is the path to the `led.wasm` binary. Make sure to move it to your
   local directory or change the path!
3. As for the QEMU component, we must explicitly declare all system ports. The
   name must match the port name of the component, otherwise NEST will fail to
   find the matching imports and exports!

Then, start the NEST server with the topology loaded and start the simulation
from the web app.

```bash
nest-server --topology topology.yaml
```

!!! tip

    You should see both instances of the LED component being successfully loaded in
    the console.

    ```toml title="NEST console"
    00:00:00.036: [LED1] [Info] Initializing component...
    00:00:00.036: [LED1] [Info] Initialization complete
    00:00:00.036: [LED2] [Info] Initializing component...
    00:00:00.036: [LED2] [Info] Initialization complete
    ```

Connect to the VM and log in:

```bash
picocom -b 115200 /tmp/qemu-serial-pty
```

Now try toggling the `led1` and `led2` lines from the VM using `gpioset`. You
will see the LEDs logging their state changes to the console in real time!

```bash
localhost:~# gpioset -t 0 led1=1
localhost:~# gpioset -t 0 led1=0
localhost:~# gpioset -t 0 led2=1
localhost:~# gpioset -t 0 led2=0
```

```toml title="NEST console"
00:00:56.169: [LED1] [Info] LED turned on!
00:00:59.669: [LED1] [Info] LED turned off!
00:01:06.532: [LED2] [Info] LED turned on!
00:01:11.396: [LED2] [Info] LED turned off!
```

That's it for the LED component. Next, we will create a more interesting I2C
component to showcase additional capabilities of NEST.

## Creating an I2C temperature sensor component

The LED component was a nice toy example to get started, however, it is neither
very impressive nor terribly useful in practice. Let's now dive a bit deeper
and create a more advanced I2C temperature sensor component inspired by the
[LM75A Digital Temperature
Sensor](https://www.ti.com/lit/ds/symlink/lm75a.pdf).

!!! note

    The LM75A Digital Temperature Sensor is a moderately complex device to
    emulate, as it features multiple registers, configurable interrupts, etc.
    For the sake of simplicity, we will only emulate the I2C bus and a single
    I2C register (the read-only temperature register). However, it is absolutely
    possible to emulate the full device accurately—which could make for a great
    exercise!

### LM75A world & setup

Start by setting up a new project by following the instructions for the LED
component up to and including [Generating bindings](#generating-bindings), with
two modifications:

1. Replace the name `led` with `lm75a`;
2. Use the world `lm75a.wit` below.

```typescript title="wit/lm75a.wit"
package my-components:lm75a; 

world lm75a { 
  include nest:plugin/nest-component; 

  // Import i2c-transaction type
  use nest:plugin/types.{i2c-transaction};

  // I2C port
  export i2c-in: func(msg: i2c-transaction);
  import i2c-out: func(msg: i2c-transaction);
}
```

As you can see, we define a single I2C port called `i2c`. Because
`i2c-transaction` is not a native WIT type, it needs to be imported from the
NEST plugin type definitions.

### I2C basics

Let's now implement the component. Our sensor will only have a single read-only
register for reading the current temperature, following to the LM75A's
temperature register specifications—therefore ignoring I2C write requests. For
now, it will answer read requests with a fixed temperature of **23 °C** and use the
default I2C address **0x48**.

```rust title="src/lib.rs"
#[allow(warnings)]
mod lm75a;

use lm75a::{
    get_time,
    i2c_out,
    Guest,
};

use crate::lm75a::nest::plugin::{
    errors::{CommandError, InitError},
    log,
    types::{
        I2cRequestOp,
        I2cResponse,
        I2cResponseOp,
        I2cTransaction,
    },
};

const DEFAULT_I2C_ADDRESS: u8 = 0x48;

// LM75A operating range.
const MIN_TEMPERATURE_C: f32 = -55.0;
const MAX_TEMPERATURE_C: f32 = 125.0;

fn celsius_to_half_degrees(temperature_c: f32) -> i16 {
    let clamped = temperature_c.clamp(
        MIN_TEMPERATURE_C,
        MAX_TEMPERATURE_C,
    );

    // LM75A resolution is 0.5°C.
    (clamped * 2.0).round() as i16
}

/// Encode a signed half-degree value in the TI LM75A 9-bit format.
///
/// The corresponding signed 9-bit value occupies bits D15:D7.
/// Bits D6:D0 are zero.
fn encode_temperature_register(value_half_degrees: i16) -> [u8; 2] {
    let raw = value_half_degrees << 7;

    [
        ((raw >> 8) & 0xFF) as u8,
        (raw & 0xFF) as u8,
    ]
}

fn read_temperature_register(temperature_half_degrees: i16, count: u32) -> Vec<u8> {
    let register_data =
        encode_temperature_register(
            temperature_half_degrees,
        ).to_vec();

    let count = count as usize;
    let mut result = Vec::with_capacity(count);

    // Reads beyond the temperature register's width return zero in this model.
    for index in 0..count {
        result.push(
            register_data
                .get(index)
                .copied()
                .unwrap_or(0x00),
        );
    }

    result
}

struct Lm75a;

// -----------------------------------------------------------------------------
// Component implementation
// -----------------------------------------------------------------------------

impl Guest for Lm75a {
    fn init(_options: Vec<(String, String)>) -> Result<(), InitError> {
        Ok(())
    }

    fn on_command(
        _command: String,
        _args: String,
    ) -> Result<(), CommandError> {
        // No supported command
        Err(CommandError::UnknownCommand)
    }

    fn i2c_in(msg: I2cTransaction) {
        // Only answer I2C requests; ignore responses
        let I2cTransaction::Request(request) = msg else {
            return;
        };

        // Ignore requests to other I2C addresses
        if request.addr != DEFAULT_I2C_ADDRESS as u16 {
            return;
        }

        log::info(&format!(
            "I2C request at {:.3}s: addr=0x{:02X}, op={:?}",
            get_time(), // (1)!
            request.addr,
            request.op,
        ));

        match request.op { // (2)!
            I2cRequestOp::Write(_data) => {
                log::warning("Write requests are unsupported; ignoring");

                // Write requests must still be acknowledged, otherwise they will time out
                let response = I2cTransaction::Response(I2cResponse {
                    op: I2cResponseOp::Write,
                });

                i2c_out(&response);
            }

            I2cRequestOp::Read(count) => {
                let temperature = celsius_to_half_degrees(23.0);
                let data = read_temperature_register(temperature, count);

                log::info(&format!(
                    "Read {} byte(s) from the temperature register: {:?}",
                    count,
                    data,
                ));

                // Send back the data
                let response = I2cTransaction::Response(I2cResponse {
                    op: I2cResponseOp::Read(data),
                });

                i2c_out(&response);
            }
        }
    }
}

lm75a::export!(Lm75a with_types_in lm75a);
```

1. `get_time()` returns the current time elapsed since the start of the
   simulation, in seconds. It can be useful for real-time components that need
   to keep track of the time elapsed between two port invocations, for example.
   Here, it is mostly used just to let you know it exists!
2. This is the core of the implementation. Make sure you spend some time
   understanding what is happening! Note the two I2C request types (read and
   write), and how in both cases we create a response object that we send back
   by calling `i2c_out`, our port's output binding.

Then, create a topology to load the component and connect it to a VM via I2C:

```yaml title="topology.yaml"
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
      i2c_timeout_ms: 10
    ports:
      system:
        - name: i2c
          type: i2c

  - name: LM75A
    type: wasm
    config:
      module: ./lm75a.wasm
    ports:
      system:
        - name: i2c
          type: i2c

wires:
  - 
    - component: AlpineVM
      port: i2c
    - component: LM75A
      port: i2c

```

Start the simulation and connect to the VM. Try listing the I2C devices:

```text
localhost:~# i2cdetect -y 1
     0  1  2  3  4  5  6  7  8  9  a  b  c  d  e  f
00:                         -- -- -- -- -- -- -- -- 
10: -- -- -- -- -- -- -- -- -- -- -- -- -- -- -- -- 
20: -- -- -- -- -- -- -- -- -- -- -- -- -- -- -- -- 
30: -- -- -- -- -- -- -- -- -- -- -- -- -- -- -- -- 
40: -- -- -- -- -- -- -- -- 48 -- -- -- -- -- -- -- 
50: -- -- -- -- -- -- -- -- -- -- -- -- -- -- -- -- 
60: -- -- -- -- -- -- -- -- -- -- -- -- -- -- -- -- 
70: -- -- -- -- -- -- -- --
```

You should see the sensor listed at address 0x48!

??? question "What do you expect to happen if we remove the `request.addr != DEFAULT_I2C_ADDRESS` guard?"

    As you might guess, the sensor will then start replying at every I2C
    address, leaving no room for other I2C devices.

    ```text
    localhost:~# i2cdetect -y 1
         0  1  2  3  4  5  6  7  8  9  a  b  c  d  e  f
    00:                         08 09 0a 0b 0c 0d 0e 0f 
    10: 10 11 12 13 14 15 16 17 18 19 1a 1b 1c 1d 1e 1f 
    20: 20 21 22 23 24 25 26 27 28 29 2a 2b 2c 2d 2e 2f 
    30: 30 31 32 33 34 35 36 37 38 39 3a 3b 3c 3d 3e 3f 
    40: 40 41 42 43 44 45 46 47 48 49 4a 4b 4c 4d 4e 4f 
    50: 50 51 52 53 54 55 56 57 58 59 5a 5b 5c 5d 5e 5f 
    60: 60 61 62 63 64 65 66 67 68 69 6a 6b 6c 6d 6e 6f 
    70: 70 71 72 73 74 75 76 77
    ```

### Reading the temperature

Now, let's actually read the temperature. Since reading the temperature
register and performing conversion manually is a bit tedious, let's use a
Python script for that. From inside the Alpine VM, install
[uv](https://docs.astral.sh/uv/):

```bash
apk add uv
```

And write a Python script named `read_temp.py`, using the `smbus2` library to
perform I2C operations:

```python title="read_temp.py"
import smbus2

LM75A_ADDRESS = 0x48

def read_temp():
    # Read 2 bytes from the temperature register (offset 0)
    with smbus2.SMBus(1) as bus:
        data = bus.read_i2c_block_data(LM75A_ADDRESS, 0, 2)
    
    # Combine MSB and LSB
    raw_temp = (data[0] << 8 | data[1]) >> 5
    
    # Handle negative temperatures
    if raw_temp & (1 << 10):
        raw_temp -= (1 << 11)
        
    celsius = raw_temp * 0.125
    return celsius

temp_c = read_temp()
print(f"Temperature: {temp_c:.3f} °C")
```

Finally, execute the script with uv:

```bash
uv run -w smbus2 read_temp.py
```

You should see the script print the hard-coded 23 °C temperature!

### Statefulness

So far, both components created in this guide have been stateless (i.e., no
information was persisted between two invocations of the component). Let's now
see how to make our sensor stateful, and we will illustrate this by making its
I2C address configurable.

Start by updating the implementation of the component:

```rust title="src/lib.rs"
#[allow(warnings)]
mod lm75a;

use lm75a::{
    get_time,
    i2c_out,
    Guest,
};

use crate::lm75a::nest::plugin::{
    errors::{CommandError, InitError},
    log,
    types::{
        I2cRequestOp,
        I2cResponse,
        I2cResponseOp,
        I2cTransaction,
    },
};

const DEFAULT_I2C_ADDRESS: u8 = 0x48;

// LM75A operating range.
const MIN_TEMPERATURE_C: f32 = -55.0;
const MAX_TEMPERATURE_C: f32 = 125.0;

struct Lm75a { // (1)!
    // Device I2C address
    i2c_address: u8,

    // Values are stored as signed half-degrees Celsius.
    temperature_half_degrees: i16,
}

impl Lm75a {
    fn new() -> Self {
        Self {
            i2c_address: DEFAULT_I2C_ADDRESS,
            temperature_half_degrees: 0,
        }
    }

    fn celsius_to_half_degrees(temperature_c: f32) -> i16 {
        let clamped = temperature_c.clamp(
            MIN_TEMPERATURE_C,
            MAX_TEMPERATURE_C,
        );

        // LM75A resolution is 0.5°C.
        (clamped * 2.0).round() as i16
    }

    /// Encode a signed half-degree value in the TI LM75A 9-bit format.
    ///
    /// The corresponding signed 9-bit value occupies bits D15:D7.
    /// Bits D6:D0 are zero.
    fn encode_temperature_register(value_half_degrees: i16) -> [u8; 2] {
        let raw = value_half_degrees << 7;

        [
            ((raw >> 8) & 0xFF) as u8,
            (raw & 0xFF) as u8,
        ]
    }

    fn read_temperature_register(&self, count: u32) -> Vec<u8> {
        let register_data =
            Self::encode_temperature_register(
                self.temperature_half_degrees,
            ).to_vec();

        let count = count as usize;
        let mut result = Vec::with_capacity(count);

        // Reads beyond the temperature register's width return zero in this model.
        for index in 0..count {
            result.push(
                register_data
                    .get(index)
                    .copied()
                    .unwrap_or(0x00),
            );
        }

        result
    }
}

// -----------------------------------------------------------------------------
// Global component state
// -----------------------------------------------------------------------------

static mut LM75A: Option<Lm75a> = None; // (2)!

fn get_self() -> &'static mut Lm75a {
    unsafe { // (3)!
        let sensor_ptr = core::ptr::addr_of_mut!(LM75A);
        if (*sensor_ptr).is_none() {
            *sensor_ptr = Some(Lm75a::new());
        }
        (*sensor_ptr).as_mut().unwrap()
    }
}

// -----------------------------------------------------------------------------
// Component implementation
// -----------------------------------------------------------------------------

impl Guest for Lm75a {
    fn init(options: Vec<(String, String)>) -> Result<(), InitError> {
        let self_ = get_self(); // (4)!

        for (option, value) in options {
            match option.as_str() {
                "i2c_address" => { // (5!)
                    let parsed = if let Some(hex) = value
                        .strip_prefix("0x")
                        .or_else(|| value.strip_prefix("0X"))
                    {
                        u8::from_str_radix(hex, 16)
                    } else {
                        value.parse::<u8>()
                    }
                    .map_err(|_| {
                        InitError::InvalidValue(
                            "i2c_address must be an integer or hexadecimal byte"
                                .into(),
                        )
                    })?;

                    // Only allow addresses between 0x48 and 0x4F
                    if !(0x48..=0x4F).contains(&parsed) {
                        return Err(InitError::InvalidValue(
                            "LM75A address must be between 0x48 and 0x4F"
                                .into(),
                        ));
                    }

                    self_.i2c_address = parsed;
                }

                unknown => {
                    return Err(InitError::UnknownOption(
                        unknown.to_string(),
                    ));
                }
            }
        }

        log::info(&format!(
            "Initialized at I2C address 0x{:02X}",
            self_.i2c_address,
        ));

        // Hard-code initial temperature
        self_.temperature_half_degrees = Self::celsius_to_half_degrees(23.0);

        Ok(())
    }

    fn on_command(
        _command: String,
        _args: String,
    ) -> Result<(), CommandError> {
        // No supported command
        Err(CommandError::UnknownCommand)
    }

    fn i2c_in(msg: I2cTransaction) {
        // Only answer I2C requests; ignore responses
        let I2cTransaction::Request(request) = msg else {
            return;
        };

        let self_ = get_self();

        // Ignore requests to other I2C addresses
        if request.addr != self_.i2c_address as u16 { // (6)!
            return;
        }

        log::info(&format!(
            "I2C request at {:.3}s: addr=0x{:02X}, op={:?}",
            get_time(),
            request.addr,
            request.op,
        ));

        match request.op {
            I2cRequestOp::Write(_data) => {
                log::warning("Write requests are unsupported; ignoring");

                // Write requests must still be acknowledged, otherwise they will time out
                let response = I2cTransaction::Response(I2cResponse {
                    op: I2cResponseOp::Write,
                });

                i2c_out(&response);
            }

            I2cRequestOp::Read(count) => {
                let data = self_.read_temperature_register(count); // (7)!

                log::info(&format!(
                    "Read {} byte(s) from the temperature register: {:?}",
                    count,
                    data,
                ));

                // Send back the data
                let response = I2cTransaction::Response(I2cResponse {
                    op: I2cResponseOp::Read(data),
                });

                i2c_out(&response);
            }
        }
    }
}

lm75a::export!(Lm75a with_types_in lm75a);
```

1. `i2c_address` and `temperature_half_degrees` are now members of the `Lm75a`
   struct, and their value will persist between component invocations.
2. We must create a singleton instance of the `Lm75a` struct to store the data
   in memory. The instance is initialized and returned by `get_self()`.
3. The unsafe block is required here, because the Rust compiler can't guarantee
   that the program's accesses to mutable global state are exclusive. For
   example, performing two concurrent invocations of our component would create
   a race condition. Thankfully, while NEST is multi-threaded at the component
   level, the Wasm component is itself single-threaded and never performs
   such concurrent invocations.
4. Since the `Guest` bindings functions don't take a `self` argument, we must
   always obtain the mutable reference to the singleton `Lm75a` struct with
   `get_self()` to access and modify its state.
5. This parses the `i2c_address` configuration option, if given, to configure
   the sensor's I2C address.
6. We can now use the stored I2C address instead of the hard-coded default!
7. Similarly, we can now answer with the stored temperature.

Note that we added the `i2c_address` configuration option to our component.
Options are simple key-value pairs of strings in the topology file, passed to
the component at initialization. The component is responsible for the parsing
logic and deciding which options to accept or reject (with
`InitError::UnknownOption` or `InitError::InvalidValue`). To try it, compile
the new version of the component and update the topology with an options list
for the LM75A instance:

```yaml title="topology.yaml"
components:
  - name: LM75A
    type: wasm
    config:
      module: ./lm75a.wasm
      options:
        - [ i2c_address, '0x4A' ]
```

Start a new simulation and observe that the address of the sensor changed to
0x4A as expected.

```bash
i2cdetect -y 1
```

### Reading temperature from an environment variable

Currently, the temperature is hard-coded to 23 °C. In practice, proper
integration tests often need to cover scenarios within different temperature
ranges, or even where the temperature evolves in real time. NEST's primary
mechanism for this is called *environment variables* (not to be confused with
Unix-style environment variables). A topology can declare any number of
environment variables of a variety of supported types, managed by NEST as part
of the simulation. They can be accessed read-only by components (via dedicated
*environment ports*), and driven externally by a client software. We will now
focus on letting our sensor read from a temperature environment variable, and
in the next guide, we will see how to use NEST's Python client to drive
that environment variable's value.

Start by updating the component's WIT world with an environment port named `temperature`:

```typescript title="wit/lm75a.wit"
package my-components:lm75a; 

world lm75a { 
  include nest:plugin/nest-component; 

  use nest:plugin/types.{i2c-transaction};

  // I2C port
  export i2c-in: func(msg: i2c-transaction);
  import i2c-out: func(msg: i2c-transaction);

  // Temperature environment port
  import read-temperature: func() -> f32;
}
```

!!! info

    Similarly to system ports, environment ports rely on a naming convention
    for NEST to properly discover them at runtime. An environment port corresponds
    to exactly one imported function named `read-<port_name>` taking no argument
    and returning a value of any supported type (`i32`, `f32`, `bool`, `vec2`,
    `vec3`, `quaternion` and `tensor`).

Regenerate the bindings, then update the component to read the temperature via
the newly generated `read_temperature` binding before answering I2C requests:

```rust title="src/lib.rs"
self_.temperature_half_degrees = Self::celsius_to_half_degrees(
    read_temperature()
);
let data = self_.read_temperature_register(count);
```

Finally, the topology must also be updated with two additions: we must declare
a new environment variable for the temperature in the `environment_variables`
section, and we must bind that variable to the component's `temperature`
environment port.

```yaml title="topology.yaml"
environment_variables:
  - name: temperature # (1)!
    type: float

components:
  - name: LM75A
    type: wasm
    config:
      module: ./lm75a.wasm
      options:
        - [ i2c_address, '0x48' ]
    ports:
      system:
        - name: i2c
          type: i2c
      environment:
        - name: temperature # (2)!
          variable: temperature
          type: float
```

1. Each environment variable has a unique name and a type (`int`, `float`,
   `bool`, `vec2`, `vec3`, `quaternion` or `tensor`).
2. Each environment port has a name (the same name used in the WIT world), a
   type, and must be bound to an environment variable of the same type. Note
   that the port and variable both being named `temperature` is a coincidence;
   they could be named completely differently. Also, multiple environment ports
   can be bound to a single environment variable.

And we are done, no more hard-coded temperature :smile:

!!! note

    If you try reading the sensor now, you will notice that the returned
    temperature is 0. This is the default value for an environment variable of
    type float. Unfortunately, there is no way to modify this value before we
    see how to use NEST's Python client to execute dynamic scenarios in the
    next guide.

### Polling the temperature with the timeout extension

Our latest version of the sensor reads the temperature from an environment
variable when it receives a read-request and always returns a perfectly
up-to-date value. While that may be a sufficiently accurate model for some
applications, in reality the LM75A sensor performs readings and conversions at
a fixed rate of 10Hz, and returns the latest conversion result, which may be
outdated by up to 100ms. This cannot be modeled natively using the bindings of
the `nest-component` world, due to NEST's event-driven architecture and the
absence of a non-blocking wait capability.

To address this limitation, custom WebAssembly components can use the optional
`timeout-ext` world; an extension of the `nest-component` world that adds the
ability to schedule timers with callbacks at the millisecond granularity.

??? question "Why optional?"

    Because making it directly part of `nest-component` would require all
    components to satisfy a `on-timeout` export, which would be unnecessarily
    verbose for those who do not need it.

Let's modify our sensor component to make temperature conversions at 10Hz using
the timeout extension, instead of making the conversion lazily when a reading
is requested. First, include the `timeout-ext` world in the `lm75a` WIT world:

```typescript title="wit/lm75a.wit"
package my-components:lm75a; 

world lm75a { 
  include nest:plugin/nest-component; 
  include nest:plugin/timeout-ext;

  use nest:plugin/types.{i2c-transaction};

  // I2C port
  export i2c-in: func(msg: i2c-transaction);
  import i2c-out: func(msg: i2c-transaction);

  // Temperature environment port
  import read-temperature: func() -> f32;
}
```

Regenerate the bindings, then implement the `on_timeout` function of the
`Guest` trait. Timers are scheduled with the `set_timer(duration_ms)` function,
which returns a `TimerId` object. When a timer expires, `on_timeout` is called
with that ID as an argument to differentiate between multiple concurrent
timers.

```rust title="src/lib.rs"
#[allow(warnings)]
mod lm75a;

use lm75a::{
    get_time,
    i2c_out,
    read_temperature,
    set_timer,
    Guest,
    TimerId,
};

use crate::lm75a::nest::plugin::{
    errors::{CommandError, InitError},
    log,
    types::{
        I2cRequestOp,
        I2cResponse,
        I2cResponseOp,
        I2cTransaction,
    },
};

const DEFAULT_I2C_ADDRESS: u8 = 0x48;

const CONVERSION_PERIOD_MS: u32 = 100;

// LM75A operating range.
const MIN_TEMPERATURE_C: f32 = -55.0;
const MAX_TEMPERATURE_C: f32 = 125.0;

struct Lm75a {
    // Device I2C address
    i2c_address: u8,

    // Values are stored as signed half-degrees Celsius.
    temperature_half_degrees: i16,

    // Temperature conversion timer ID 
    conversion_timer: Option<TimerId>, // (1)!
}

impl Lm75a {
    fn new() -> Self {
        Self {
            i2c_address: DEFAULT_I2C_ADDRESS,
            temperature_half_degrees: 0,
            conversion_timer: None,
        }
    }

    fn celsius_to_half_degrees(temperature_c: f32) -> i16 {
        let clamped = temperature_c.clamp(
            MIN_TEMPERATURE_C,
            MAX_TEMPERATURE_C,
        );

        // LM75A resolution is 0.5°C.
        (clamped * 2.0).round() as i16
    }

    /// Encode a signed half-degree value in the TI LM75A 9-bit format.
    ///
    /// The corresponding signed 9-bit value occupies bits D15:D7.
    /// Bits D6:D0 are zero.
    fn encode_temperature_register(value_half_degrees: i16) -> [u8; 2] {
        let raw = value_half_degrees << 7;

        [
            ((raw >> 8) & 0xFF) as u8,
            (raw & 0xFF) as u8,
        ]
    }

    fn schedule_conversion(&mut self) {
        self.conversion_timer = Some(set_timer(CONVERSION_PERIOD_MS)); // (2)!
    }

    fn perform_conversion(&mut self) {
        let environment_temperature_c: f32 = read_temperature();

        self.temperature_half_degrees =
            Self::celsius_to_half_degrees(environment_temperature_c);
    }

    fn read_temperature_register(&self, count: u32) -> Vec<u8> {
        let register_data =
            Self::encode_temperature_register(
                self.temperature_half_degrees,
            ).to_vec();

        let count = count as usize;
        let mut result = Vec::with_capacity(count);

        // Reads beyond the temperature register's width return zero in this model.
        for index in 0..count {
            result.push(
                register_data
                    .get(index)
                    .copied()
                    .unwrap_or(0x00),
            );
        }

        result
    }
}

// -----------------------------------------------------------------------------
// Global component state
// -----------------------------------------------------------------------------

static mut LM75A: Option<Lm75a> = None;

fn get_self() -> &'static mut Lm75a {
    unsafe {
        let sensor_ptr = core::ptr::addr_of_mut!(LM75A);
        if (*sensor_ptr).is_none() {
            *sensor_ptr = Some(Lm75a::new());
        }
        (*sensor_ptr).as_mut().unwrap()
    }
}

// -----------------------------------------------------------------------------
// Component implementation
// -----------------------------------------------------------------------------

impl Guest for Lm75a {
    fn init(options: Vec<(String, String)>) -> Result<(), InitError> {
        let self_ = get_self();

        for (option, value) in options {
            match option.as_str() {
                "i2c_address" => {
                    let parsed = if let Some(hex) = value
                        .strip_prefix("0x")
                        .or_else(|| value.strip_prefix("0X"))
                    {
                        u8::from_str_radix(hex, 16)
                    } else {
                        value.parse::<u8>()
                    }
                    .map_err(|_| {
                        InitError::InvalidValue(
                            "i2c_address must be an integer or hexadecimal byte"
                                .into(),
                        )
                    })?;

                    // Only allow addresses between 0x48 and 0x4F
                    if !(0x48..=0x4F).contains(&parsed) {
                        return Err(InitError::InvalidValue(
                            "LM75A address must be between 0x48 and 0x4F"
                                .into(),
                        ));
                    }

                    self_.i2c_address = parsed;
                }

                unknown => {
                    return Err(InitError::UnknownOption(
                        unknown.to_string(),
                    ));
                }
            }
        }

        log::info(&format!(
            "Initialized at I2C address 0x{:02X}",
            self_.i2c_address,
        ));

        // Perform initial temperature conversion and schedule the next
        self_.perform_conversion();
        self_.schedule_conversion(); // (3)!

        Ok(())
    }

    fn on_command(
        _command: String,
        _args: String,
    ) -> Result<(), CommandError> {
        // No supported command
        Err(CommandError::UnknownCommand)
    }

    fn on_timeout(timer_id: TimerId) {
        let self_ = get_self();

        // Ignore stale timers (should never happen in this example, since there is always at most
        // one concurrent timer)
        if self_.conversion_timer != Some(timer_id) {
            return;
        }

        // Convert and reschedule
        self_.perform_conversion();
        self_.schedule_conversion(); // (4)!
    }

    fn i2c_in(msg: I2cTransaction) {
        // Only answer I2C requests; ignore responses
        let I2cTransaction::Request(request) = msg else {
            return;
        };

        let self_ = get_self();

        // Ignore requests to other I2C addresses
        if request.addr != self_.i2c_address as u16 {
            return;
        }

        log::info(&format!(
            "I2C request at {:.3}s: addr=0x{:02X}, op={:?}",
            get_time(),
            request.addr,
            request.op,
        ));

        match request.op {
            I2cRequestOp::Write(_data) => {
                log::warning("Write requests are unsupported; ignoring");

                // Write requests must still be acknowledged, otherwise they will time out
                let response = I2cTransaction::Response(I2cResponse {
                    op: I2cResponseOp::Write,
                });

                i2c_out(&response);
            }

            I2cRequestOp::Read(count) => {
                let data = self_.read_temperature_register(count); // (5)!

                log::info(&format!(
                    "Read {} byte(s) from the temperature register: {:?}",
                    count,
                    data,
                ));

                // Send back the data
                let response = I2cTransaction::Response(I2cResponse {
                    op: I2cResponseOp::Read(data),
                });

                i2c_out(&response);
            }
        }
    }
}

lm75a::export!(Lm75a with_types_in lm75a);
```

1. We store the latest conversion timer's ID to know what `on_timeout` fired
   for. Note that this is not strictly necessary when the component is
   guaranteed to always have at most one concurrent timer.
2. `set_timer` schedules a new timer and returns its `TimerID`.
3. We must schedule the initial conversion timer in `init`, otherwise
   `on_timeout` will never be called for the first time.
4. In order to get repeating callbacks, we must schedule the next conversion
   timer in `on_timeout`.
5. We now return the latest measurement performed by the component's 10Hz
   conversion cycle.

There is nothing to change in the topology this time. Recompile the component,
and it will work out of the box. However, there is not much we can manually
test this time. Try adding some logging if you want to make sure the conversion
timers are working!

## Conclusion

In this guide, we learned how to write custom components in WebAssembly to
emulate a simple LED and a more advanced I2C temperature sensor with
statefulness, environment variables and asynchronous callbacks. We also tested
both components manually from the Linux VM we set up in the previous guide. In
the next guide, we will learn how to make components even more interactive with
commands and events, and to automate testing with scenarios using the NEST's
Python client!
