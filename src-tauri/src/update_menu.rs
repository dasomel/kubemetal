//! D54: "Check for Updates…" in the app menu plus one silent check at launch.
//!
//! Menu install is best-effort: a platform menu failure must never block launch (cost: no
//! update discovery that run, retried next launch). The launch-time check is silent — offline
//! or air-gapped starts (D25) see nothing. Only a menu click reports "up to date" or a failure,
//! so an unreachable GitHub is surfaced when the user asked, never claimed as "latest".

use tauri::menu::{Menu, MenuItem};
use tauri::{App, AppHandle, Manager, Wry};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::Mutex;

use crate::services::update_check::{self, UpdateResult};

const CHECK_UPDATES: &str = "check-updates";

struct UpdateState {
    item: MenuItem<Wry>,
    /// `(version, release page)` found by an earlier check, so a later click offers it
    /// without refetching.
    found: Mutex<Option<(String, String)>>,
}

pub fn setup(app: &App) {
    if let Err(error) = install(app) {
        eprintln!("[KubeMetal] could not install update menu: {error}");
    }
}

fn install(app: &App) -> tauri::Result<()> {
    let menu = Menu::default(app.handle())?;
    let items = menu.items()?;
    let Some(app_menu) = items.first().and_then(|item| item.as_submenu()) else {
        eprintln!("[KubeMetal] default menu has no application submenu");
        return Ok(());
    };
    let item = MenuItem::with_id(app, CHECK_UPDATES, "Check for Updates…", true, None::<&str>)?;
    // Right after "About KubeMetal", where macOS apps conventionally put it.
    if app_menu.items()?.is_empty() {
        app_menu.append(&item)?;
    } else {
        app_menu.insert(&item, 1)?;
    }
    app.set_menu(menu)?;
    app.manage(UpdateState {
        item,
        found: Mutex::new(None),
    });
    app.on_menu_event(|app, event| {
        if event.id().as_ref() == CHECK_UPDATES {
            let app = app.clone();
            tauri::async_runtime::spawn(async move { run_check(app, true).await });
        }
    });
    let handle = app.handle().clone();
    tauri::async_runtime::spawn(async move { run_check(handle, false).await });
    Ok(())
}

async fn run_check(app: AppHandle, interactive: bool) {
    let state = app.state::<UpdateState>();
    if interactive {
        // Lock only to read the cached value; the network call below runs unlocked.
        let cached = state.found.lock().await.clone();
        if let Some((version, url)) = cached {
            offer(&app, &version, url);
            return;
        }
    }
    match update_check::check().await {
        UpdateResult::Available { version, url } => {
            *state.found.lock().await = Some((version.clone(), url.clone()));
            if let Err(error) = state
                .item
                .set_text(format!("Update Available (v{version})…"))
            {
                eprintln!("[KubeMetal] could not update menu label: {error}");
            }
            if interactive {
                offer(&app, &version, url);
            }
        }
        UpdateResult::UpToDate { current } if interactive => {
            app.dialog()
                .message(format!("Version {current} is the latest version."))
                .title("KubeMetal is up to date")
                .kind(MessageDialogKind::Info)
                .show(|_| {});
        }
        UpdateResult::Failed { reason } if interactive => {
            app.dialog()
                .message(reason)
                .title("Couldn't check for updates")
                .kind(MessageDialogKind::Warning)
                .show(|_| {});
        }
        _ => {}
    }
}

fn offer(app: &AppHandle, version: &str, url: String) {
    let current = update_check::current_version();
    let handle = app.clone();
    app.dialog()
        .message(format!(
            "KubeMetal v{version} is available (you have v{current}).\n\n\
             Open the release page to download it. The app is not replaced automatically."
        ))
        .title("Update available")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Open Release Page".into(),
            "Later".into(),
        ))
        .show(move |open| {
            if open {
                open_release(&handle, &url);
            }
        });
}

fn open_release(app: &AppHandle, url: &str) {
    // Re-validate at the sink: only the exact numeric release-tag page may reach the browser.
    if !update_check::is_release_url(url) {
        eprintln!("[KubeMetal] refusing to open non-release URL");
        return;
    }
    if let Err(error) = app.opener().open_url(url, None::<&str>) {
        app.dialog()
            .message(format!("Could not open the release page: {error}"))
            .title("Couldn't open update page")
            .kind(MessageDialogKind::Warning)
            .show(|_| {});
    }
}
