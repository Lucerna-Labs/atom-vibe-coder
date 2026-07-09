use crate::model::{
    MainTab, NativeApp, SettingsTab, APPLY_PROVIDER, APP_SCROLL, ARTIFACT_SCROLL,
    BUILD_DESIGN_UPLOAD, BUS_SCROLL, CAPTURE_PROOF, DESIGN_CSS_PATH_INPUT, DESIGN_HTML_PATH_INPUT,
    DESIGN_UPLOAD_TAB, EVIDENCE_SCROLL, EXEC_PROVIDER, INTENT_INPUT, LEFT_SCROLL, MARK_DRIFT,
    PROVIDER_AUTH_HEADER_INPUT, PROVIDER_AUTH_SCHEME_INPUT, PROVIDER_BODY_TEMPLATE_INPUT,
    PROVIDER_CONNECTIONS_TAB, PROVIDER_FORMAT_INPUT, PROVIDER_KEY_ENV_INPUT, PROVIDER_KIND_INPUT,
    PROVIDER_MODEL_INPUT, PROVIDER_RESPONSE_KEY_INPUT, PROVIDER_URL_INPUT, RUNTIME_SETTINGS_TAB,
    RUN_LOOP, SETTINGS_SCROLL, SETTINGS_TAB, WORKSPACE_TAB,
};
use math_atoms_core::{gates, mission, recipes, RuntimeStatus};
use pmre_kit::{
    ux::{Align, Dim, Edges, Justify, Style, UxNode},
    Rgba,
};
use pmre_orchestrator::UiState;

const BG: Rgba = Rgba::new(0.055, 0.058, 0.06, 1.0);

fn surface() -> Rgba {
    Rgba::rgb8(245, 247, 246)
}
fn panel() -> Rgba {
    Rgba::rgb8(255, 255, 252)
}
fn ink() -> Rgba {
    Rgba::rgb8(16, 24, 24)
}
fn muted() -> Rgba {
    Rgba::rgb8(90, 103, 102)
}
fn line() -> Rgba {
    Rgba::rgb8(205, 214, 211)
}
fn teal() -> Rgba {
    Rgba::rgb8(0, 132, 142)
}
fn amber() -> Rgba {
    Rgba::rgb8(207, 145, 0)
}
fn red() -> Rgba {
    Rgba::rgb8(210, 82, 55)
}
fn blue() -> Rgba {
    Rgba::rgb8(70, 88, 168)
}

pub fn build(app: &NativeApp, ui: &UiState) -> UxNode {
    UxNode::boxed(
        Style::col()
            .w(Dim::Flex(1.0))
            .h(Dim::Flex(1.0))
            .pad(Edges::all(18.0))
            .gap(14.0)
            .bg(surface())
            .scroll(APP_SCROLL),
        vec![
            header(app),
            top_tabs(app, ui),
            match app.active_main_tab {
                MainTab::Workspace => workspace_body(ui, app),
                MainTab::Settings => settings_body(app, ui),
            },
        ],
    )
}

fn top_tabs(app: &NativeApp, ui: &UiState) -> UxNode {
    UxNode::boxed(
        Style::row()
            .h(Dim::Px(44.0))
            .gap(8.0)
            .pad(Edges::all(4.0))
            .radius(8.0)
            .bg(panel())
            .border(1.0, line()),
        vec![
            tab_button(
                ui,
                WORKSPACE_TAB,
                "Workspace",
                app.active_main_tab == MainTab::Workspace,
            ),
            tab_button(
                ui,
                SETTINGS_TAB,
                "Settings",
                app.active_main_tab == MainTab::Settings,
            ),
        ],
    )
}

fn workspace_body(ui: &UiState, app: &NativeApp) -> UxNode {
    UxNode::boxed(
        Style::row().gap(14.0).h(Dim::Px(690.0)),
        vec![left_panel(ui), center_panel(app), right_panel(app)],
    )
}

