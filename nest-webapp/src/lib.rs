use nest_api::{LogLevelDTO, LogUpdate, NestApiAction, NestApiUpdate, StatusUpdate, TopologyDTO, UploadTopologyAction};
use leptos::{html::{Div, Input}, prelude::*, server::codee::string::FromToStringCodec};
use leptos_meta::{provide_meta_context, Link, MetaTags, Stylesheet, Title};
use leptos_router::{
    components::{Route, Router, Routes},
    StaticSegment,
};
use leptos_use::{core::ConnectionReadyState, use_websocket_with_options, UseWebSocketOptions, UseWebSocketReturn};
use leptos_icons::Icon;
use icondata as i;
use web_sys::{Event, File, FileReader, wasm_bindgen::{closure::Closure, JsCast}};

#[derive(Clone, Debug)]
pub enum SimulatorState {
    Idle,
    Ready {
        topology: TopologyDTO
    },
    Running {
        topology: TopologyDTO,
    },
}

impl SimulatorState {
    pub fn is_idle(&self) -> bool {
        matches!(self, SimulatorState::Idle)
    }

    pub fn is_ready(&self) -> bool {
        matches!(self, SimulatorState::Ready {..})
    }
    
    pub fn is_running(&self) -> bool {
        matches!(self, SimulatorState::Running {..})
    }
}

impl From<StatusUpdate> for SimulatorState {
    fn from(value: StatusUpdate) -> Self {
        match value {
            StatusUpdate::Idle => SimulatorState::Idle,
            StatusUpdate::Ready(update) => SimulatorState::Ready {
                topology: update.topology
            },
            StatusUpdate::Running(update) => SimulatorState::Running {
                topology: update.topology,
            },
        }
    }
}

pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <AutoReload options=options.clone()/>
                <HydrationScripts options/>
                <MetaTags/>
            </head>
            <body>
                <App/>
            </body>
        </html>
    }
}

#[component]
pub fn App() -> impl IntoView {
    // Provides context that manages stylesheets, titles, meta tags, etc.
    provide_meta_context();

    view! {
        <Stylesheet id="leptos" href="/pkg/nest.css"/>

        // sets the icon
        <Link rel="icon" href="/assets/NEST-logo.svg" type_="image/svg+xml"/>

        // sets the document title
        <Title text="NEST"/>

        // content for this welcome page
        <Router>
            <main>
                <Routes fallback=|| "Page not found.".into_view()>
                    <Route path=StaticSegment("") view=HomePage/>
                </Routes>
            </main>
        </Router>
    }
}

#[cfg(not(feature = "ssr"))]
fn make_ws_url(path: &str) -> String {
    window()
        .location()
        .host()
        .map(|host| {
            let protocol = window()
                .location()
                .protocol()
                .unwrap_or_else(|_| "http:".to_string());
            
            let ws_protocol = if protocol == "https:" { "wss" } else { "ws" };
            format!("{}://{}{}", ws_protocol, host, path)
        })
        .unwrap_or_else(|_| format!("ws://localhost:3000{}", path))
}

#[cfg(feature = "ssr")]
pub fn make_ws_url(path: &str) -> String {
    // Fallback when running on the server (no browser window)
    format!("ws://localhost:3000{}", path)
}

