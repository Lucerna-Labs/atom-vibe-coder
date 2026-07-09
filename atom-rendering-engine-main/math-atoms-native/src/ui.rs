use crate::model::{
    MainTab, NativeApp, APPLY_PROVIDER, APP_SCROLL, ARTIFACT_SCROLL, BUILD_DESIGN_UPLOAD,
    BUS_SCROLL, CAPTURE_PROOF, DESIGN_CSS_PATH_INPUT, DESIGN_HTML_PATH_INPUT, DESIGN_UPLOAD_TAB,
    EVIDENCE_SCROLL, EXEC_PROVIDER, HOOKS_TAB, INTENT_INPUT, MARK_DRIFT, MCP_TAB,
    PROVIDER_AUTH_HEADER_INPUT, PROVIDER_AUTH_SCHEME_INPUT, PROVIDER_BODY_TEMPLATE_INPUT,
    PROVIDER_CONNECTIONS_TAB, PROVIDER_FORMAT_INPUT, PROVIDER_KEY_ENV_INPUT, PROVIDER_KIND_INPUT,
    PROVIDER_MODEL_INPUT, PROVIDER_RESPONSE_KEY_INPUT, PROVIDER_URL_INPUT, RUNTIME_SETTINGS_TAB,
    RUN_LOOP, SETTINGS_SCROLL, SETTINGS_TAB, SKILLS_TAB, WIKI_TAB, WORKSPACE_TAB,
};
use math_atoms_core::{gates, mission, recipes, RuntimeStatus};
use pmre_kit::{
    ux::{Align, Dim, Edges, Justify, Style, UxNode},
    Rgba,
};
use pmre_orchestrator::UiState;

const BG: Rgba = Rgba::new(0.028, 0.035, 0.045, 1.0);

fn bg() -> Rgba {
    Rgba::rgb8(8, 11, 15)
}
fn sidebar_bg() -> Rgba {
    Rgba::rgb8(13, 16, 21)
}
fn glass() -> Rgba {
    Rgba::new(0.075, 0.086, 0.102, 0.94)
}
fn glass_deep() -> Rgba {
    Rgba::new(0.045, 0.055, 0.068, 0.96)
}
fn glass_lift() -> Rgba {
    Rgba::new(0.105, 0.116, 0.132, 0.92)
}
fn line() -> Rgba {
    Rgba::new(0.62, 0.72, 0.72, 0.18)
}
fn line_hot() -> Rgba {
    Rgba::new(0.96, 0.74, 0.32, 0.72)
}
fn ink() -> Rgba {
    Rgba::rgb8(238, 242, 237)
}
fn muted() -> Rgba {
    Rgba::rgb8(146, 158, 158)
}
fn dim() -> Rgba {
    Rgba::rgb8(94, 104, 108)
}
fn lamp() -> Rgba {
    Rgba::rgb8(236, 185, 70)
}
fn lamp_soft() -> Rgba {
    Rgba::rgb8(186, 139, 45)
}
fn teal() -> Rgba {
    Rgba::rgb8(120, 235, 225)
}
fn teal_dim() -> Rgba {
    Rgba::rgb8(38, 150, 146)
}
fn red() -> Rgba {
    Rgba::rgb8(214, 86, 68)
}
fn blue() -> Rgba {
    Rgba::rgb8(110, 126, 205)
}

pub fn build(app: &NativeApp, ui: &UiState) -> UxNode {
    UxNode::boxed(
        Style::col()
            .w(Dim::Flex(1.0))
            .h(Dim::Flex(1.0))
            .bg(bg())
            .scroll(APP_SCROLL),
        vec![UxNode::boxed(
            Style::row().w(Dim::Flex(1.0)).h(Dim::Px(920.0)).bg(bg()),
            vec![sidebar(app, ui), main_frame(app, ui)],
        )],
    )
}

