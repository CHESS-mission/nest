use std::{net::SocketAddr, sync::Arc, time::{Instant}};

use nest_api::{ErrorUpdate, EventUpdate, LogUpdate, NestApiAction, NestApiUpdate, StatusUpdate};
use axum::{extract::{ws::{Message, WebSocket}, FromRef, State, WebSocketUpgrade}, response::Response, routing::any, Router};
use futures_util::{sink::SinkExt, stream::StreamExt};
use leptos::prelude::*;
use leptos_axum::{generate_route_list, LeptosRoutes};
use nest_webapp::*;
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::{component::{ComponentEvent, LogEntry}, config::Config, parser::specs::TopologySpec, simulation::SimulationReceiver, simulator::{Simulator, SimulatorError}};

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(thiserror::Error, Debug)]
pub enum WsError {
    #[error("client disconnected")]
    ClientGone,
    #[error("serialization failed: {0}")]
    SerializationFailed(#[from] serde_json::Error),
    #[error("simulator error: {0}")]
    SimulatorError(#[from] SimulatorError),
    #[error("simulation error: {0}")]
    SimulationError(anyhow::Error),
}

pub trait TryIntoWsMessage {
    fn try_into_ws_message(self) -> Result<Message, WsError>;
}

impl TryIntoWsMessage for NestApiUpdate {
    fn try_into_ws_message(self) -> Result<Message, WsError> {
        let json = serde_json::to_string(&self)?;
        Ok(Message::Text(json.into()))
    }
}

#[derive(FromRef, Clone)]
struct NestServerState {
    simulator: Arc<Mutex<Simulator>>,
    next_conn_id: Arc<AtomicU64>,
    update_tx: broadcast::Sender<NestApiUpdate>,
    update_broadcaster: Arc<Mutex<Option<UpdateBroadcaster>>>,
    leptos_options: LeptosOptions,
}

impl NestServerState {
    fn next_conn_id(&self) -> u64 {
        self.next_conn_id.fetch_add(1, Ordering::Relaxed)
    }
}

struct UpdateBroadcaster {
    cancel: CancellationToken,
    log_join: tokio::task::JoinHandle<()>,
    event_join: tokio::task::JoinHandle<()>,
}

impl UpdateBroadcaster {
    fn start(
        mut sim_log_receiver: SimulationReceiver<LogEntry>,
        mut sim_event_receiver: SimulationReceiver<ComponentEvent>,
        sim_started_at: Instant,
        tx: broadcast::Sender<NestApiUpdate>,
    ) -> Self {
        let cancel = CancellationToken::new();
        let log_cancel = cancel.clone();
        let event_cancel = cancel.clone();

        let log_tx = tx.clone();
        let event_tx = tx;

        let log_join = tokio::task::spawn_blocking(move || {
            while !log_cancel.is_cancelled() {
                match sim_log_receiver.recv() {
                    Some((source, log)) => {
                        log_tx.send(NestApiUpdate::Log(LogUpdate {
                            timestamp: log.timestamp
                                .duration_since(sim_started_at)
                                .as_secs_f64(),
                            level: log.level,
                            component: source,
                            message: log.message
                        })).ok();
                    }
                    None => break, // receiver ended naturally
                }
            }
        });

        let event_join = tokio::task::spawn_blocking(move || {
            while !event_cancel.is_cancelled() {
                match sim_event_receiver.recv() {
                    Some((source, ComponentEvent::SimulationEvent { timestamp, event, value })) => {
                        let event_update = EventUpdate {
                            timestamp: timestamp
                                .duration_since(sim_started_at)
                                .as_secs_f64(),
                            component: source,
                            event: event,
                            data: value,
                        };
                        tracing::debug!("broadcasting event: {:?}", event_update);
                        event_tx.send(NestApiUpdate::Event(event_update)).ok();
                    }
                    Some((source, ComponentEvent::Error { timestamp, message })) => {
                        tracing::error!(component = %source, "Component error ({}): {}", source, message);
                        event_tx.send(NestApiUpdate::Error(ErrorUpdate {
                            timestamp: timestamp
                                .duration_since(sim_started_at)
                                .as_secs_f64(),
                            component: source,
                            message
                        })).ok();
                    }
                    None => break,
                }
            }
        });

        Self { cancel, log_join, event_join }
    }

    async fn stop(self) {
        self.cancel.cancel();
        self.log_join.await.ok();
        self.event_join.await.ok();
    }
}

pub struct NestServer {
    addr: SocketAddr,
    start_immediately: bool,
    simulator: Simulator,
}

impl NestServer {
    pub fn new(addr: SocketAddr, start_immediately: bool, simulator: Simulator) -> Self {
        Self { addr, start_immediately, simulator }
    }

    pub fn from_config(config: Config) -> anyhow::Result<Self> {
        let mut simulator = Simulator::new();

        if let Some(path) = config.topology {
            let topology = TopologySpec::from_yaml_file(&path)?;
            simulator.load_topology(topology)?;
        }

        Ok(Self {
            addr: config.listen,
            start_immediately: config.start,
            simulator
        })
    }

    pub async fn serve(self) -> anyhow::Result<()> {
        let conf = get_configuration(None)?;
        let leptos_options = conf.leptos_options;

        // Channels
        let (update_tx, _) = broadcast::channel::<NestApiUpdate>(256);

        let state = NestServerState {
            simulator: Arc::new(Mutex::new(self.simulator)),
            next_conn_id: Arc::new(AtomicU64::new(1)),
            update_tx,
            update_broadcaster: Arc::new(Mutex::new(None)),
            leptos_options,
        };

        if self.start_immediately {
            start_simulation(&state).await?;
        }

        // Leptos routes
        let routes = generate_route_list(App);

        // Create a shutdown signal channel
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

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
            .with_state(state.clone());

        tracing::info!(addr = %self.addr, "listening on http://{}", self.addr);
        let listener = tokio::net::TcpListener::bind(&self.addr).await?;
        
        let server = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            });

        // Wait for either server to finish or shutdown signal
        tokio::select! {
            result = server => {
                result?;
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Received Ctrl-C signal");
                let _ = shutdown_tx.send(());
            }
        }

        // Perform graceful shutdown of components
        if let Ok(mut broadcaster_guard) = state.update_broadcaster.try_lock() {
            if let Some(broadcaster) = broadcaster_guard.take() {
                tracing::info!("Stopping update broadcaster");
                broadcaster.stop().await;
            }
        }

        if let Ok(mut sim_guard) = state.simulator.try_lock() {
            tracing::info!("Stopping simulation");
            let _ = sim_guard.stop();
        }

        tracing::info!("Server shutdown complete");
        Ok(())
    }
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<NestServerState>) -> Response {
    ws.on_upgrade(|socket| websocket_task(socket, state))
}

