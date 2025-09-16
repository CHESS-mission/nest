use std::time::SystemTime;

use api::{Log, NestApiCommand, NestApiEvent, SimulatorState, Topology};
use leptos::{logging, prelude::*, server::codee::string::FromToStringCodec};
use leptos_meta::{provide_meta_context, MetaTags, Stylesheet, Title};
use leptos_router::{
    components::{Route, Router, Routes},
    StaticSegment,
};
use leptos_use::{core::ConnectionReadyState, use_websocket_with_options, UseWebSocketOptions, UseWebSocketReturn};
use leptos_icons::Icon;
use icondata as i;

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
        "ws://localhost:3000/ws",
        UseWebSocketOptions::default().immediate(true),
    );

    // fn format_time(time_ms: u128) -> String {
    //     let total_secs = time_ms / 1000;
    //     let hours = total_secs / 3600;
    //     let minutes = (total_secs % 3600) / 60;
    //     let seconds = total_secs % 60;
    //     let millis = time_ms - total_secs * 1000;
    //
    //     format!("{:02}:{:02}:{:02}.{:03}", hours, minutes, seconds, millis)
    // }

    let (simulator_state, set_simulator_state) = signal(SimulatorState::Idle);

    let (logs, set_logs) = signal(Vec::new());
    
    Effect::new(move |_| {
        message.with(move |message| {
            if let Some(message) = message {
                logging::log!("{}", message);
                if let Ok(event) = serde_json::from_str(&message) {
                    logging::log!("A");
                    match event {
                        NestApiEvent::SimulatorState(simulator_state) => set_simulator_state.set(simulator_state),
                        NestApiEvent::Log(log) => set_logs.update(|logs| logs.push(log)),
                    }
                } else {
                    logging::log!("B");
                }
            }
        })
    });

    let simulator_start = {
        let send = send.clone();
        move || send(&serde_json::to_string(&NestApiCommand::Start).unwrap())
    };

    let simulator_stop = {
        let send = send.clone();
        move || send(&serde_json::to_string(&NestApiCommand::Stop).unwrap())
    };

    provide_context(simulator_state);

    view! {
        <div>
            // Header
            <Header
                ws_ready_state=ready_state
                ws_connect=open
                ws_disconnect=close
                start=simulator_start
                stop=simulator_stop
            />
            // Logs
            <div class="px-8">
                <div class="mb-4 text-neutral-950 text-3xl font-light">
                    "Logs"
                </div>
                <For
                    each=move || logs.get().into_iter().enumerate()
                    key=|(index, _)| *index
                    let:((_, log))
                >
                    {move || {
                        let Log { source, level, timestamp, message } = log.clone();
                        view! {
                            <div class="mt-2 p-4 shadow-md border border-neutral-200 rounded-lg flex flex-col font-medium">
                                <div class="flex flex-row font-mono text-xs">
                                    <div class="px-3 py-1 rounded-full bg-green-200">
                                        // {format_time(timestamp)}
                                    </div>
                                    <div class="ml-2 px-3 py-1 rounded-full bg-indigo-200">
                                        {format!("{:?}", level)}
                                    </div>
                                    <div class="ml-2 px-3 py-1 rounded-full bg-orange-200">
                                        {source}
                                    </div>
                                </div>
                                <div class="mt-2 px-1 font-mono text-sm">
                                    {message}
                                </div>
                            </div>
                        }
                    }}
                </For>
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
) -> impl IntoView {
    let connected = move || ws_ready_state.get() == ConnectionReadyState::Open;

    let simulator_state = use_context::<ReadSignal<SimulatorState>>().expect("to have found the simulator state provided");

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

    view! {
        <div class="px-2 py-1 flex">
            <div class="ml-1 flex items-center text-neutral-950 text-5xl font-extralight">
                "NEST"
            </div>
            <div class="flex flex-col grow">
                <div class="flex justify-end">
                    <ConnectionWidget ws_ready_state=ws_ready_state ws_connect=ws_connect ws_disconnect=ws_disconnect/>
                </div>
                <div class="mt-1 flex justify-end">
                    <div class="flex items-center">
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
fn ConnectionWidget(ws_ready_state: Signal<ConnectionReadyState>, ws_connect: impl Fn() + 'static, ws_disconnect: impl Fn() + 'static) -> impl IntoView {
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
            <span class="ml-1 text-neutral-700 text-xs">
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
                let base = "mr-1 group-disabled:text-neutral-300".to_owned();
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
            class="group flex items-center text-neutral-950 bg-white border border-neutral-300 cursor-pointer
                hover:bg-neutral-100 rounded-md font-medium text-sm px-6 h-8 disabled:bg-neutral-50
                disabled:text-neutral-300 disabled:cursor-default"
        >
            {icon_view}
            {label}
        </button>
    }
}