fn header(app: &NativeApp) -> UxNode {
    UxNode::boxed(
        Style::row()
            .h(Dim::Px(74.0))
            .align(Align::Center)
            .justify(Justify::SpaceBetween)
            .pad(Edges::xy(18.0, 0.0))
            .radius(8.0)
            .bg(panel())
            .border(1.0, line()),
        vec![
            UxNode::boxed(
                Style::col().gap(4.0),
                vec![
                    UxNode::text("Math Atoms Coder", 25.0, ink()),
                    UxNode::text(mission().body, 12.0, muted()),
                ],
            ),
            UxNode::boxed(
                Style::row().gap(8.0),
                vec![
                    metric("status", app.status().as_str(), status_color(app.status())),
                    metric(
                        "proofs",
                        app.runtime.state().proof_count.to_string(),
                        teal(),
                    ),
                    metric("drift", app.runtime.state().drift_count.to_string(), red()),
                ],
            ),
        ],
    )
}

fn left_panel(ui: &UiState) -> UxNode {
    UxNode::boxed(
        card_style().w(Dim::Pct(28.0)).gap(12.0).scroll(LEFT_SCROLL),
        vec![
            label("Intent"),
            input_box(ui),
            UxNode::boxed(
                Style::row().gap(8.0).h(Dim::Px(42.0)),
                vec![
                    button(ui, RUN_LOOP, "Run", teal(), Rgba::rgb8(255, 255, 255)),
                    button(
                        ui,
                        EXEC_PROVIDER,
                        "Provider",
                        blue(),
                        Rgba::rgb8(255, 255, 255),
                    ),
                    button(
                        ui,
                        CAPTURE_PROOF,
                        "Capture",
                        amber(),
                        Rgba::rgb8(255, 255, 255),
                    ),
                    button(ui, MARK_DRIFT, "Drift", red(), Rgba::rgb8(255, 255, 255)),
                ],
            ),
            label("Gates"),
            UxNode::boxed(
                Style::col().gap(7.0),
                gates()
                    .iter()
                    .map(|gate| {
                        mini_card(
                            gate.title,
                            &format!("{} / {}", gate.layer, gate.body),
                            if gate.title == "Ornith parity" {
                                amber()
                            } else {
                                teal()
                            },
                        )
                    })
                    .collect(),
            ),
        ],
    )
}

fn settings_body(app: &NativeApp, ui: &UiState) -> UxNode {
    UxNode::boxed(
        Style::row().gap(14.0).h(Dim::Px(760.0)),
        vec![settings_nav(app, ui), settings_panel(app, ui)],
    )
}

fn settings_nav(app: &NativeApp, ui: &UiState) -> UxNode {
    UxNode::boxed(
        card_style().w(Dim::Pct(24.0)).gap(10.0),
        vec![
            label("Settings"),
            tab_button(
                ui,
                PROVIDER_CONNECTIONS_TAB,
                "Provider Connections",
                app.active_settings_tab == SettingsTab::ProviderConnections,
            ),
            tab_button(
                ui,
                DESIGN_UPLOAD_TAB,
                "Design Upload",
                app.active_settings_tab == SettingsTab::DesignUpload,
            ),
            tab_button(
                ui,
                RUNTIME_SETTINGS_TAB,
                "Runtime",
                app.active_settings_tab == SettingsTab::Runtime,
            ),
            label("Connection Status"),
            mini_card(
                if app.runtime.provider().is_ready() {
                    "provider ready"
                } else {
                    "provider blocked"
                },
                &format!(
                    "{} {} via {}",
                    app.runtime.provider().kind.as_str(),
                    app.runtime.provider().model,
                    app.runtime.provider().api_key_env
                ),
                if app.runtime.provider().is_ready() {
                    teal()
                } else {
                    red()
                },
            ),
        ],
    )
}

fn settings_panel(app: &NativeApp, ui: &UiState) -> UxNode {
    UxNode::boxed(
        card_style()
            .w(Dim::Pct(76.0))
            .gap(12.0)
            .scroll(SETTINGS_SCROLL),
        match app.active_settings_tab {
            SettingsTab::ProviderConnections => provider_connections_panel(app, ui),
            SettingsTab::DesignUpload => design_upload_panel(app, ui),
            SettingsTab::Runtime => runtime_settings_panel(app),
        },
    )
}

