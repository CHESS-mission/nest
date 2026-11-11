//! A test driver for NEST components: an I2C master, a GPIO driver and a serial
//! sender, commanded over the NEST command interface, reporting what it
//! receives as events. It exists so that a component can be exercised without a
//! virtual machine, which makes it usable from `verify.sh` and from CI.
//!
//! Commands: `i2c-read`, `i2c-write`, `i2c-poll`, `lines-set`, `serial-send`.
//! Events: `i2c-reply`, `lines-received`, `serial-received`, `poll-done`.

#[allow(warnings)]
mod bindings {
    wit_bindgen::generate!({
        world: "nest:harness/driver",
        path: "wit",
        generate_all,
    });
}

use bindings::nest::plugin::errors::{CommandError, InitError};
use bindings::nest::plugin::log;
use bindings::nest::plugin::types::{
    Bytes, I2cRequest, I2cRequestOp, I2cResponse, I2cResponseOp, I2cTransaction,
};
use bindings::{emit, i2c_out, lines_out, serial_out, set_timer, Guest, TimerId};

use std::cell::RefCell;
use std::collections::HashMap;

struct Harness;

#[derive(Clone)]
struct Poll {
    addr: u16,
    register: u8,
    len: u32,
    period_ms: u32,
    left: u32,
}

// Timers which belong to a repeating read, by timer ID. Timers cannot be
// cancelled, so an expired entry is dropped by its own callback.
thread_local! {
    static POLLS: RefCell<HashMap<TimerId, Poll>> = RefCell::new(HashMap::new());
}

/// Request the value of a register: a write of the pointer, then a read
fn read_register(addr: u16, register: u8, len: u32) {
    i2c_out(&I2cTransaction::Request(I2cRequest {
        addr,
        op: I2cRequestOp::Write(vec![register]),
    }));
    i2c_out(&I2cTransaction::Request(I2cRequest {
        addr,
        op: I2cRequestOp::Read(len),
    }));
}

fn write_register(addr: u16, register: u8, data: &[u8]) {
    let mut payload = vec![register];
    payload.extend_from_slice(data);
    i2c_out(&I2cTransaction::Request(I2cRequest {
        addr,
        op: I2cRequestOp::Write(payload),
    }));
}

/// Arguments arrive as the string rendering of the JSON value sent by the
/// client, so anything which is not an object is a mistake
fn arguments(args: &str) -> Result<serde_json::Value, String> {
    match serde_json::from_str(args) {
        Ok(value @ serde_json::Value::Object(_)) => Ok(value),
        Ok(value) => Err(format!("expected a JSON object, received {value}")),
        Err(error) => Err(format!("could not parse arguments ({error}); note that commands take a JSON object, sent as the `arguments` field of the command")),
    }
}

fn field_u64(args: &serde_json::Value, key: &str) -> Result<u64, String> {
    args.get(key)
        .and_then(|value| value.as_u64())
        .ok_or_else(|| format!("missing or invalid \"{key}\""))
}

fn field_bytes(args: &serde_json::Value, key: &str) -> Result<Bytes, String> {
    args.get(key)
        .and_then(|value| value.as_array())
        .map(|items| items.iter().filter_map(|value| value.as_u64().map(u8::try_from).map(Result::unwrap_or_default)).collect())
        .ok_or_else(|| format!("missing or invalid \"{key}\", expected an array of integers"))
}

impl Guest for Harness {
    fn init(options: Vec<(String, String)>) -> Result<(), InitError> {
        log::info(&format!("harness ready, {} option(s) ignored", options.len()));
        Ok(())
    }

    fn on_command(command: String, args: String) -> Result<(), CommandError> {
        let args = arguments(&args).map_err(CommandError::InvalidValue)?;
        let invalid = |error: String| CommandError::InvalidValue(error);

        match command.as_str() {
            "i2c-read" => {
                let addr = field_u64(&args, "addr").map_err(invalid)? as u16;
                let register = field_u64(&args, "register").map_err(invalid)? as u8;
                let len = field_u64(&args, "len").map_err(invalid)? as u32;
                read_register(addr, register, len);
                Ok(())
            }

            "i2c-write" => {
                let addr = field_u64(&args, "addr").map_err(invalid)? as u16;
                let register = field_u64(&args, "register").map_err(invalid)? as u8;
                let data = field_bytes(&args, "data").map_err(invalid)?;
                write_register(addr, register, &data);
                Ok(())
            }

            "i2c-poll" => {
                let poll = Poll {
                    addr: field_u64(&args, "addr").map_err(invalid)? as u16,
                    register: field_u64(&args, "register").map_err(invalid)? as u8,
                    len: field_u64(&args, "len").map_err(invalid)? as u32,
                    period_ms: field_u64(&args, "period_ms").map_err(invalid)? as u32,
                    left: field_u64(&args, "count").map_err(invalid)? as u32,
                };
                log::info(&format!(
                    "polling register {:#x} of {:#x} every {} ms, {} time(s)",
                    poll.register,
                    poll.addr,
                    poll.period_ms,
                    poll.left
                ));
                let id = set_timer(poll.period_ms.max(1));
                POLLS.with(|polls| polls.borrow_mut().insert(id, poll));
                Ok(())
            }

            "lines-set" => {
                let value = args.get("value").and_then(|value| value.as_bool()).unwrap_or(false);
                lines_out(value);
                Ok(())
            }

            "serial-send" => {
                let bytes = field_bytes(&args, "bytes").map_err(invalid)?;
                serial_out(&bytes);
                Ok(())
            }

            _ => Err(CommandError::UnknownCommand),
        }
    }

    fn on_timeout(id: TimerId) {
        // Never call the host while holding the borrow, since the host may in
        // turn call this instance
        let poll = POLLS.with(|polls| polls.borrow_mut().remove(&id));
        let Some(poll) = poll else { return };

        read_register(poll.addr, poll.register, poll.len);

        if poll.left > 1 {
            let id = set_timer(poll.period_ms.max(1));
            POLLS.with(|polls| polls.borrow_mut().insert(id, Poll { left: poll.left - 1, ..poll }));
        } else {
            emit(
                "poll-done",
                &format!("{{\"addr\":{},\"register\":{}}}", poll.addr, poll.register),
            );
        }
    }

    fn i2c_in(msg: I2cTransaction) {
        match msg {
            I2cTransaction::Response(I2cResponse { op: I2cResponseOp::Read(data) }) => {
                emit("i2c-reply", &serde_json::to_string(&data).unwrap_or_default());
            }
            I2cTransaction::Response(I2cResponse { op: I2cResponseOp::Write }) => {
                emit("i2c-reply", "{\"write\":true}");
            }
            I2cTransaction::Request(request) => {
                log::info(&format!("ignoring unexpected request to {:#x}", request.addr));
            }
        }
    }

    fn lines_in(msg: bool) {
        emit("lines-received", if msg { "true" } else { "false" });
    }

    fn serial_in(msg: Bytes) {
        emit("serial-received", &serde_json::to_string(&msg).unwrap_or_default());
    }
}

bindings::export!(Harness with_types_in bindings);