fn sidebar(app: &NativeApp, ui: &UiState) -> UxNode {
    UxNode::boxed(
        Style::col()
            .w(Dim::Px(218.0))
            .h(Dim::Px(920.0))
            .pad(Edges::xy(14.0, 14.0))
            .gap(18.0)
            .bg(sidebar_bg())
            .border(1.0, line()),
        vec![
            brand_block(),
            nav_section(
                "PRIMARY",
                vec![
                    nav_button(
                        ui,
                        WORKSPACE_TAB,
                        "[]",
                        "Assistant",
                        app.active_main_tab == MainTab::Workspace,
                    ),
                    nav_button(
                        ui,
                        PROVIDER_CONNECTIONS_TAB,
                        "<>",
                        "Provider",
                        app.active_main_tab == MainTab::Provider,
                    ),
                    nav_button(
                        ui,
                        WIKI_TAB,
                        "#",
                        "Wiki Graph",
                        app.active_main_tab == MainTab::Wiki,
                    ),
                ],
            ),
            nav_section(
                "AGENT",
                vec![
                    nav_button(ui, MCP_TAB, "M", "MCP", app.active_main_tab == MainTab::Mcp),
                    nav_button(
                        ui,
                        SKILLS_TAB,
                        "*",
                        "Skills",
                        app.active_main_tab == MainTab::Skills,
                    ),
                    nav_button(
                        ui,
                        HOOKS_TAB,
                        "!",
                        "Hooks",
                        app.active_main_tab == MainTab::Hooks,
                    ),
                ],
            ),
            nav_section(
                "SYSTEM",
                vec![
                    nav_button(
                        ui,
                        SETTINGS_TAB,
                        "~",
                        "Settings",
                        app.active_main_tab == MainTab::Settings,
                    ),
                    nav_button(
                        ui,
                        RUNTIME_SETTINGS_TAB,
                        "R",
                        "Runtime",
                        app.active_main_tab == MainTab::Settings,
                    ),
                    nav_button(
                        ui,
                        DESIGN_UPLOAD_TAB,
                        "+",
                        "Design Upload",
                        app.active_main_tab == MainTab::DesignUpload,
                    ),
                ],
            ),
            UxNode::boxed(Style::col().h(Dim::Flex(1.0)), vec![]),
            mini_health(app),
        ],
    )
}

fn brand_block() -> UxNode {
    UxNode::boxed(
        Style::row()
            .h(Dim::Px(70.0))
            .gap(10.0)
            .align(Align::Center)
            .pad(Edges::xy(8.0, 0.0))
            .border(1.0, Rgba::new(1.0, 1.0, 1.0, 0.04)),
        vec![
            UxNode::boxed(
                Style::row()
                    .w(Dim::Px(36.0))
                    .h(Dim::Px(36.0))
                    .align(Align::Center)
                    .justify(Justify::Center)
                    .radius(8.0)
                    .bg(Rgba::new(0.0, 0.0, 0.0, 0.24))
                    .border(1.0, line_hot()),
                vec![UxNode::text("A", 18.0, lamp())],
            ),
            UxNode::boxed(
                Style::col().gap(2.0),
                vec![
                    UxNode::text("Atom", 19.0, ink()),
                    UxNode::text("VIBE CODER . LUCERNA LABS", 9.0, muted()),
                ],
            ),
        ],
    )
}

fn nav_section(label: &str, rows: Vec<UxNode>) -> UxNode {
    let mut children = vec![UxNode::text(label, 10.0, dim())];
    children.extend(rows);
    UxNode::boxed(Style::col().gap(7.0), children)
}

fn nav_button(ui: &UiState, id: u32, icon: &str, label: &str, selected: bool) -> UxNode {
    let active = ui.is_hover(id) || ui.is_pressed(id);
    let bg_col = if selected {
        Rgba::new(0.86, 0.66, 0.22, 0.13)
    } else if active {
        Rgba::new(1.0, 1.0, 1.0, 0.055)
    } else {
        Rgba::new(0.0, 0.0, 0.0, 0.0)
    };
    let border_col = if selected {
        line_hot()
    } else {
        Rgba::new(0.0, 0.0, 0.0, 0.0)
    };
    UxNode::boxed(
        Style::row()
            .button(id)
            .h(Dim::Px(39.0))
            .align(Align::Center)
            .gap(10.0)
            .pad(Edges::xy(10.0, 0.0))
            .radius(7.0)
            .bg(bg_col)
            .border(1.0, border_col),
        vec![
            UxNode::text(icon, 13.0, if selected { lamp() } else { muted() }),
            UxNode::text(label, 14.0, if selected { ink() } else { muted() }),
        ],
    )
}

