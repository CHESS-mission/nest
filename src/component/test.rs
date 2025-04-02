use anyhow::Result;

use super::{Component, DynamicSystemPortHub, GenericMessage, IntMessage, PortQueueCapacity, SystemPort, SystemPortHub, TextMessage};

pub struct TestComponentConfig {
    pub initial_sum: i32,
    pub greeting: String,
}

pub struct TestComponentState {
    pub current_sum: i32,
}

pub struct TestComponent {
    system_ports_hub: DynamicSystemPortHub<TestComponentState>,
    state: TestComponentState,
}

impl TestComponent {
    pub fn new(config: TestComponentConfig) -> Self {
        let mut hub = DynamicSystemPortHub::new();

        let cumsum: SystemPort<DynamicSystemPortHub<TestComponentState>, TestComponentState, IntMessage> = SystemPort::new(
            Box::new(move |hub, state, msg| {
                state.current_sum += msg.data;
                let output = TextMessage::new(&format!("Cumulative sum: {}", config.initial_sum + state.current_sum));
                hub.port_by_name("print").unwrap().send_generic(GenericMessage::Text(output))?;
                Ok(())
            }),
            PortQueueCapacity::Unbounded);

        let print : SystemPort<DynamicSystemPortHub<TestComponentState>, TestComponentState, TextMessage> = SystemPort::new(
            Box::new(move |_hub, _state, msg| {
                println!("{} Got: {}", config.greeting, msg.data);
                Ok(())
            }),
            PortQueueCapacity::Unbounded);

        hub.add_port("cumsum", Box::new(cumsum));
        hub.add_port("print", Box::new(print));

        Self { state: TestComponentState { current_sum: 0 }, system_ports_hub: hub }
    }
}

impl Component<TestComponentState> for TestComponent {
    fn system_port_hub(&self) -> &dyn SystemPortHub {
        &self.system_ports_hub
    }

    fn system_port_hub_mut(&mut self) -> &mut dyn SystemPortHub {
        &mut self.system_ports_hub
    }

    fn run(&mut self) -> Result<()> {
        self.system_ports_hub.loop_recv_dispatch(&mut self.state)
    }
}
