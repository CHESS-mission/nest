use std::time::Instant;

use anyhow::{anyhow, bail, Result};
use serde::Deserialize;

use crate::component::{
    Component, ComponentHandle, ComponentHandleCommand, ComponentParams, HandlerStatus,
    MessageDispatcher,
};
use crate::ports::{AnySystemPort, I2cMessage, I2cTransaction, SystemPort};
use crate::{any_message_enum, with_message_wrapper};

// Config

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FuzzerConfig {
    #[serde(default)]
    fuzzed_ports: Vec<FuzzedGroupSpec>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FuzzedGroupSpec {
    name: String,
    syst_side: Vec<String>,
    comp_side: Vec<String>,
    // Optional I2C address of the sensor on the comp side.
    // When set, the syst→comp handler only forwards requests whose I2C
    // address matches this value.  This prevents other sensors on the
    // shared QEMU I2C bus from receiving requests that are not meant for
    // them and accidentally responding (masking a fault injection).
    #[serde(default)]
    sensor_address: Option<u16>,
}

// State

struct FuzzerState {
    fuzzing_select: bool,
}

impl Default for FuzzerState {
    fn default() -> Self {
        FuzzerState {
            fuzzing_select: false,
        }
    }
}

// Component

#[derive(Default)]
pub struct FuzzerComponent {
    fuzzed_ports: Vec<AnyFuzzedGroup>,
}

#[derive(Clone, Debug)]
struct FuzzedGroup<M: Clone + Send + Sync + 'static> {
    name: String,
    syst_side: Vec<Option<SystemPort<M>>>,
    comp_side: Vec<Option<SystemPort<M>>>,
    sensor_address: Option<u16>,
}

impl<M: Clone + Send + Sync + 'static> FuzzedGroup<M> {
    fn new(
        name: String,
        syst_side: Vec<Option<SystemPort<M>>>,
        comp_side: Vec<Option<SystemPort<M>>>,
        sensor_address: Option<u16>,
    ) -> Self {
        Self {
            name,
            syst_side,
            comp_side,
            sensor_address,
        }
    }
}

any_message_enum!(AnyFuzzedGroup, FuzzedGroup);