fn provider_connections_panel(app: &NativeApp, ui: &UiState) -> Vec<UxNode> {
    vec![
        label("Provider Connections"),
        mini_card(
            if app.runtime.provider().is_ready() {
                "configured"
            } else {
                "blocked"
            },
            &format!(
                "{} / {} {} via {}",
                app.runtime.provider().kind.as_str(),
                app.runtime.provider().wire_format.as_str(),
                app.runtime.provider().model,
                app.runtime.provider().api_key_env
            ),
            if app.runtime.provider().is_ready() {
                teal()
            } else {
                red()
            },
        ),
        UxNode::boxed(
            Style::row().gap(8.0).h(Dim::Px(42.0)),
            vec![button(
                ui,
                APPLY_PROVIDER,
                "Apply Provider",
                teal(),
                Rgba::rgb8(255, 255, 255),
            )],
        ),
        UxNode::boxed(
            Style::row().gap(8.0),
            vec![
                mini_card(
                    "route",
                    "proof-loop -> provider-adapter -> model-worker",
                    blue(),
                ),
                mini_card(
                    "return",
                    "model-worker -> proof-loop -> artifact-state",
                    teal(),
                ),
            ],
        ),
        provider_input(ui, PROVIDER_KIND_INPUT, "kind"),
        provider_input(ui, PROVIDER_FORMAT_INPUT, "format"),
        provider_input(ui, PROVIDER_MODEL_INPUT, "model"),
        provider_input(ui, PROVIDER_URL_INPUT, "endpoint"),
        provider_input(ui, PROVIDER_KEY_ENV_INPUT, "key env"),
        provider_input(ui, PROVIDER_AUTH_HEADER_INPUT, "auth header"),
        provider_input(ui, PROVIDER_AUTH_SCHEME_INPUT, "auth scheme"),
        provider_input(ui, PROVIDER_RESPONSE_KEY_INPUT, "response key"),
        provider_input(ui, PROVIDER_BODY_TEMPLATE_INPUT, "body template"),
    ]
}

fn design_upload_panel(app: &NativeApp, ui: &UiState) -> Vec<UxNode> {
    vec![
        label("Design Upload"),
        mini_card(
            if app.design_build_running {
                "building"
            } else if app.last_design_output.starts_with("Design upload blocked:") {
                "blocked"
            } else if app
                .last_design_output
                .starts_with("design upload build ok:")
            {
                "compiled"
            } else {
                "ready"
            },
            &app.last_design_output,
            if app.design_build_running {
                amber()
            } else if app.last_design_output.starts_with("Design upload blocked:") {
                red()
            } else {
                teal()
            },
        ),
        UxNode::boxed(
            Style::row().gap(8.0).h(Dim::Px(42.0)),
            vec![button(
                ui,
                BUILD_DESIGN_UPLOAD,
                "Build Design",
                teal(),
                Rgba::rgb8(255, 255, 255),
            )],
        ),
        UxNode::boxed(
            Style::row().gap(8.0),
            vec![
                mini_card("upload", "html/css files -> design-upload gate", blue()),
                mini_card(
                    "render",
                    "render_html -> PMRE artifact -> side window",
                    teal(),
                ),
            ],
        ),
        provider_input(ui, DESIGN_HTML_PATH_INPUT, "html path"),
        provider_input(ui, DESIGN_CSS_PATH_INPUT, "css path"),
    ]
}