fn main_frame(app: &NativeApp, ui: &UiState) -> UxNode {
    UxNode::boxed(
        Style::col()
            .w(Dim::Flex(1.0))
            .h(Dim::Px(920.0))
            .pad(Edges::all(20.0))
            .gap(14.0)
            .bg(Rgba::new(0.0, 0.0, 0.0, 0.10)),
        vec![
            top_bar(app),
            match app.active_main_tab {
                MainTab::Workspace => assistant_dashboard(app, ui),
                MainTab::Provider => full_provider(app, ui),
                MainTab::Wiki => full_wiki(app),
                MainTab::Mcp => full_mcp(app),
                MainTab::Skills => full_skills(app),
                MainTab::Hooks => full_hooks(app),
                MainTab::Settings => full_settings(app),
                MainTab::DesignUpload => full_design_upload(app, ui),
            },
        ],
    )
}

fn top_bar(app: &NativeApp) -> UxNode {
    UxNode::boxed(
        Style::row()
            .h(Dim::Px(76.0))
            .align(Align::Center)
            .justify(Justify::SpaceBetween)
            .pad(Edges::xy(18.0, 0.0))
            .radius(8.0)
            .bg(glass())
            .border(1.0, line())
            .shadow(0.0, 8.0, 28.0, Rgba::new(0.0, 0.0, 0.0, 0.25)),
        vec![
            UxNode::boxed(
                Style::col().gap(4.0),
                vec![
                    UxNode::text("Atom Vibe Coder", 24.0, ink()),
                    UxNode::text(
                        "Native atom renderer . Spiderweb bus . Lucerna glass",
                        11.0,
                        muted(),
                    ),
                ],
            ),
            UxNode::boxed(
                Style::row().gap(9.0).align(Align::Center),
                vec![
                    top_dot("GATEWAY", app.status() != RuntimeStatus::Blocked),
                    top_dot("BUS", app.runtime.bus().contains_all_layers()),
                    top_dot("WIKI", !app.runtime.state().evidence.is_empty()),
                    top_dot("MCP", true),
                    top_dot("HOOKS", app.runtime.state().blockers.is_empty()),
                    status_chip(app.status().as_str(), status_color(app.status())),
                ],
            ),
        ],
    )
}

fn assistant_dashboard(app: &NativeApp, ui: &UiState) -> UxNode {
    UxNode::boxed(
        Style::row().gap(14.0).h(Dim::Px(790.0)),
        vec![chat_surface(app, ui), preview_stack(app)],
    )
}

fn chat_surface(app: &NativeApp, ui: &UiState) -> UxNode {
    UxNode::boxed(
        pane_style().w(Dim::Pct(65.0)).gap(14.0),
        vec![
            UxNode::boxed(
                Style::row()
                    .align(Align::Center)
                    .justify(Justify::SpaceBetween),
                vec![
                    UxNode::boxed(
                        Style::col().gap(4.0),
                        vec![
                            section_label("ASSISTANT"),
                            UxNode::text("Build brief", 20.0, ink()),
                            UxNode::text(mission().body, 11.0, muted()),
                        ],
                    ),
                    UxNode::boxed(
                        Style::row().gap(8.0),
                        vec![
                            compact_metric(
                                "atoms",
                                app.runtime.state().selected_atoms.len().to_string(),
                                teal(),
                            ),
                            compact_metric(
                                "proofs",
                                app.runtime.state().proof_count.to_string(),
                                lamp(),
                            ),
                            compact_metric(
                                "route",
                                app.runtime.state().last_route.len().to_string(),
                                blue(),
                            ),
                        ],
                    ),
                ],
            ),
            transcript_panel(app),
            chat_box(ui),
        ],
    )
}

fn transcript_panel(app: &NativeApp) -> UxNode {
    let recipe = selected_recipe(app);
    UxNode::boxed(
        Style::col()
            .h(Dim::Flex(1.0))
            .gap(10.0)
            .pad(Edges::all(14.0))
            .radius(8.0)
            .bg(glass_deep())
            .border(1.0, line()),
        vec![
            mini_card(
                "selected recipe",
                &format!("{} | {}", recipe.name, recipe.summary),
                lamp(),
            ),
            mini_card(
                "runtime summary",
                &app.last_run_summary,
                status_color(app.status()),
            ),
            mini_card(
                "atom stack",
                &app.runtime.state().selected_atoms.join(" -> "),
                teal(),
            ),
            blocker_list(app),
        ],
    )
}

