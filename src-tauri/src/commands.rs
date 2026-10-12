//! Thin IPC layer. Authorization and validation live in `Core`; the webview is not trusted with either.
//! The one thing done here is the Windows Hello prompt: it is modal and slow, so it runs without the core lock.
use crate::{
    api::{ApiError, Status},
    auth::Proof,
    capture,
    core::Core,
    events::Event,
    hello::{Verdict, Verifier},
    telegram::{self, Alerts, Bot, BotError, View},
};
use serde::Deserialize;
use std::sync::{Arc, Mutex, MutexGuard};
use tauri::{ipc::Response, AppHandle, Manager, State};

pub type Shared = Arc<Mutex<Core>>;
pub type CoreState<'a> = State<'a, Shared>;
pub type HelloState<'a> = State<'a, Arc<dyn Verifier>>;
pub type BotState<'a> = State<'a, Arc<dyn Bot>>;

pub fn lock(core: &Shared) -> MutexGuard<'_, Core> {
    // A panic elsewhere must not turn into a permanently dead UI; the state itself is re-derivable.
    core.lock().unwrap_or_else(|p| p.into_inner())
}

/// What the webview offers for a privileged action.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Credential {
    Password { password: String },
    Hello,
}

async fn ask_hello(app: &AppHandle, hello: &HelloState<'_>, reason: String) -> Result<Verdict, ApiError> {
    let hwnd = app.get_webview_window("main").and_then(|w| w.hwnd().ok()).map_or(0, |h| h.0 as isize);
    let verifier = hello.inner().clone();
    tauri::async_runtime::spawn_blocking(move || verifier.verify(hwnd, &reason))
        .await
        .map_err(|e| ApiError::new("internal", format!("falha no Windows Hello: {e}")))
}

fn hello_error(verdict: Verdict) -> ApiError {
    match verdict {
        Verdict::Cancelled => ApiError::new("hello_cancelled", "verificação cancelada"),
        Verdict::Unavailable(why) => ApiError::new("hello_unavailable", why),
        Verdict::Failed(why) => ApiError::new("hello_failed", why),
        Verdict::Verified => ApiError::new("internal", "estado inesperado"),
    }
}

/// Turns what the webview sent into a proof for the core. A Hello refusal is a failed attempt like a wrong
/// password (photo, lockout); closing the prompt is not.
async fn prove(app: &AppHandle, core: &CoreState<'_>, hello: &HelloState<'_>, cred: Credential, action: &'static str) -> Result<Proof, ApiError> {
    if let Credential::Password { password } = cred {
        return Ok(Proof::Password(password));
    }
    lock(core).precheck(&Proof::Hello)?;
    match ask_hello(app, hello, format!("Porteiro: {action}")).await? {
        Verdict::Verified => Ok(Proof::Hello),
        Verdict::Failed(why) => Err(lock(core).hello_failed(action, &why)),
        other => Err(hello_error(other)),
    }
}

#[tauri::command]
pub async fn get_status(core: CoreState<'_>) -> Result<Status, ApiError> {
    Ok(lock(&core).status())
}

/// First run only; the core refuses it once a password exists.
#[tauri::command]
pub async fn setup_password(core: CoreState<'_>, password: String) -> Result<Status, ApiError> {
    lock(&core).setup_password(&password)
}

