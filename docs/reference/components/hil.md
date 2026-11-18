---
title: Hardware-in-the-loop
---

# Hardware-in-the-loop Component

WIP

## Overview

WIP

## Example

WIP

```yaml title="topology.yaml"
  - name: HIL
    type: hil
    config:
      - i2c:
          main:
            speed: slow
          eps:
            speed: slow
        uart:
          adcs:
            - baudrate: 115200
              stop_bits: 1
              parity: none
          uhf_a:
            - baudrate: 115200
              stop_bits: 1
              parity: none
          uhf_b:
            - baudrate: 115200
              stop_bits: 1
              parity: none
          gnss_a:
            - baudrate: 115200
              stop_bits: 1
              parity: none
          gnss_b:
            - baudrate: 115200
              stop_bits: 1
              parity: none
          adcs_rs422:
            - baudrate: 115200
              stop_bits: 1
              parity: none
          spad_rs422:
            - baudrate: 115200
              stop_bits: 1
              parity: none
    ports:
      - gpio:
          - name: adcs_boot
            type: gpio
          - name: eps_hb_a
            type: gpio
          - name: eps_hb_b
            type: gpio
          - name: gnss_enable
            type: gpio
          - name: gnss_pwr_oc
            type: gpio
          - name: spad_cdone
            type: gpio
          - name: spad_pgood
            type: gpio
          - name: spad_reset
            type: gpio
          - name: spad_bkpcfg
            type: gpio
        i2c:
          - name: main
            type: i2c
          - name: eps
            type: i2c
        uart:
          - name: adcs
            type: uart
          - name: uhf_a
            type: uart
          - name: uhf_b
            type: uart
          - name: gnss_a
            type: uart
          - name: gnss_b
            type: uart
          - name: adcs_rs422
            type: uart
          - name: spad_rs422
            type: uart
```