fn chat_box(ui: &UiState) -> UxNode {
    UxNode::boxed(
        Style::col()
            .h(Dim::Px(188.0))
            .gap(10.0)
            .pad(Edges::all(14.0))
            .radius(8.0)
            .bg(glass_lift())
            .border(
                1.0,
                if ui.is_focused(INTENT_INPUT) {
                    line_hot()
                } else {
                    line()
                },
            ),
        vec![
            input_box(ui),
            UxNode::boxed(
                Style::row().gap(8.0).h(Dim::Px(40.0)).align(Align::Center),
                vec![
                    button(ui, RUN_LOOP, "Run", lamp(), Rgba::rgb8(15, 18, 22)),
                    button(ui, EXEC_PROVIDER, "Provider", teal_dim(), ink()),
                    button(ui, CAPTURE_PROOF, "Capture", glass_deep(), ink()),
                    button(ui, MARK_DRIFT, "Drift", red(), ink()),
                ],
            ),
        ],
    )
}

fn preview_stack(app: &NativeApp) -> UxNode {
    UxNode::boxed(
        Style::col().w(Dim::Pct(35.0)).gap(12.0),
        vec![
            preview_pane("SIDE ARTIFACTS", artifact_window(app), ARTIFACT_SCROLL),
            preview_pane("WIKI GRAPH RAG", evidence_preview(app), EVIDENCE_SCROLL),
            preview_pane("SPIDERWEB BUS", bus_preview(app), BUS_SCROLL),
            preview_pane("PROVIDER OUTPUT", provider_output_preview(app), 0),
        ],
    )
}

fn preview_pane(title: &str, child: UxNode, _scroll_id: u32) -> UxNode {
    UxNode::boxed(
        Style::col()
            .h(Dim::Flex(1.0))
            .gap(8.0)
            .pad(Edges::all(12.0))
            .radius(8.0)
            .bg(glass())
            .border(1.0, line()),
        vec![section_label(title), child],
    )
}

fn full_provider(app: &NativeApp, ui: &UiState) -> UxNode {
    full_page(
        "Provider Connections",
        "Configure any model provider without leaving the native Atom renderer.",
        provider_connections_panel(app, ui),
    )
}

fn full_wiki(app: &NativeApp) -> UxNode {
    let mut rows = wiki_rows(app);
    rows.insert(
        0,
        UxNode::boxed(
            Style::row().gap(10.0),
            vec![
                compact_metric(
                    "evidence",
                    app.runtime.state().evidence.len().to_string(),
                    lamp(),
                ),
                compact_metric("recipes", recipes().len().to_string(), teal()),
                compact_metric("gates", gates().len().to_string(), blue()),
            ],
        ),
    );
    full_page(
        "Wiki Graph RAG",
        "Graph evidence is pulled into the proof loop before provider execution.",
        rows,
    )
}

fn full_mcp(app: &NativeApp) -> UxNode {
    full_page(
        "MCP",
        "Native command surfaces exposed through the same atom route contracts.",
        vec![
            mini_card("provider surface", app.provider_title_state(), provider_color(app)),
            mini_card("artifact surface", &app.artifact_title_state(), teal()),
            mini_card(
                "command bus",
                "Assistant, Provider, Wiki, MCP, Skills, Hooks, Settings, and Design tabs use stable native command IDs.",
                lamp(),
            ),
            mini_card(
                "runtime contract",
                "MCP traffic must enter through recipe selection, graph evidence, provider execution, and proof capture.",
                blue(),
            ),
            bus_preview(app),
        ],
    )
}

