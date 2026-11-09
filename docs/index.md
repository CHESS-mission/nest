---
title: Home
---

# NEST Documentation

![NEST logo](./images/NEST-logo-full.png#only-light){ width="60%", style="margin: 40px auto;" }
![NEST logo](./images/NEST-logo-full-dark.png#only-dark){ width="60%", style="margin: 40px auto;" }
/// caption
///

<center>
    <b>Welcome to the documentation for NEST!</b>
</center>

## About NEST

NEST (*Numerical Environment for Software Testing*) is modular framework for
reproducible software-in-the-loop testing of embedded systems. It is developed
by the [EPFL Spacecraft Team](https://www.epflspacecraftteam.ch/) as a tool for
system-level validation and testing of the flight software for the [CHESS
constellation](https://www.epflspacecraftteam.ch/project#chess) satellites.

NEST was born out of the need for a low-cost alternative to expensive hardware
testbeds in early stages of developments, capable of reproducing realistic
operational conditions, introducing hardware faults, and automatically
validating the behavior of the system against scripted scenarios.

## Features

<div class="grid cards" markdown>

-   :lucide-graph-star:{ .lg .middle } __Declarative topologies__

    ---

    Specify systems in YAML by connecting virtual machines and custom emulated
    hardware

    <!-- [:octicons-arrow-right-24: Topology specification](#) -->

-   :lucide-file-box:{ .lg .middle } __Custom devices__

    ---

    Emulate custom devices as WebAssembly plugins in Rust, C/C++, Python,
    or any supported programming language

-   :lucide-calendar-chevrons-right:{ .lg .middle } __Reproducible scenarios__

    ---

    Script scenarios to test the system under both nominal and faulty
    operational conditions

-   :lucide-replace:{ .lg .middle } __Full transparency__

    ---

    Test the exact software you will deploy on the real system without any
    instrumentation

</div>

## Getting started

A good place to get started is the [Introduction
guides](./guides/introduction/index.md).