impl Component for FuzzerComponent {
    fn start(self: Box<Self>, _sim_start_time: Instant) -> Result<ComponentHandle> {
        let handle =
            ComponentHandle::start(move |command_receiver, event_sender, logger, _status| {
                let mut dispatcher = MessageDispatcher::<FuzzerState>::new();

                // Command handler
                dispatcher.register(
                    command_receiver,
                    Box::new(|msg, state| {
                        match msg {
                            ComponentHandleCommand::Stop => {
                                logger.info("Stopping fuzzer component")?;
                                return Ok(HandlerStatus::Stop);
                            }
                            ComponentHandleCommand::StartStatusUpdates => {}
                            ComponentHandleCommand::StopStatusUpdates => {}
                            ComponentHandleCommand::SimulatorCommand { command, args } => {
                                let args_string = args.to_string();
                                let result = match command.as_str() {
                                    "set_fuzzing_select" => {
                                        if let Ok(v) = serde_json::from_value::<bool>(args) {
                                            state.fuzzing_select = v;
                                            Ok(())
                                        } else {
                                            Err(format!(
                                                "Bad arguments for simulator command {}: {}",
                                                command, args_string
                                            ))
                                        }
                                    }
                                    _ => Err(format!("Unknown simulator command: {}", command)),
                                };

                                match result {
                                    Ok(()) => logger.info(format!(
                                        "Received simulator command: {} {}",
                                        command, args_string
                                    ))?,
                                    Err(err) => logger.error(&err)?,
                                };
                            }
                        }
                        Ok(HandlerStatus::Ok)
                    }),
                );

                // Per-group port handlers
                for group in &self.fuzzed_ports {
                    match group {
                        // I2C-specific branch: applies sensor_address filter
                        AnyFuzzedGroup::I2c(group) => {
                            let sensor_address = group.sensor_address;

                            // comp → syst  (sensor responses back toward QEMU)
                            for comp_port in &group.comp_side {
                                if let Some(comp_port) = comp_port {
                                    let syst_ports = group.syst_side.clone();
                                    dispatcher.register_system_port(
                                        comp_port,
                                        Box::new(move |msg: I2cMessage, state| {
                                            eprintln!("[FUZZER DBG] comp→syst: fuzzing_select={} sensor_address={:?} msg={:?}", state.fuzzing_select, sensor_address, msg);
                                            if !state.fuzzing_select {
                                                for syst_port in syst_ports.iter().flatten() {
                                                    syst_port.send(msg.clone());
                                                }
                                            }
                                            Ok(HandlerStatus::Ok)
                                        }),
                                    );
                                }
                            }

                            // syst → comp  (requests from QEMU toward sensor)
                            for syst_port in &group.syst_side {
                                if let Some(syst_port) = syst_port {
                                    let comp_ports = group.comp_side.clone();
                                    dispatcher.register_system_port(
                                        syst_port,
                                        Box::new(move |msg: I2cMessage, state| {
                                            let req_addr = match &msg {
                                                I2cTransaction::Request(r) => Some(r.addr),
                                                _ => None,
                                            };
                                            eprintln!("[FUZZER DBG] syst→comp: fuzzing_select={} sensor_address={:?} req_addr={:?}", state.fuzzing_select, sensor_address, req_addr);
                                            if !state.fuzzing_select {
                                                // Only forward requests addressed to our sensor.
                                                // Requests for other addresses are dropped here;
                                                // the correct fuzzer for that address will handle them.
                                                let addr_ok = match sensor_address {
                                                    None => true,
                                                    Some(addr) => matches!(
                                                        &msg,
                                                        I2cTransaction::Request(req) if req.addr == addr
                                                    ),
                                                };
                                                eprintln!("[FUZZER DBG] addr_ok={addr_ok}");
                                                if addr_ok {
                                                    for comp_port in comp_ports.iter().flatten() {
                                                        comp_port.send(msg.clone());
                                                    }
                                                }
                                            }
                                            Ok(HandlerStatus::Ok)
                                        }),
                                    );
                                }
                            }
                        }

                        // Generic branch: no address filter for non-I2C ports
                        other => {
                            with_message_wrapper!(AnyFuzzedGroup, other.clone(), |group| {
                                for comp_port in &group.comp_side {
                                    if let Some(comp_port) = comp_port {
                                        let syst_ports = group.syst_side.clone();
                                        dispatcher.register_system_port(
                                            comp_port,
                                            Box::new(move |msg, state| {
                                                if !state.fuzzing_select {
                                                    for syst_port in syst_ports.iter().flatten() {
                                                        syst_port.send(msg.clone());
                                                    }
                                                }
                                                Ok(HandlerStatus::Ok)
                                            }),
                                        );
                                    }
                                }
                                for syst_port in &group.syst_side {
                                    if let Some(syst_port) = syst_port {
                                        let comp_ports = group.comp_side.clone();
                                        dispatcher.register_system_port(
                                            syst_port,
                                            Box::new(move |msg, state| {
                                                if !state.fuzzing_select {
                                                    for comp_port in comp_ports.iter().flatten() {
                                                        comp_port.send(msg.clone());
                                                    }
                                                }
                                                Ok(HandlerStatus::Ok)
                                            }),
                                        );
                                    }
                                }
                            });
                        }
                    }
                }

                dispatcher.start_dispatch(FuzzerState {
                    fuzzing_select: false,
                })?;

                let _ = event_sender;
                Ok(())
            });

        Ok(handle)
    }

    fn from_params(mut params: ComponentParams) -> Result<Self>
    where
        Self: Sized,
    {
        let mut comp = Self::default();
        let config = serde_json::from_value::<FuzzerConfig>(params.config().clone())?;

        for group in &config.fuzzed_ports {
            if group.comp_side.is_empty() || group.syst_side.is_empty() {
                bail!(
                    "Fuzzed group {} must have at least one system-side and one component-side port",
                    group.name
                );
            }

            if let Some(first_port) = params.system_ports.remove(&group.syst_side[0]) {
                let sensor_address = group.sensor_address;
                with_message_wrapper!(AnySystemPort, first_port, |first_port| {
                    let mut syst_side_ports: Vec<Option<SystemPort<_>>> = vec![Some(first_port)];
                    for port_name in &group.syst_side[1..] {
                        syst_side_ports.push(
                            params
                                .system_ports
                                .remove(port_name)
                                .map(|port| {
                                    port.try_into().map_err(|_| {
                                        anyhow!("All ports in a fuzzed group must have the same type")
                                    })
                                })
                                .transpose()?,
                        );
                    }
                    let comp_side_ports = group
                        .comp_side
                        .iter()
                        .map(|port_name| {
                            params
                                .system_ports
                                .remove(port_name)
                                .map(|port| {
                                    port.try_into().map_err(|_| {
                                        anyhow!("All ports in a fuzzed group must have the same type")
                                    })
                                })
                                .transpose()
                        })
                        .collect::<Result<_>>()?;

                    comp.fuzzed_ports.push(
                        FuzzedGroup::new(group.name.clone(), syst_side_ports, comp_side_ports, sensor_address)
                            .into(),
                    );
                });
            } else {
                for port_name in &group.comp_side {
                    params.system_ports.remove(port_name);
                }
            }
        }

        if let Some((name, _)) = params.system_ports.iter().next() {
            bail!("Unknown system port {} for FuzzerComponent", name);
        }

        Ok(comp)
    }
}