#[tauri::command]
pub async fn unlock(app: AppHandle, core: CoreState<'_>, hello: HelloState<'_>, auth: Credential, minutes: Option<u32>) -> Result<Status, ApiError> {
    let proof = prove(&app, &core, &hello, auth, "liberar o acesso").await?;
    lock(&core).unlock(proof, minutes)
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
pub async fn add_domain(app: AppHandle, core: CoreState<'_>, hello: HelloState<'_>, auth: Credential, domain: String) -> Result<Status, ApiError> {
    let proof = prove(&app, &core, &hello, auth, "adicionar um site").await?;
    lock(&core).add_domain(proof, &domain)
}

#[tauri::command]
pub async fn remove_domain(app: AppHandle, core: CoreState<'_>, hello: HelloState<'_>, auth: Credential, domain: String) -> Result<Status, ApiError> {
    let proof = prove(&app, &core, &hello, auth, "remover um site").await?;
    lock(&core).remove_domain(proof, &domain)
}

#[tauri::command]
pub async fn set_service(app: AppHandle, core: CoreState<'_>, hello: HelloState<'_>, auth: Credential, id: String, enabled: bool) -> Result<Status, ApiError> {
    let proof = prove(&app, &core, &hello, auth, "alterar um serviço").await?;
    lock(&core).set_service(proof, &id, enabled)
}

#[tauri::command]
pub async fn set_unlock_minutes(app: AppHandle, core: CoreState<'_>, hello: HelloState<'_>, auth: Credential, minutes: u32) -> Result<Status, ApiError> {
    let proof = prove(&app, &core, &hello, auth, "mudar a duração da liberação").await?;
    lock(&core).set_unlock_minutes(proof, minutes)
}

/// Changing the password always needs the current one: Hello cannot stand in for the secret it backs up.
#[tauri::command]
pub async fn change_password(core: CoreState<'_>, current: String, next: String) -> Result<Status, ApiError> {
    lock(&core).change_password(&current, &next)
}

/// Turning Hello on first makes Windows confirm it works for this person, so a broken setup cannot lock the
/// owner into a method that never succeeds.
#[tauri::command]
pub async fn set_hello(app: AppHandle, core: CoreState<'_>, hello: HelloState<'_>, auth: Credential, enabled: bool) -> Result<Status, ApiError> {
    if enabled {
        match ask_hello(&app, &hello, "Porteiro: ativar o Windows Hello".into()).await? {
            Verdict::Verified => (),
            other => return Err(hello_error(other)),
        }
    }
    let proof = match auth {
        Credential::Password { password } => Proof::Password(password),
        Credential::Hello => prove(&app, &core, &hello, Credential::Hello, "alterar o Windows Hello").await?,
    };
    lock(&core).set_hello(proof, enabled)
}

/// The only exit. Lifts the block, removes the restart task, then ends the process.
#[tauri::command]
pub async fn disable_and_quit(app: AppHandle, core: CoreState<'_>, hello: HelloState<'_>, auth: Credential) -> Result<(), ApiError> {
    let proof = prove(&app, &core, &hello, auth, "desativar a proteção").await?;
    lock(&core).disable_and_quit(proof)?;
    crate::QUIT_AUTHORIZED.store(true, std::sync::atomic::Ordering::SeqCst);
    app.exit(0);
    Ok(())
}

fn bot_error(e: BotError) -> ApiError {
    let code = match e {
        BotError::Unauthorized => "telegram_token_rejected",
        BotError::Transient(_) | BotError::RetryAfter(_) => "telegram_unreachable",
        BotError::Rejected(_) => "telegram_error",
    };
    ApiError::new(code, e.describe())
}

#[tauri::command]
pub async fn telegram_status(core: CoreState<'_>) -> Result<View, ApiError> {
    Ok(lock(&core).telegram_view())
}

/// The password is checked before the token goes anywhere, and the check with Telegram runs without the core
/// lock: it can take seconds and the UI polls the core every second.
#[tauri::command]
pub async fn telegram_connect(app: AppHandle, core: CoreState<'_>, hello: HelloState<'_>, bot: BotState<'_>, auth: Credential, token: String) -> Result<View, ApiError> {
    let token = token.trim().to_string();
    if !telegram::valid_token(&token) {
        return Err(ApiError::new("telegram_token_invalid", "token inválido: copie-o inteiro do @BotFather"));
    }
    let proof = prove(&app, &core, &hello, auth, "conectar o Telegram").await?;
    let verified = lock(&core).verify(proof, "conectar o Telegram")?;
    let (client, probe) = (bot.inner().clone(), token.clone());
    let me = tauri::async_runtime::spawn_blocking(move || client.get_me(&probe))
        .await
        .map_err(|e| ApiError::new("internal", format!("falha ao falar com o Telegram: {e}")))?
        .map_err(bot_error)?;
    lock(&core).telegram_connect(verified, &token, &me.username)
}

#[tauri::command]
pub async fn telegram_pair(app: AppHandle, core: CoreState<'_>, hello: HelloState<'_>, auth: Credential) -> Result<View, ApiError> {
    let proof = prove(&app, &core, &hello, auth, "parear um chat do Telegram").await?;
    lock(&core).telegram_pair(proof)
}

#[tauri::command]
pub async fn telegram_remove_chat(app: AppHandle, core: CoreState<'_>, hello: HelloState<'_>, auth: Credential, chat: i64) -> Result<View, ApiError> {
    let proof = prove(&app, &core, &hello, auth, "remover um chat do Telegram").await?;
    lock(&core).telegram_remove_chat(proof, chat)
}

#[tauri::command]
pub async fn telegram_set_alerts(app: AppHandle, core: CoreState<'_>, hello: HelloState<'_>, auth: Credential, alerts: Alerts) -> Result<View, ApiError> {
    let proof = prove(&app, &core, &hello, auth, "mudar os alertas do Telegram").await?;
    lock(&core).telegram_set_alerts(proof, alerts)
}

#[tauri::command]
pub async fn telegram_disconnect(app: AppHandle, core: CoreState<'_>, hello: HelloState<'_>, auth: Credential) -> Result<View, ApiError> {
    let proof = prove(&app, &core, &hello, auth, "desconectar o Telegram").await?;
    lock(&core).telegram_disconnect(proof)
}