fn full_skills(app: &NativeApp) -> UxNode {
    full_page(
        "Skills",
        "Runtime skills are presented as gate-owned capabilities, not loose prompts.",
        vec![
            mini_card(
                "native renderer",
                "PMRE renders the app shell, uploaded HTML/CSS artifacts, and generated side artifacts.",
                teal(),
            ),
            mini_card(
                "provider adapter",
                "OpenAI, Ollama Cloud, Mistral, DeepSeek, and custom wire formats route through one adapter.",
                lamp(),
            ),
            mini_card(
                "wiki graph",
                "Graph retrieval contributes evidence nodes to the current proof state.",
                blue(),
            ),
            mini_card(
                "proof capture",
                &format!("{} evidence nodes on the current run.", app.runtime.state().evidence.len()),
                status_color(app.status()),
            ),
            mini_card(
                "design rail",
                "Generated PMRE apps receive dependency-free color, type, glass, animation, and control customization.",
                teal(),
            ),
        ],
    )
}

fn full_hooks(app: &NativeApp) -> UxNode {
    let mut rows: Vec<UxNode> = gates()
        .iter()
        .map(|gate| {
            mini_card(
                gate.title,
                &format!("{} | {}", gate.layer, gate.body),
                lamp(),
            )
        })
        .collect();
    rows.push(section_label("LIVE BLOCKERS"));
    rows.push(blocker_list(app));
    full_page(
        "Hooks",
        "Gate state is visible inside the app and follows the proof route.",
        rows,
    )
}

fn full_settings(app: &NativeApp) -> UxNode {
    full_page(
        "Settings",
        "Runtime state, store health, and bus pressure for the native shell.",
        runtime_settings_panel(app),
    )
}

fn full_design_upload(app: &NativeApp, ui: &UiState) -> UxNode {
    full_page(
        "Design Upload",
        "Compile uploaded HTML/CSS into a native PMRE side artifact.",
        design_upload_panel(app, ui),
    )
}

fn full_page(title: &str, subtitle: &str, mut rows: Vec<UxNode>) -> UxNode {
    let mut children = vec![UxNode::boxed(
        Style::col().gap(5.0),
        vec![
            section_label("FULL VIEW"),
            UxNode::text(title, 22.0, ink()),
            UxNode::text(subtitle, 12.0, muted()),
        ],
    )];
    children.append(&mut rows);
    UxNode::boxed(
        pane_style()
            .w(Dim::Flex(1.0))
            .h(Dim::Px(790.0))
            .gap(12.0)
            .scroll(SETTINGS_SCROLL),
        children,
    )
}

fn provider_connections_panel(app: &NativeApp, ui: &UiState) -> Vec<UxNode> {
    vec![
        UxNode::boxed(
            Style::row().gap(10.0),
            vec![
                compact_metric(
                    "state",
                    if app.runtime.provider().is_ready() {
                        "ready"
                    } else {
                        "blocked"
                    },
                    provider_color(app),
                ),
                compact_metric(
                    app.runtime.provider().kind.as_str(),
                    app.runtime.provider().wire_format.as_str(),
                    teal(),
                ),
                compact_metric("model", app.runtime.provider().model.clone(), lamp()),
            ],
        ),
        UxNode::boxed(
            Style::row().gap(8.0).h(Dim::Px(42.0)),
            vec![button(
                ui,
                APPLY_PROVIDER,
                "Apply Provider",
                lamp(),
                Rgba::rgb8(15, 18, 22),
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
        mini_card(
            "last output",
            &app.last_provider_output,
            provider_color(app),
        ),
    ]
}

fn design_upload_panel(app: &NativeApp, ui: &UiState) -> Vec<UxNode> {
    vec![
        mini_card(
            app.design_title_state(),
            &app.last_design_output,
            design_color(app),
        ),
        UxNode::boxed(
            Style::row().gap(8.0).h(Dim::Px(42.0)),
            vec![button(
                ui,
                BUILD_DESIGN_UPLOAD,
                "Build Design",
                lamp(),
                Rgba::rgb8(15, 18, 22),
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
        UxNode::boxed(
            Style::row().gap(10.0),
            vec![
                compact_metric("status", app.status().as_str(), status_color(app.status())),
                compact_metric(
                    "route",
                    app.runtime.state().last_route.len().to_string(),
                    blue(),
                ),
                compact_metric(
                    "evidence",
                    app.runtime.state().evidence.len().to_string(),
                    lamp(),
                ),
                compact_metric("artifacts", app.side_artifacts.len().to_string(), teal()),
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
            status_color(app.status()),
        ),
        blocker_list(app),
    ]
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
            "no app artifacts",
            "Run the provider app gate or build an uploaded design to populate this window.",
            muted(),
        ));
    }
    UxNode::boxed(
        Style::col()
            .scroll(ARTIFACT_SCROLL)
            .h(Dim::Px(132.0))
            .gap(7.0),
        rows,
    )
}

fn evidence_preview(app: &NativeApp) -> UxNode {
    UxNode::boxed(
        Style::col()
            .scroll(EVIDENCE_SCROLL)
            .h(Dim::Px(126.0))
            .gap(7.0),
        wiki_rows(app),
    )
}

fn wiki_rows(app: &NativeApp) -> Vec<UxNode> {
    let mut evidence_rows: Vec<UxNode> = app
        .runtime
        .state()
        .evidence
        .iter()
        .map(|item| mini_card(&item.title, &item.excerpt, lamp()))
        .collect();
    if evidence_rows.is_empty() {
        evidence_rows.push(mini_card(
            "no evidence",
            "Run the loop to retrieve graph evidence for the current app request.",
            muted(),
        ));
    }
    evidence_rows
}

fn bus_preview(app: &NativeApp) -> UxNode {
    let mut bus_rows: Vec<UxNode> = app
        .runtime
        .bus()
        .envelopes()
        .iter()
        .rev()
        .take(10)
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
                    math_atoms_core::BusLayer::L2Flow => lamp(),
                    math_atoms_core::BusLayer::L3Orchestration => blue(),
                },
            )
        })
        .collect();
    if bus_rows.is_empty() {
        bus_rows.push(mini_card(
            "no bus route",
            "Run the loop to emit L0-L3 Spiderweb envelopes.",
            muted(),
        ));
    }
    UxNode::boxed(
        Style::col().scroll(BUS_SCROLL).h(Dim::Px(134.0)).gap(7.0),
        bus_rows,
    )
}