async fn websocket_task(socket: WebSocket, state: NestServerState) {
    // Create tracing span with unique connection id
    let conn_id = state.next_conn_id();
    let span = tracing::info_span!("ws_connection", conn_id);

    async move {
        tracing::debug!("connection opened");

        // Handle the connection
        let result = handle_websocket(socket, state).await;

        match result {
            Ok(()) => tracing::debug!("connection closed"),
            Err(WsError::ClientGone) => tracing::debug!("client disconnected"),
            Err(err) => tracing::error!(%err, "websocket error"),
        }
    }
    .instrument(span)
    .await;
}

async fn handle_websocket(socket: WebSocket, state: NestServerState) -> Result<(), WsError> {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (out_tx, mut out_rx) = mpsc::channel::<Message>(256);

    // Spawn WebSocket writer task
    tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if ws_sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Subscribe to update stream
    let mut update_rx = state.update_tx.subscribe();

    // Send initial status immediately (watch holds current value)
    let initial_status = state.simulator.lock().await.status();
    out_tx.send(
        NestApiUpdate::Status(initial_status).try_into_ws_message()?
    ).await.map_err(|_| WsError::ClientGone)?;

    // Main loop
    loop {
        tokio::select! {
            // Simulation update (forward to client)
            sim_update = update_rx.recv() => {
                match sim_update {
                    Ok(update) => out_tx.send(update.try_into_ws_message()?).await
                        .map_err(|_| WsError::ClientGone)?,

                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(dropped)) => {
                        tracing::warn!("dropped {dropped} simulation updates (can't keep up)")
                    }
                }
            }

            // Client request (action)
            request = ws_receiver.next() => {
                let Some(Ok(msg)) = request else { break; };

                match msg {
                    Message::Text(text) => {
                        tracing::debug!(json = %text, "received client request");
                        match serde_json::from_str::<NestApiAction>(&text) {
                            Ok(action) => handle_client_action(action, &state).await?,
                            Err(e) => tracing::warn!("received invalid JSON from client (ignoring): {}", e),
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        }
    }

    // Close connection
    Ok(())
}

/// Handle one client action.
/// Returns a user-facing error string on failure (so the WS loop can send it).
async fn handle_client_action(
    action: NestApiAction,
    state: &NestServerState,
) -> Result<(), WsError> {

    match action {
        NestApiAction::Start => {
            let new_status = start_simulation(state).await?;

            // Broadcast new status
            state.update_tx.send(
                NestApiUpdate::Status(new_status)
            ).map_err(|_| WsError::ClientGone)?;
        }

        NestApiAction::Stop => {
            let new_status = {
                let mut sim = state.simulator.lock().await;

                // Stop simulation
                tracing::info!("stopping simulation");
                sim.stop().inspect_err(|e| {
                    tracing::warn!("couldn't stop simulation: {}", e);
                }).ok();

                sim.status()
            };

            // Stop update broadcaster
            if let Some(broadcaster) = state.update_broadcaster.lock().await.take() {
                broadcaster.stop().await;
            }

            // Broadcast new status
            state.update_tx.send(
                NestApiUpdate::Status(new_status)
            ).map_err(|_| WsError::ClientGone)?;
        }

        NestApiAction::UploadTopology(upload) => {
            // Parse yaml into TopologySpec
            match TopologySpec::from_yaml(&upload.topology_yaml) {
                Ok(spec) => {
                    let new_status = {
                        let mut sim = state.simulator.lock().await;

                        // Load new topology
                        tracing::info!("loading topology");
                        sim.load_topology(spec).inspect_err(|e| {
                            tracing::warn!("couldn't load topology: {}", e);
                        }).ok();

                        sim.status()
                    };

                    // Broadcast new status
                    state.update_tx.send(
                        NestApiUpdate::Status(new_status)
                    ).map_err(|_| WsError::ClientGone)?;
                }
                Err(e) => tracing::warn!("couldn't parse topology: {}", e),
            }
        }

        NestApiAction::Command(cmd) => {
            let sim = state.simulator.lock().await;

            match sim.component_handle(&cmd.component) {
                Ok(handle) => handle
                    .simulator_command(cmd.command, cmd.arguments)
                    .map_err(WsError::SimulationError)?,
                Err(e) => tracing::warn!("couldn't send command to component: {}", e),
            }
        }

        NestApiAction::SetEnv(set_env) => {
            let res = {
                let sim = state.simulator.lock().await;
                sim.env_var(&set_env.variable)
                    .inspect_err(|e| tracing::warn!("couldn't set environment variable: {}", e))
                    .cloned()
            };

            if let Ok(any_var) = res {
                any_var.set_from_api(set_env).inspect_err(|e|
                    tracing::warn!("couldn't set environment variable: {}", e)
                ).ok();
            }
        }
    }

    Ok(())
}

async fn start_simulation(state: &NestServerState) -> Result<StatusUpdate, WsError> {
    let (log_receiver, event_receiver, new_status, sim_started_at) = {
        let mut sim = state.simulator.lock().await;

        // Start simulation
        tracing::info!("starting simulation");
        sim.start().inspect_err(|e| {
            tracing::warn!("couldn't start simulation: {}", e);
        }).ok();

        // Get log & event receivers
        (sim.log_receiver()?, sim.event_receiver()?, sim.status(), sim.sim_started_at()?)
    };

    // Start a new update broadcaster
    let broadcaster = UpdateBroadcaster::start(
        log_receiver,
        event_receiver,
        sim_started_at,
        state.update_tx.clone()
    );
    *state.update_broadcaster.lock().await = Some(broadcaster);

    Ok(new_status)
}

