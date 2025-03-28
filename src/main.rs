use std::rc::Rc;
use std::cell::RefCell;
use std::collections::VecDeque;
use extism::*;

struct Wire {
    a_to_b: VecDeque<String>, // Deque for FIFO queue
    b_to_a: VecDeque<String>,
    // Might need other DS for concurrent threads (async, crossbeam) ?
}

impl Wire {
    fn new() -> Self {
        Wire {
            a_to_b: VecDeque::new(),
            b_to_a: VecDeque::new(),
        }
    }
}

// Interface for components
trait MyComponent {
    fn send(&self, msg: &str);
    fn receive(&self);
}

struct CompA {
    name: String,
    wire: Rc<RefCell<Wire>>,
}

impl MyComponent for CompA {
    fn send(&self, msg: &str) {
        let mut w = self.wire.borrow_mut();
        w.a_to_b.push_back(msg.to_string());
        println!("{} sent: {}", self.name, msg);
    }

    fn receive(&self) {
        let mut w = self.wire.borrow_mut();
        if let Some(m) = w.b_to_a.pop_front() {
            println!("{} received: {}", self.name, m);
        }
    }
}

struct CompB {
    name: String,
    wire: Rc<RefCell<Wire>>,
}

impl MyComponent for CompB {
    fn send(&self, msg: &str) {
        let mut w = self.wire.borrow_mut();
        w.b_to_a.push_back(msg.to_string());
        println!("{} sent: {}", self.name, msg);
    }

    fn receive(&self) {
        let mut w = self.wire.borrow_mut();
        if let Some(m) = w.a_to_b.pop_front() {
            println!("{} received: {}", self.name, m);
        }
    }
}

// WASM Component
struct CompC {
    name: String,
    plugin: Plugin,
    wire: Rc<RefCell<Wire>>,
}

impl CompC {
    fn new(name: &str, wire: Rc<RefCell<Wire>>) -> Self {
        let url = Wasm::url("https://github.com/extism/plugins/releases/latest/download/count_vowels.wasm");
        let manifest = Manifest::new([url]);
        let mut plugin = Plugin::new(&manifest, [], true).unwrap();

        Self {
            name: name.to_string(),
            plugin,
            wire,
        }
    }

    fn call_wasm(&mut self, input: &str) {
        let res = self.plugin.call::<&str, &str>("count_vowels", "Hello, world!").unwrap();
        println!("{}", res);
    }
}

impl MyComponent for CompC {
    fn send(&self, msg: &str) {
        let mut w = self.wire.borrow_mut();
        w.b_to_a.push_back(msg.to_string());
        println!("{} [WASM plugin] sent: {}", self.name, msg);
    }

    fn receive(&self) {
        let mut w = self.wire.borrow_mut();
        if let Some(m) = w.a_to_b.pop_front() {
            println!("{} [WASM plugin] received from A: {}", self.name, m);
        }
    }
}

// Wrapper that can contain either CompA or CompB
enum ComponentWrapper {
    A(CompA),
    B(CompB),
    C(CompC)
}

// Redirect send and receive methods calls to appropriate components' imlementations
impl MyComponent for ComponentWrapper {
    fn send(&self, msg: &str) {
        match self {
            ComponentWrapper::A(comp) => comp.send(msg),
            ComponentWrapper::B(comp) => comp.send(msg),
            ComponentWrapper::C(comp) => comp.send(msg),
        }
    }

    fn receive(&self) {
        match self {
            ComponentWrapper::A(comp) => comp.receive(),
            ComponentWrapper::B(comp) => comp.receive(),
            ComponentWrapper::C(comp) => comp.receive(),
        }
    }
}

fn main() {
    // Create shared wire
    let shared_wire_ab = Rc::new(RefCell::new(Wire::new())); // Rc to share same instance of Wire between CompA and CompB
    // Might need to switch to Arc for multithread

    // Create wrapped components
    let comp_a = ComponentWrapper::A(CompA {
        name: "CompA".to_string(),
        wire: shared_wire_ab.clone(),
    });
    let comp_b = ComponentWrapper::B(CompB {
        name: "CompB".to_string(),
        wire: shared_wire_ab.clone(),
    });

    // Test sending/receiving message
    comp_a.send("Hello from A");
    comp_b.receive();

    comp_b.send("Hello from B");
    comp_a.receive();

    let shared_wire_ac = Rc::new(RefCell::new(Wire::new())); // between CompA and CompC
    let mut comp_c = CompC::new("CompC", shared_wire_ac.clone());

    comp_c.call_wasm("Hello from host");

}