#[component]
fn HomePage() -> impl IntoView {
    let UseWebSocketReturn {
        ready_state,
        message,
        send,
        open,
        close,
        ..
    } = use_websocket_with_options::<String, String, FromToStringCodec, _, _>(
        &make_ws_url("/ws"),
        UseWebSocketOptions::default().immediate(true),
    );

    let (simulator_state, set_simulator_state) = signal(SimulatorState::Idle);

    let logs = RwSignal::new(Vec::new());
    
    Effect::new(move |_| {
        message.with(move |message| {
            if let Some(message) = message {
                if let Ok(event) = serde_json::from_str(&message) {
                    match event {
                        NestApiUpdate::Status(update)=>set_simulator_state.set(update.into()),
                        NestApiUpdate::Event(_event_update) => (), // TODO
                        NestApiUpdate::Log(log)=>logs.update(|logs|logs.push(log)),
                        NestApiUpdate::Error(_error_update) => (), // TODO
                    }
                }
            }
        })
    });

    let load_topology = {
        let send = send.clone();

        move |file: File| {
            let reader = FileReader::new().expect("create FileReader");

            // Clone what we need inside the async callback
            let send = send.clone();

            let onload = {
                let reader = reader.clone();
                Closure::once(move |_: web_sys::ProgressEvent| {
                    let content = reader
                        .result()
                        .expect("file reader result")
                        .as_string()
                        .expect("file content to be string");

                    let msg = serde_json::to_string(
                        &NestApiAction::UploadTopology(
                            UploadTopologyAction {
                                topology_yaml: content,
                            }
                        )
                    ).unwrap();

                    send(&msg);
                })
            };

            reader.set_onload(Some(onload.as_ref().unchecked_ref()));
            reader.read_as_text(&file).expect("read file");

            // Prevent the closure from being dropped
            onload.forget();
        }
    };
    
    let simulator_start = {
        let send = send.clone();
        move || send(&serde_json::to_string(&NestApiAction::Start).unwrap())
    };

    let simulator_stop = {
        let send = send.clone();
        move || send(&serde_json::to_string(&NestApiAction::Stop).unwrap())
    };

    provide_context(simulator_state);

    view! {
        <div class="h-screen w-screen overflow-hidden bg-gray-200 flex flex-col">
            // Header
            <Header
                ws_ready_state=ready_state
                ws_connect=open
                ws_disconnect=close
                start=simulator_start
                stop=simulator_stop
                load_topology=load_topology
            />
            <div class="grow p-4 flex overflow-hidden">
                // Side Panel
                <SidePanel
                    logs=logs
                    style:width="100%"
                />
            </div>
        </div>
    }
}

#[component]
fn Header(
    ws_ready_state: Signal<ConnectionReadyState>,
    ws_connect: impl Fn() + 'static,
    ws_disconnect: impl Fn() + 'static,
    start: impl Fn() + 'static,
    stop: impl Fn() + 'static,
    load_topology: impl Fn(File) + 'static,
) -> impl IntoView {
    let connected = move || ws_ready_state.get() == ConnectionReadyState::Open;

    let simulator_state =
        use_context::<ReadSignal<SimulatorState>>()
            .expect("to have found the simulator state provided");

    let start_stop_label = move || match *simulator_state.read() {
        SimulatorState::Running { .. } => "stop",
        _ => "start",
    };

    let start_stop_icon = move || match *simulator_state.read() {
        SimulatorState::Running { .. } => i::TbPlayerStopOutline,
        _ => i::TbPlayerPlayOutline,
    };

    let start_stop_color = move || match *simulator_state.read() {
        SimulatorState::Running { .. } => "text-red-500",
        _ => "text-lime-500",
    };

    let start_stop_toggle = move || match *simulator_state.read() {
        SimulatorState::Idle => (),
        SimulatorState::Ready { .. } => start(),
        SimulatorState::Running { .. } => stop(),
    };

    let file_input = NodeRef::<Input>::new();

    let on_select_topology_file = move |ev: Event| {
        ev.prevent_default();

        if let Some(files) = file_input.get().unwrap().files() {
            if let Some(file) = files.get(0) {
                load_topology(file);
            }
        }
    };

    view! {
        <div class="px-2 pt-1 pb-2 flex bg-white border border-gray-300/50 shadow-md">
            <div class="ml-1 flex items-center text-gray-950 text-5xl font-extralight">
                <img src="/assets/NEST-logo.png" alt="NEST logo" class="h-[1em] w-auto mr-2 align-middle" />
                "NEST"
            </div>

            <div class="flex flex-col grow">
                <div class="flex justify-end">
                    <ConnectionWidget
                        ws_ready_state=ws_ready_state
                        ws_connect=ws_connect
                        ws_disconnect=ws_disconnect
                    />
                </div>

                <div class="mt-1 flex justify-end">
                    <div class="flex items-center gap-2">
                        <input
                            type="file"
                            class="hidden"
                            node_ref=file_input
                            on:change=on_select_topology_file
                        />

                        <Button
                            label="load topology"
                            icon=i::TbLinkOutline // TODO
                            on:click=move |_| {
                                if let Some(input) = file_input.get() {
                                    input.click();
                                }
                            }
                            attr:disabled=move || !connected() || simulator_state.with(|s| s.is_running())
                        />

                        <Button
                            label=Signal::derive(start_stop_label)
                            icon=Signal::derive(start_stop_icon)
                            icon_color=Signal::derive(start_stop_color)
                            on:click=move |_| start_stop_toggle()
                            attr:disabled=move || !connected() || simulator_state.with(|s| s.is_idle())
                        />
                    </div>
                </div>
            </div>
        </div>
    }
}

