use std::collections::HashMap;

use crate::component::Component;

pub struct Simulation {
    _topology: HashMap<String, Box<dyn Component>>,
}

impl Simulation {
    pub fn new() -> Self {
        Self { _topology: HashMap::new() }
    }

    pub fn run_topology(&mut self) {
        // let handles = self.topology.drain().map(|(name, mut component)|
        //     thread::Builder::new().name(name).spawn(move || {
        //         component.run().unwrap();
        //     }).unwrap()
        // );
        //
        // for handle in handles {
        //     handle.join().unwrap();
        // }
    }
}