fn runtime_settings_panel(app: &NativeApp) -> Vec<UxNode> {
    vec![
        label("Runtime"),
        UxNode::boxed(
            Style::row().gap(8.0),
            vec![
                metric("status", app.status().as_str(), status_color(app.status())),
                metric(
                    "route",
                    app.runtime.state().last_route.len().to_string(),
                    blue(),
                ),
                metric(
                    "evidence",
                    app.runtime.state().evidence.len().to_string(),
                    amber(),
                ),
            ],
        ),
        mini_card(
            "selected recipe",
            &app.runtime.state().selected_recipe,
            status_color(app.status()),
        ),
        mini_card(
            "proof store",
            &app.last_run_summary,
            if app.status() == RuntimeStatus::Blocked {
                red()
            } else {
                teal()
            },
        ),
        label("Blockers"),
        blocker_list(app),
    ]
}

fn center_panel(app: &NativeApp) -> UxNode {
    let state = app.runtime.state();
    let recipe = recipes()
        .iter()
        .find(|recipe| recipe.id == state.selected_recipe)
        .unwrap_or(&recipes()[0]);
    let mut bus_rows: Vec<UxNode> = app
        .runtime
        .bus()
        .envelopes()
        .iter()
        .rev()
        .take(16)
        .map(|env| {
            mini_card(
                env.layer.label(),
                &format!(
                    "T{} {:?} {} -> {}",
                    env.thread_id, env.kind, env.source, env.target
                ),
                match env.layer {
                    math_atoms_core::BusLayer::L0Transport => teal(),
                    math_atoms_core::BusLayer::L1Message => red(),
                    math_atoms_core::BusLayer::L2Flow => amber(),
                    math_atoms_core::BusLayer::L3Orchestration => blue(),
                },
            )
        })
        .collect();
    if bus_rows.is_empty() {
        bus_rows.push(mini_card(
            "No bus route",
            "Run the loop to emit L0-L3 envelopes.",
            muted(),
        ));
    }

    UxNode::boxed(
        card_style().w(Dim::Pct(42.0)).gap(12.0),
        vec![
            label("Spiderweb Bus"),
            UxNode::boxed(
                Style::row().gap(10.0),
                vec![
                    layer_pill("L0", teal()),
                    layer_pill("L1", red()),
                    layer_pill("L2", amber()),
                    layer_pill("L3", blue()),
                ],
            ),
            label("Fabric"),
            UxNode::boxed(
                Style::row().gap(8.0),
                vec![
                    metric(
                        "threads",
                        app.runtime.bus().threads().len().to_string(),
                        teal(),
                    ),
                    metric(
                        "intersections",
                        app.runtime.bus().intersections().len().to_string(),
                        amber(),
                    ),
                    metric(
                        "preloads",
                        app.runtime.bus().preloads().len().to_string(),
                        blue(),
                    ),
                    metric(
                        "pressure",
                        app.runtime.bus().backpressure().len().to_string(),
                        red(),
                    ),
                ],
            ),
            UxNode::boxed(
                Style::col()
                    .scroll(BUS_SCROLL)
                    .h(Dim::Px(230.0))
                    .gap(7.0)
                    .pad(Edges::all(8.0))
                    .radius(7.0)
                    .bg(Rgba::rgb8(239, 244, 242))
                    .border(1.0, line()),
                bus_rows,
            ),
            label("Selected Recipe"),
            UxNode::boxed(
                Style::col()
                    .gap(5.0)
                    .pad(Edges::all(12.0))
                    .radius(7.0)
                    .bg(Rgba::rgb8(248, 250, 247))
                    .border(1.0, line()),
                vec![
                    UxNode::text(recipe.name, 16.0, ink()),
                    UxNode::text(recipe.summary, 12.0, muted()),
                    UxNode::text(
                        format!("atoms: {}", state.selected_atoms.join(", ")),
                        12.0,
                        ink(),
                    ),
                    UxNode::text(&app.last_run_summary, 12.0, status_color(state.status)),
                ],
            ),
            label("Blockers"),
            blocker_list(app),
        ],
    )
}