fn provider_output_preview(app: &NativeApp) -> UxNode {
    UxNode::boxed(
        Style::col().h(Dim::Px(110.0)).gap(7.0),
        vec![mini_card(
            if app.provider_running {
                "running"
            } else {
                app.provider_title_state()
            },
            &app.last_provider_output,
            provider_color(app),
        )],
    )
}

fn input_box(ui: &UiState) -> UxNode {
    let text = focused_input_text(ui, INTENT_INPUT);
    UxNode::boxed(
        Style::col()
            .input(INTENT_INPUT)
            .h(Dim::Px(100.0))
            .pad(Edges::all(12.0))
            .radius(7.0)
            .bg(Rgba::new(0.02, 0.025, 0.032, 0.88))
            .border(
                1.0,
                if ui.is_focused(INTENT_INPUT) {
                    line_hot()
                } else {
                    line()
                },
            ),
        vec![UxNode::text(text, 14.0, ink())],
    )
}

fn provider_input(ui: &UiState, id: u32, label_text: &str) -> UxNode {
    let text = focused_input_text(ui, id);
    UxNode::boxed(
        Style::col().gap(4.0),
        vec![
            UxNode::text(label_text.to_ascii_uppercase(), 9.0, muted()),
            UxNode::boxed(
                Style::col()
                    .input(id)
                    .h(Dim::Px(36.0))
                    .pad(Edges::xy(9.0, 8.0))
                    .radius(6.0)
                    .bg(glass_deep())
                    .border(
                        1.0,
                        if ui.is_focused(id) {
                            line_hot()
                        } else {
                            line()
                        },
                    ),
                vec![UxNode::text(text, 11.0, ink())],
            ),
        ],
    )
}

fn focused_input_text(ui: &UiState, id: u32) -> String {
    let mut text = ui.input_text(id).to_string();
    if ui.is_focused(id) && caret_visible(ui) {
        text.push('|');
    }
    text
}

