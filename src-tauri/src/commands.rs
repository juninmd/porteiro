//! Thin IPC layer. Authorization and validation live in `Core`; the webview is not trusted with either.
use crate::{
    api::{ApiError, Status},
    capture,
    core::Core,
    events::Event,
};
use std::sync::{Arc, Mutex, MutexGuard};
use tauri::{ipc::Response, AppHandle, State};

pub type Shared = Arc<Mutex<Core>>;
pub type CoreState<'a> = State<'a, Shared>;

pub fn lock(core: &Shared) -> MutexGuard<'_, Core> {
    // A panic elsewhere must not turn into a permanently dead UI; the state itself is re-derivable.
    core.lock().unwrap_or_else(|p| p.into_inner())
}

#[tauri::command]
pub async fn get_status(core: CoreState<'_>) -> Result<Status, ApiError> {
    Ok(lock(&core).status())
}

#[tauri::command]
pub async fn unlock(core: CoreState<'_>, password: String, minutes: Option<u32>) -> Result<Status, ApiError> {
    lock(&core).unlock(&password, minutes)
}

#[tauri::command]
pub async fn relock(core: CoreState<'_>) -> Result<Status, ApiError> {
    Ok(lock(&core).relock())
}

#[tauri::command]
pub async fn list_events(core: CoreState<'_>, offset: usize, limit: usize) -> Result<Vec<Event>, ApiError> {
    Ok(lock(&core).list_events(offset, limit))
}

#[tauri::command]
pub async fn get_shot(core: CoreState<'_>, name: String) -> Result<Response, ApiError> {
    let dir = lock(&core).shots_dir();
    capture::read(&dir, &name).map(Response::new).map_err(|e| ApiError::new("shot_unavailable", e))
}

#[tauri::command]
pub async fn add_domain(core: CoreState<'_>, password: String, domain: String) -> Result<Status, ApiError> {
    lock(&core).add_domain(&password, &domain)
}

#[tauri::command]
pub async fn remove_domain(core: CoreState<'_>, password: String, domain: String) -> Result<Status, ApiError> {
    lock(&core).remove_domain(&password, &domain)
}

#[tauri::command]
pub async fn set_service(core: CoreState<'_>, password: String, id: String, enabled: bool) -> Result<Status, ApiError> {
    lock(&core).set_service(&password, &id, enabled)
}

#[tauri::command]
pub async fn set_unlock_minutes(core: CoreState<'_>, password: String, minutes: u32) -> Result<Status, ApiError> {
    lock(&core).set_unlock_minutes(&password, minutes)
}

#[tauri::command]
pub async fn change_password(core: CoreState<'_>, current: String, next: String) -> Result<Status, ApiError> {
    lock(&core).change_password(&current, &next)
}

/// The only exit. Lifts the block, removes the restart task, then ends the process.
#[tauri::command]
pub async fn disable_and_quit(app: AppHandle, core: CoreState<'_>, password: String) -> Result<(), ApiError> {
    lock(&core).disable_and_quit(&password)?;
    crate::QUIT_AUTHORIZED.store(true, std::sync::atomic::Ordering::SeqCst);
    app.exit(0);
    Ok(())
}