fn right_panel(app: &NativeApp) -> UxNode {
    let mut evidence_rows: Vec<UxNode> = app
        .runtime
        .state()
        .evidence
        .iter()
        .map(|item| mini_card(&item.title, &item.excerpt, amber()))
        .collect();
    if evidence_rows.is_empty() {
        evidence_rows.push(mini_card(
            "No evidence",
            "Graph retrieval has not run in this session.",
            muted(),
        ));
    }
    UxNode::boxed(
        card_style().w(Dim::Pct(30.0)).gap(12.0),
        vec![
            label("Side Artifact Window"),
            artifact_window(app),
            label("Wiki Graph RAG"),
            UxNode::boxed(
                Style::col()
                    .scroll(EVIDENCE_SCROLL)
                    .h(Dim::Px(170.0))
                    .gap(7.0)
                    .pad(Edges::all(8.0))
                    .radius(7.0)
                    .bg(Rgba::rgb8(240, 244, 247))
                    .border(1.0, line()),
                evidence_rows,
            ),
            label("Provider Output"),
            UxNode::boxed(
                Style::col()
                    .gap(6.0)
                    .h(Dim::Flex(1.0))
                    .pad(Edges::all(12.0))
                    .radius(7.0)
                    .bg(Rgba::rgb8(249, 249, 246))
                    .border(1.0, line()),
                vec![
                    UxNode::text(
                        if app.provider_running {
                            "running"
                        } else {
                            "ready"
                        },
                        13.0,
                        if app.provider_running {
                            amber()
                        } else {
                            teal()
                        },
                    ),
                    UxNode::text(&app.last_provider_output, 12.0, ink()),
                ],
            ),
        ],
    )
}

fn artifact_window(app: &NativeApp) -> UxNode {
    let mut rows: Vec<UxNode> = app
        .side_artifacts
        .iter()
        .map(|artifact| {
            let artifact_ref = if artifact.artifact_path.trim().is_empty() {
                &artifact.source_path
            } else {
                &artifact.artifact_path
            };
            mini_card(
                &format!("{} / {}", artifact.name, artifact.status),
                &format!("{} | {}", artifact.output, artifact_ref),
                teal(),
            )
        })
        .collect();
    if rows.is_empty() {
        rows.push(mini_card(
            "No app artifacts",
            "Run the provider multi-app build gate to populate this side window.",
            muted(),
        ));
    }
    UxNode::boxed(
        Style::col()
            .scroll(ARTIFACT_SCROLL)
            .h(Dim::Px(185.0))
            .gap(7.0)
            .pad(Edges::all(8.0))
            .radius(7.0)
            .bg(Rgba::rgb8(238, 246, 244))
            .border(1.0, line()),
        rows,
    )
}

fn input_box(ui: &UiState) -> UxNode {
    let text = ui.input_text(INTENT_INPUT);
    UxNode::boxed(
        Style::col()
            .input(INTENT_INPUT)
            .h(Dim::Px(112.0))
            .pad(Edges::all(12.0))
            .radius(7.0)
            .bg(Rgba::rgb8(250, 252, 250))
            .border(
                1.5,
                if ui.is_focused(INTENT_INPUT) {
                    teal()
                } else {
                    line()
                },
            ),
        vec![UxNode::text(text, 13.0, ink())],
    )
}

fn provider_input(ui: &UiState, id: u32, label_text: &str) -> UxNode {
    UxNode::boxed(
        Style::col().gap(3.0),
        vec![
            UxNode::text(label_text.to_ascii_uppercase(), 9.0, muted()),
            UxNode::boxed(
                Style::col()
                    .input(id)
                    .h(Dim::Px(34.0))
                    .pad(Edges::xy(8.0, 7.0))
                    .radius(6.0)
                    .bg(Rgba::rgb8(250, 252, 250))
                    .border(1.2, if ui.is_focused(id) { teal() } else { line() }),
                vec![UxNode::text(ui.input_text(id), 11.0, ink())],
            ),
        ],
    )
}

fn blocker_list(app: &NativeApp) -> UxNode {
    let blockers = &app.runtime.state().blockers;
    if blockers.is_empty() {
        mini_card("clear", "No blockers on the last run.", teal())
    } else {
        UxNode::boxed(
            Style::col().gap(6.0),
            blockers
                .iter()
                .map(|item| mini_card("blocked", item, red()))
                .collect(),
        )
    }
}

