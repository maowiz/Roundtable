//! Tauri host: a narrow, validated command surface over the Rust core (backend plan section 11).
//! The renderer never receives a key, never builds a request, never runs SQL.

mod commands;
mod types;

use rt_adapters::{anthropic::AnthropicAdapter, google::GoogleAdapter, openai::OpenAiAdapter, openai_compat::OpenAiCompatAdapter, ProviderAdapter, ProviderCatalog};
use rt_budget::Catalog;
use rt_core::RunId;
use rt_engine::{CredentialManagerSecrets, Engine};
use rt_ledger::Ledger;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use tokio_util::sync::CancellationToken;

pub struct AppState {
    pub engine: Arc<Engine>,
    pub providers: ProviderCatalog,
    pub running: Arc<Mutex<HashMap<RunId, CancellationToken>>>,
    pub data_dir: std::path::PathBuf,
}

fn build_engine(data_dir: &std::path::Path) -> Result<Arc<Engine>, String> {
    let (ledger, report) = Ledger::open(data_dir.join("data").join("roundtable.db")).map_err(|e| e.to_string())?;
    tracing::info!(?report, "ledger opened");
    let providers = ProviderCatalog::bundled();
    let adapters: Vec<Arc<dyn ProviderAdapter>> = vec![
        Arc::new(OpenAiAdapter { catalog: providers.clone() }),
        Arc::new(AnthropicAdapter { catalog: providers.clone() }),
        Arc::new(GoogleAdapter { catalog: providers.clone() }),
        Arc::new(OpenAiCompatAdapter { provider_id: "xai".into(), catalog: providers.clone() }),
        Arc::new(OpenAiCompatAdapter { provider_id: "groq".into(), catalog: providers.clone() }),
        Arc::new(OpenAiCompatAdapter { provider_id: "openrouter".into(), catalog: providers.clone() }),
    ];
    let prices = Catalog::bundled().map_err(|e| e.to_string())?;
    Ok(Engine::new(Arc::new(ledger), adapters, Arc::new(CredentialManagerSecrets), prices))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive("info".parse().unwrap())).init();
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_local_data_dir()?;
            let engine = build_engine(&data_dir).map_err(std::io::Error::other)?;
            // launch-time recovery (PRD 8.2): non-terminal runs become interrupted, in-flight attempts uncertain
            match engine.mark_interrupted_at_launch() {
                Ok(ids) if !ids.is_empty() => tracing::info!(count = ids.len(), "runs marked interrupted at launch"),
                Ok(_) => {}
                Err(e) => tracing::error!(%e, "launch recovery failed"),
            }
            // event bridge: every committed engine event becomes a window event with its sequence number
            let handle = app.handle().clone();
            let mut rx = engine.subscribe();
            tauri::async_runtime::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(ev) => {
                            let _ = handle.emit("run-event", &ev);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(n, "event subscriber lagged; UI will resync from its sequence");
                        }
                        Err(_) => break,
                    }
                }
            });
            app.manage(AppState { engine, providers: ProviderCatalog::bundled(), running: Arc::new(Mutex::new(HashMap::new())), data_dir });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_credentials,
            commands::save_credential,
            commands::test_credential,
            commands::delete_credential,
            commands::list_providers,
            commands::list_models,
            commands::list_panels,
            commands::save_panel,
            commands::delete_panel,
            commands::preflight_run,
            commands::start_run,
            commands::cancel_run,
            commands::resume_run,
            commands::retry_uncertain,
            commands::get_run_snapshot,
            commands::get_events,
            commands::list_run_papers,
            commands::inspect_paper,
            commands::list_runs,
            commands::delete_run,
            commands::get_settings,
            commands::save_settings,
            commands::app_info,
            commands::get_system_prompts,
            commands::save_system_prompts,
            commands::preview_prompt,
        ])
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed) {
                tracing::info!(window = %window.label(), ?event, "window closing");
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building Roundtable")
        .run(|_app, event| match event {
            tauri::RunEvent::ExitRequested { code, .. } => tracing::info!(?code, "exit requested"),
            tauri::RunEvent::Exit => tracing::info!("exiting"),
            _ => {}
        });
}