fn caret_visible(ui: &UiState) -> bool {
    ((ui.animation_time * 2.0).floor() as u32).is_multiple_of(2)
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

fn mini_health(app: &NativeApp) -> UxNode {
    UxNode::boxed(
        Style::col()
            .gap(6.0)
            .pad(Edges::all(10.0))
            .radius(8.0)
            .bg(glass_deep())
            .border(1.0, line()),
        vec![
            section_label("STATUS"),
            UxNode::text(app.status().as_str(), 16.0, status_color(app.status())),
            UxNode::text(app.provider_title_state(), 11.0, muted()),
            UxNode::text(app.artifact_title_state(), 11.0, muted()),
        ],
    )
}

fn pane_style() -> Style {
    Style::col()
        .h(Dim::Flex(1.0))
        .pad(Edges::all(16.0))
        .radius(8.0)
        .bg(glass())
        .border(1.0, line())
        .shadow(0.0, 10.0, 32.0, Rgba::new(0.0, 0.0, 0.0, 0.30))
}

fn section_label(text: &str) -> UxNode {
    UxNode::text(text.to_ascii_uppercase(), 10.0, muted())
}

fn button(ui: &UiState, id: u32, label: &str, bg_col: Rgba, fg: Rgba) -> UxNode {
    let active = ui.is_pressed(id) || ui.is_hover(id);
    UxNode::boxed(
        Style::row()
            .button(id)
            .w(Dim::Flex(1.0))
            .h(Dim::Px(40.0))
            .align(Align::Center)
            .justify(Justify::Center)
            .radius(7.0)
            .bg(if active {
                bg_col.with_alpha(0.84)
            } else {
                bg_col
            })
            .border(1.0, if active { line_hot() } else { line() }),
        vec![UxNode::text(label, 13.0, fg)],
    )
}

fn mini_card(title: &str, body: &str, accent: Rgba) -> UxNode {
    UxNode::boxed(
        Style::row()
            .gap(9.0)
            .pad(Edges::all(9.0))
            .radius(7.0)
            .bg(glass_deep())
            .border(1.0, line()),
        vec![
            UxNode::boxed(
                Style::row()
                    .w(Dim::Px(7.0))
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

fn compact_metric(label: &str, value: impl Into<String>, color: Rgba) -> UxNode {
    UxNode::boxed(
        Style::col()
            .w(Dim::Px(104.0))
            .h(Dim::Px(50.0))
            .align(Align::Center)
            .justify(Justify::Center)
            .radius(7.0)
            .bg(glass_deep())
            .border(1.0, line()),
        vec![
            UxNode::text(value.into(), 14.0, color),
            UxNode::text(label, 9.0, muted()),
        ],
    )
}

fn status_chip(text: &str, color: Rgba) -> UxNode {
    UxNode::boxed(
        Style::row()
            .h(Dim::Px(26.0))
            .align(Align::Center)
            .justify(Justify::Center)
            .pad(Edges::xy(10.0, 0.0))
            .radius(13.0)
            .bg(color.with_alpha(0.14))
            .border(1.0, color.with_alpha(0.45)),
        vec![UxNode::text(text, 10.0, color)],
    )
}

fn top_dot(label: &str, on: bool) -> UxNode {
    UxNode::boxed(
        Style::row().gap(6.0).align(Align::Center),
        vec![
            UxNode::boxed(
                Style::row()
                    .w(Dim::Px(8.0))
                    .h(Dim::Px(8.0))
                    .radius(4.0)
                    .bg(if on { teal() } else { lamp_soft() }),
                vec![],
            ),
            UxNode::text(label, 9.0, muted()),
        ],
    )
}

fn selected_recipe(app: &NativeApp) -> &'static math_atoms_core::Recipe {
    let state = app.runtime.state();
    recipes()
        .iter()
        .find(|recipe| recipe.id == state.selected_recipe)
        .unwrap_or(&recipes()[0])
}

fn status_color(status: RuntimeStatus) -> Rgba {
    match status {
        RuntimeStatus::Draft => lamp(),
        RuntimeStatus::ProviderPending => lamp(),
        RuntimeStatus::Proven => teal(),
        RuntimeStatus::Blocked => red(),
        RuntimeStatus::DriftFlagged => blue(),
    }
}

fn provider_color(app: &NativeApp) -> Rgba {
    if app.provider_running {
        lamp()
    } else if app.provider_title_state() == "provider:blocked" {
        red()
    } else {
        teal()
    }
}

fn design_color(app: &NativeApp) -> Rgba {
    if app.design_build_running {
        lamp()
    } else if app.design_title_state() == "design:blocked" {
        red()
    } else {
        teal()
    }
}

pub fn background() -> Rgba {
    BG
}
