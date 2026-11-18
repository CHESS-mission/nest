# Introduction guides

NEST's introduction guides are a 3-part series covering the basics of the
framework, from setting up a simple virtual machine to running and testing
complete simulations with custom GPIO and I2C devices. What you will learn:

1. [Running VMs](./running-vms.md): Set up an Alpine Linux QEMU virtual machine
   and run it inside NEST
2. [Writing custom devices](./custom-devices.md): Create custom WebAssembly
   components to emulate external devices connected to the virtual machine via
   hardware ports
3. [Testing with scenarios](./scenarios.md): Make the custom components
   observable and interactive, and script complete testing scenarios to
   automatically validate system behavior