fn card_style() -> Style {
    Style::col()
        .h(Dim::Flex(1.0))
        .pad(Edges::all(14.0))
        .radius(8.0)
        .bg(panel())
        .border(1.0, line())
        .shadow(0.0, 2.0, 8.0, Rgba::new(0.0, 0.0, 0.0, 0.10))
}

fn label(text: &str) -> UxNode {
    UxNode::text(text.to_ascii_uppercase(), 11.0, muted())
}

fn button(ui: &UiState, id: u32, label: &str, bg: Rgba, fg: Rgba) -> UxNode {
    let active = ui.is_pressed(id) || ui.is_hover(id);
    UxNode::boxed(
        Style::row()
            .button(id)
            .w(Dim::Flex(1.0))
            .h(Dim::Px(40.0))
            .align(Align::Center)
            .justify(Justify::Center)
            .radius(7.0)
            .bg(if active { bg.with_alpha(0.86) } else { bg })
            .border(1.0, bg),
        vec![UxNode::text(label, 13.0, fg)],
    )
}

fn tab_button(ui: &UiState, id: u32, label: &str, selected: bool) -> UxNode {
    let bg = if selected {
        teal()
    } else if ui.is_hover(id) || ui.is_pressed(id) {
        Rgba::rgb8(230, 237, 234)
    } else {
        Rgba::rgb8(249, 250, 247)
    };
    let fg = if selected {
        Rgba::rgb8(255, 255, 255)
    } else {
        ink()
    };
    UxNode::boxed(
        Style::row()
            .button(id)
            .w(Dim::Flex(1.0))
            .h(Dim::Px(36.0))
            .align(Align::Center)
            .justify(Justify::Center)
            .radius(7.0)
            .bg(bg)
            .border(1.0, if selected { teal() } else { line() }),
        vec![UxNode::text(label, 13.0, fg)],
    )
}

fn mini_card(title: &str, body: &str, accent: Rgba) -> UxNode {
    UxNode::boxed(
        Style::row()
            .gap(9.0)
            .pad(Edges::all(9.0))
            .radius(7.0)
            .bg(Rgba::rgb8(253, 253, 249))
            .border(1.0, line()),
        vec![
            UxNode::boxed(
                Style::row()
                    .w(Dim::Px(8.0))
                    .h(Dim::Px(34.0))
                    .radius(4.0)
                    .bg(accent),
                vec![],
            ),
            UxNode::boxed(
                Style::col().gap(3.0).w(Dim::Flex(1.0)),
                vec![
                    UxNode::text(title, 13.0, ink()),
                    UxNode::text(body, 11.0, muted()),
                ],
            ),
        ],
    )
}

fn metric(label: &str, value: impl Into<String>, color: Rgba) -> UxNode {
    UxNode::boxed(
        Style::col()
            .w(Dim::Px(94.0))
            .h(Dim::Px(48.0))
            .align(Align::Center)
            .justify(Justify::Center)
            .radius(7.0)
            .bg(Rgba::rgb8(249, 250, 247))
            .border(1.0, line()),
        vec![
            UxNode::text(value.into(), 17.0, color),
            UxNode::text(label, 9.0, muted()),
        ],
    )
}

fn layer_pill(label: &str, color: Rgba) -> UxNode {
    UxNode::boxed(
        Style::row()
            .w(Dim::Px(58.0))
            .h(Dim::Px(28.0))
            .align(Align::Center)
            .justify(Justify::Center)
            .radius(14.0)
            .bg(color),
        vec![UxNode::text(label, 12.0, Rgba::rgb8(255, 255, 255))],
    )
}

fn status_color(status: RuntimeStatus) -> Rgba {
    match status {
        RuntimeStatus::Draft => amber(),
        RuntimeStatus::ProviderPending => amber(),
        RuntimeStatus::Proven => teal(),
        RuntimeStatus::Blocked => red(),
        RuntimeStatus::DriftFlagged => blue(),
    }
}

pub fn background() -> Rgba {
    BG
}