#[component]
fn SidePanel(
    #[prop(into)]
    logs: RwSignal<Vec<LogUpdate>>,
) -> impl IntoView {
    #[derive(Clone, PartialEq)]
    enum Tab {
        Logs,
        Events,
    }

    let seg_button_items = vec![
        SegButtonItem {
            label: "logs".to_owned(),
            value: Tab::Logs,
        },
        SegButtonItem {
            label: "events".to_owned(),
            value: Tab::Events,
        },
    ];

    let selected_tab = RwSignal::new(Tab::Logs);

    let simulator_state = use_context::<ReadSignal<SimulatorState>>().expect("to have found the simulator state provided");

    Effect::new(move || {
        simulator_state.with(move |sim_state| {
            if let SimulatorState::Running { .. } = sim_state {
                logs.update(|logs| logs.clear());
            }
        })
    });

    let console_ref: NodeRef<Div> = NodeRef::new();
    let (auto_scroll, set_auto_scroll) = signal(true);

    let on_scroll = {
        let console_ref = console_ref.clone();
        move |_| {
            if let Some(div) = console_ref.get() {
                let at_bottom = div.scroll_top() + div.client_height() >= div.scroll_height() - 5;
                set_auto_scroll.set(at_bottom);
            }
        }
    };

    // Auto-scroll only if user is at the bottom
    Effect::new(move || {
        logs.with(|_| {
            if auto_scroll.get() {
                if let Some(div) = console_ref.get() {
                    div.set_scroll_top(div.scroll_height());
                }
            }
        });
    });

    let format_timestamp = |timestamp: f64| {
        let total_secs = timestamp.floor() as i64;

        let hours = total_secs / 3600;
        let minutes = (total_secs % 3600) / 60;
        let seconds = total_secs % 60;
        let millis = ((timestamp - total_secs as f64) * 1000.0).round() as i64;

        format!("{:02}:{:02}:{:02}.{:03}", hours, minutes, seconds, millis)
    };


    view! {
        <div class="p-3 flex flex-col bg-white rounded-xl border border-gray-300/50 shadow-md">
            <div class="flex justify-center">
                <SegButton
                    items=seg_button_items
                    selected=selected_tab
                />
            </div>
            <div
                node_ref=console_ref
                class="mt-3 p-3 overflow-y-auto h-full bg-gray-900 rounded-lg"
                on:scroll=on_scroll
            >
                <For
                    each=move || logs.get().into_iter().enumerate()
                    key=|(index, _)| *index
                    let:((_index, log))
                >
                    {move || {
                        let log = log.clone();
                        view! {
                            <div class="text-sm text-gray-400 font-mono">
                                {format_timestamp(log.timestamp)}
                                ": ["
                                <span class="text-gray-500 italic">
                                    {log.component}
                                </span>
                                "] ["
                                <span
                                    class:text-blue-500=move || log.level == LogLevelDTO::Info
                                    class:text-yellow-500=move || log.level == LogLevelDTO::Warning
                                    class:text-red-500=move || log.level == LogLevelDTO::Error
                                >
                                    {format!("{:?}", log.level)}
                                </span>
                                "] "
                                <span
                                    class:text-gray-50=move || log.level == LogLevelDTO::Info
                                    class:text-yellow-500=move || log.level == LogLevelDTO::Warning
                                    class:text-red-500=move || log.level == LogLevelDTO::Error
                                >
                                    {log.message}
                                </span>
                            </div>
                        }
                    }}
                </For>
            </div>
        </div>
    }
}

