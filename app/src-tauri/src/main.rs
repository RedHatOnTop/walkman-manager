#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod analysis;
mod build;
mod devices;
mod install;
mod install_drive;
mod library;
mod retag;
mod sensme;
mod studio;
mod tools;

use serde::Serialize;
use tauri::Manager;
use tauri::http::{Response, header};
use uwm_core::adb::Adb;
use uwm_core::profile::{self, Profile};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Environment {
    version: &'static str,
    os: &'static str,
    adb_path: Option<String>,
    adb_version: Option<String>,
}

#[tauri::command]
fn environment() -> Environment {
    let adb = Adb::locate(None).ok();
    Environment {
        version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        adb_path: adb.as_ref().map(|a| a.path().display().to_string()),
        adb_version: adb.and_then(|a| a.version().ok()),
    }
}

#[tauri::command]
fn profiles() -> Vec<Profile> {
    profile::all().to_vec()
}

fn main() {
    // pkexec starts this binary again to send Sony's update command as root; that copy does
    // only that.
    let mut args = std::env::args_os().skip(1);
    if args.next().is_some_and(|a| a == uwm_core::flash::upg::HELPER_ARG) {
        std::process::exit(uwm_core::flash::upg::helper_main(args.next().as_deref()));
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn,walkman_manager=info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("art", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let key = request.uri().path().trim_start_matches('/').to_owned();
            // A player cover may take an adb round trip, so it is answered off the
            // webview's thread.
            std::thread::spawn(move || {
                let file = app
                    .try_state::<library::LibraryService>()
                    .and_then(|svc| svc.art_bytes(&key));
                let response = match file {
                    Some(bytes) => Response::builder()
                        .header(header::CONTENT_TYPE, "image/jpeg")
                        .header(header::CACHE_CONTROL, "max-age=31536000, immutable")
                        .body(bytes),
                    None => Response::builder().status(404).body(Vec::new()),
                };
                responder.respond(response.expect("static response parts"));
            });
        })
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let cache_dir = app.path().app_cache_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            // Before the device scanner, which takes the downloaded adb where PATH has none.
            app.manage(tools::ToolsService::new(&data_dir, &cache_dir));
            let state = devices::DeviceService::start(app.handle().clone(), data_dir.clone());
            app.manage(state);
            app.manage(library::LibraryService::new(&data_dir, &cache_dir));
            app.manage(retag::LookupService::new(&data_dir, &cache_dir));
            app.manage(analysis::AnalysisService::new(&data_dir, &cache_dir));
            app.manage(studio::StudioService::new(&data_dir));
            app.manage(install::InstallService::new());
            library::start_scan(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            environment,
            profiles,
            devices::scan_devices,
            devices::rescan_devices,
            devices::known_devices,
            devices::device_snapshot,
            library::library_state,
            library::library_add_root,
            library::library_remove_root,
            library::library_rescan,
            library::library_cancel_scan,
            library::device_music,
            library::sync_plan,
            library::sync_start,
            library::sync_cancel,
            library::player_library,
            library::pull_plan,
            library::pull_start,
            retag::lookup_search,
            retag::lookup_cover,
            retag::lookup_propose,
            retag::retag_start,
            retag::retag_forget,
            sensme::sensme_import,
            sensme::sensme_read,
            analysis::analysis_engine,
            analysis::analysis_pick_engine,
            analysis::analysis_plan,
            analysis::analysis_start,
            studio::studio_view,
            studio::studio_add,
            studio::studio_pick,
            studio::studio_fetch,
            studio::studio_remove,
            studio::studio_cancel,
            studio::studio_open_guide,
            studio::studio_licence,
            studio::studio_agree,
            studio::studio_open_input_guide,
            build::studio_pick_repo,
            build::studio_read_key,
            build::studio_host,
            build::studio_build,
            build::studio_build_cancel,
            build::studio_open_build,
            build::studio_forget_build,
            install::install_plan,
            install::install_state,
            install::install_start,
            install::install_cancel,
            install::install_skip,
            install::install_clear,
            install::install_leave,
            tools::tools_view,
            tools::tools_licence,
            tools::tools_agree,
            tools::tools_fetch,
            tools::tools_cancel,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start the application");
}
