use std::{net::SocketAddr, sync::{Arc}};

use api::{LogUpdate, NestApiAction, NestApiUpdate, ReadyStatusUpdate, RunningStatusUpdate, StatusUpdate};
use axum::{extract::{ws::{Message, WebSocket}, FromRef, State, WebSocketUpgrade}, response::Response, routing::any, Router};
use futures_util::{sink::SinkExt, stream::StreamExt};
use leptos::prelude::*;
use leptos_axum::{generate_route_list, LeptosRoutes};
use app::*;
use tokio::sync::Mutex;

use crate::{component::LogEntry, simulation::SimulationReceiver, simulator::{Simulator, SimulatorState}};

#[derive(FromRef, Clone)]
struct NestServerState {
    pub simulator: Arc<Mutex<Simulator>>,
    pub log_sender: tokio::sync::broadcast::Sender<(String, LogEntry)>,
    pub leptos_options: LeptosOptions,
}

impl NestServerState {
    pub async fn into_status_update(&self) -> StatusUpdate {
        let sim = self.simulator.lock().await;
        match sim.state() {
            SimulatorState::Idle => StatusUpdate::Idle,
            SimulatorState::Ready => StatusUpdate::Ready(
                ReadyStatusUpdate {
                    topology: sim.topology().unwrap().clone().into()
                }
            ),
            SimulatorState::Running => StatusUpdate::Running(
                RunningStatusUpdate {
                    topology: sim.topology().unwrap().clone().into(),
                    started_at: sim.started_time().unwrap()
                }
            ),
        }
    }
}

struct Broadcaster {
    join_handle: tokio::task::JoinHandle<()>,
}

impl Broadcaster {
    fn start<T: Send + Sync + 'static>(mut sim_receiver: SimulationReceiver<T>, sender: tokio::sync::broadcast::Sender<(String, T)>) -> Self {
        let join_handle = tokio::task::spawn_blocking(move || {
            while let Some(msg) = sim_receiver.recv() {
                let _ = sender.send(msg);
            }
        });

        Self { join_handle }
    }

    async fn stop(self) {
        let _ = self.join_handle.await;
    }
}

pub struct NestServer {
    addr: SocketAddr,
    simulator: Simulator,
}

impl NestServer {
    pub fn new(addr: SocketAddr, simulator: Simulator) -> Self {
        Self { addr, simulator }
    }

    pub async fn serve(self) {
        let conf = get_configuration(None).unwrap();
        let leptos_options = conf.leptos_options;

        let (log_sender, _) = tokio::sync::broadcast::channel(1000);

        let state = NestServerState {
            simulator: Arc::new(Mutex::new(self.simulator)),
            log_sender,
            leptos_options,
        };

        // Generate the list of routes in your Leptos App
        let routes = generate_route_list(App);

        let app = Router::new()
            .route("/ws", any(ws_handler))
            .leptos_routes_with_context(&state, routes,
                {
                    let state = state.clone();
                    move || provide_context(state.clone())
                },
                {
                    let state = state.clone();
                    move || shell(state.leptos_options.clone())
                },
            )
            .fallback(leptos_axum::file_and_error_handler::<NestServerState, _>(shell))
            .with_state(state);

        println!("listening on http://{}", &self.addr);
        let listener = tokio::net::TcpListener::bind(&self.addr).await.unwrap();
        axum::serve(listener, app).await.unwrap();
    }
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<NestServerState>) -> Response {
    ws.on_upgrade(|socket| handle_websocket(socket, state))
}

async fn handle_websocket(socket: WebSocket, state: NestServerState) {
    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Send initial state
    let api_state = serde_json::to_string(&NestApiUpdate::Status(state.into_status_update().await)).unwrap();
    let _ = ws_sender.send(Message::Text(api_state.into())).await;

    let ws_sender = Arc::new(Mutex::new(ws_sender));

    // Spawn log forwarding task
    let mut log_receiver = state.log_sender.subscribe();
    tokio::spawn({
        let ws_sender = ws_sender.clone();
        async move {
            while let Ok((source, log_entry)) = log_receiver.recv().await {
                let log = serde_json::to_string(
                    &NestApiUpdate::Log(LogUpdate {
                        timestamp: log_entry.timestamp,
                        level: log_entry.level,
                        component: source,
                        message: log_entry.message,
                    })
                ).unwrap();
                let _ = ws_sender.lock().await.send(Message::Text(log.into())).await;
            }
        }
    });

    while let Some(msg) = ws_receiver.next().await {
        if let Ok(msg) = msg {
            if let Message::Text(msg) = msg {
                let action: NestApiAction= serde_json::from_str(&msg).unwrap();
                match action {
                    NestApiAction::Start => {
                        {
                            let mut sim = state.simulator.lock().await;
                            // Start simulation
                            sim.start().unwrap();
                            // Start log broadcaster
                            Broadcaster::start(sim.log_receiver().unwrap(), state.log_sender.clone());
                        }
                        // Send new state
                        let api_state = serde_json::to_string(&NestApiUpdate::Status(state.into_status_update().await)).unwrap();
                        let _ = ws_sender.lock().await.send(Message::Text(api_state.into())).await;
                    },
                    NestApiAction::Stop => {
                        {
                            let mut sim = state.simulator.lock().await;
                            // Stop simulation
                            sim.stop().unwrap();
                        }
                        // Send new state
                        let api_state = serde_json::to_string(&NestApiUpdate::Status(state.into_status_update().await)).unwrap();
                        let _ = ws_sender.lock().await.send(Message::Text(api_state.into())).await;
                        // TODO: Does broadcaster stop on its own?
                    },
                    NestApiAction::UploadTopology(upload_topology_action) => todo!(),
                    NestApiAction::Command(command_action) => todo!(),
                    NestApiAction::SetEnv(set_env_action) => todo!(),
                }
            }
        } else {
            // client disconnected
            return;
        };
    }
}