#[component]
fn ConnectionWidget(
    ws_ready_state: Signal<ConnectionReadyState>,
    ws_connect: impl Fn() + 'static,
    ws_disconnect: impl Fn() + 'static
) -> impl IntoView {
    let status = move || match ws_ready_state.get() {
        ConnectionReadyState::Connecting => "connecting...",
        ConnectionReadyState::Open => "connected",
        ConnectionReadyState::Closing => "disconnecting...",
        ConnectionReadyState::Closed => "disconnected",
    };

    let connected = move || ws_ready_state.get() == ConnectionReadyState::Open;
    
    let toggle_connection = move |_| {
        if connected() {
            ws_disconnect();
        } else {
            ws_connect();
        }
    };

    let (hovering, set_hovering) = signal(false);

    view! {
        <div class="py-1 px-2 flex items-center cursor-pointer rounded-md font-medium hover:font-bold"
            class=("bg-red-50", move || !connected())
            class=("bg-lime-50", connected)
            on:pointerenter={move |_| set_hovering.set(true)}
            on:pointerleave={move |_| set_hovering.set(false)}
            on:click=toggle_connection
        >
            <span
                class=("text-red-500", move || !connected())
                class=("text-lime-500", connected)
            >
                {move || match connected() {
                    true => view! { <Icon icon={i::TbLinkOutline}/> },
                    false => view! { <Icon icon={i::TbLinkOffOutline}/> },
                }}
            </span>
            <span class="ml-1 text-gray-700 text-xs">
                {move || if hovering.get() {
                    if connected() { "disconnect" } else { "connect" }
                } else {
                    status()
                }}
            </span>
        </div>
    }
}

#[component]
fn Button(
    #[prop(into)]
    label: Signal<String>,
    #[prop(into, optional)]
    icon: Option<Signal<icondata_core::Icon>>,
    #[prop(into, optional)]
    icon_color: Option<Signal<String>>,
) -> impl IntoView {
    let icon_view = move || icon.map(|icon| view!{
        <div
            class=move || {
                let base = "mr-1 group-disabled:text-gray-300".to_owned();
                match icon_color {
                    Some(color) => format!("{} {}", base, color.get()),
                    None => base,
                }
            }
        >
            <Icon icon={icon} width="1.2em" height="1.2em"/>
        </div>
    });

    view! {
        <button
            class="group flex items-center text-gray-950 bg-white border border-gray-300 cursor-pointer
                hover:bg-gray-100 rounded-md font-medium text-sm px-6 h-8 disabled:bg-gray-50
                disabled:text-gray-300 disabled:cursor-default"
        >
            {icon_view}
            {label}
        </button>
    }
}

struct SegButtonItem<T: Clone + PartialEq + Send + Sync + 'static> {
    label: String,
    value: T,
}

#[component]
fn SegButton<T: Clone + PartialEq + Send + Sync + 'static>(
    items: Vec<SegButtonItem<T>>,
    selected: RwSignal<T>,
) -> impl IntoView {
    view! {
        <div class="py-1.5 px-1 group flex w-fit rounded-lg bg-gray-200 inset-shadow-sm">
            {items.into_iter().map(|SegButtonItem { label, value }| view! {
                <button
                    class="mx-0.5 flex items-center text-gray-950 cursor-pointer
                        rounded-md font-medium text-sm px-6 h-8 group-disabled:bg-gray-50
                        group-disabled:text-gray-300 group-disabled:cursor-default"
                    class=(["bg-white", "shadow-sm"], {
                        let value = value.clone();
                        move || selected.with(|s| *s == value)
                    })
                    on:click={
                        let value = value.clone();
                        move |_| selected.set(value.clone())
                    }
                >
                    {label}
                </button>
            }).collect_view()}
        </div>
    }
}
