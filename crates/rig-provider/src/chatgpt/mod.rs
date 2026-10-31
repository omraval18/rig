//! ChatGPT subscription ("Codex") OAuth.
//!
//! The token file uses the same JSON layout as the Codex CLI's `auth.json`, so
//! a file produced by either tool is readable by both. `rig login codex` runs
//! its own PKCE flow and therefore owns an independent refresh-token chain:
//! sharing one chain between two programs would make the first refresh log the
//! other one out, because refresh tokens rotate.

use crate::auth::Auth;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use rig_core::provider::ProviderError;
use rig_core::sync::MutexExt;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

mod login;

pub use login::{LoginPlan, login};

/// Public OAuth client id of the Codex CLI.
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const ISSUER: &str = "https://auth.openai.com";
const SCOPE: &str = "openid profile email offline_access api.connectors.read api.connectors.invoke";
const CALLBACK_PORT: u16 = 1455;
/// Refresh this long before the access token expires.
const REFRESH_MARGIN_SECS: u64 = 120;

pub struct ChatGptAuth {
    path: PathBuf,
    refresh_lock: Mutex<()>,
    /// The token file as last read, keyed by its modification time, so a request does not
    /// re-read and re-parse it unless another process (or a refresh) changed it.
    cache: Mutex<Option<(SystemTime, Value)>>,
}

impl ChatGptAuth {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            refresh_lock: Mutex::new(()),
            cache: Mutex::new(None),
        }
    }

    fn tokens(&self) -> Result<Value, ProviderError> {
        let mtime = std::fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .ok();
        let mut cache = self.cache.locked();
        if let (Some(t), Some((seen, v))) = (mtime, cache.as_ref())
            && t == *seen
        {
            return Ok(v.clone());
        }
        let v = read_file(&self.path)?;
        *cache = mtime.map(|t| (t, v.clone()));
        Ok(v)
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn claims(jwt: &str) -> Value {
    jwt.split('.')
        .nth(1)
        .and_then(|p| B64.decode(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(Value::Null)
}

fn account_id_from(tokens: &Value) -> Option<String> {
    if let Some(id) = tokens["account_id"].as_str() {
        return Some(id.to_string());
    }
    for key in ["id_token", "access_token"] {
        let c = claims(tokens[key].as_str().unwrap_or(""));
        if let Some(id) = c["https://api.openai.com/auth"]["chatgpt_account_id"].as_str() {
            return Some(id.to_string());
        }
    }
    None
}

fn read_file(path: &Path) -> Result<Value, ProviderError> {
    let raw = std::fs::read_to_string(path).map_err(|_| {
        ProviderError::Auth(format!(
            "not signed in to ChatGPT ({} not found). Run `rig login codex`.",
            path.display()
        ))
    })?;
    let v: Value = serde_json::from_str(&raw)
        .map_err(|e| ProviderError::Auth(format!("{} is not valid JSON: {e}", path.display())))?;
    if v["tokens"]["access_token"].as_str().is_none() {
        return Err(ProviderError::Auth(format!(
            "{} has no ChatGPT login. Run `rig login codex`.",
            path.display()
        )));
    }
    Ok(v)
}

fn write_file(path: &Path, v: &Value) -> Result<(), ProviderError> {
    let text = serde_json::to_string_pretty(v).unwrap_or_default();
    rig_core::fsutil::write_atomic(path, text.as_bytes(), 0o600)
        .map(drop)
        .map_err(|e| ProviderError::Auth(format!("cannot write {}: {e}", path.display())))
}

fn iso_now() -> String {
    let secs = now_secs() as i64;
    let (y, m, d) = rig_core::prompt::civil_from_days(secs.div_euclid(86_400));
    let t = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        t / 3600,
        t % 3600 / 60,
        t % 60
    )
}

impl ChatGptAuth {
    fn refresh(&self, stale_refresh_token: &str) -> Result<Value, ProviderError> {
        let _guard = self.refresh_lock.locked();
        // Another thread or process may have refreshed already; refresh tokens rotate,
        // so reusing the stale one would fail and could invalidate the session.
        let current = read_file(&self.path)?;
        if current["tokens"]["refresh_token"].as_str() != Some(stale_refresh_token) {
            return Ok(current);
        }
        let body = json!({
            "client_id": CLIENT_ID,
            "grant_type": "refresh_token",
            "refresh_token": stale_refresh_token,
            "scope": "openid profile email",
        });
        let mut resp = crate::http::post_json(
            &crate::http::agent(),
            &format!("{ISSUER}/oauth/token"),
            &[],
            &body,
        )
        .map_err(|e| match e {
            ProviderError::Http { status, .. } if status == 400 || status == 401 => {
                ProviderError::Auth("ChatGPT session expired. Run `rig login codex` again.".into())
            }
            other => other,
        })?;
        let fresh: Value = resp
            .body_mut()
            .read_json()
            .map_err(|e| ProviderError::Protocol(format!("token response: {e}")))?;
        let mut next = current;
        for k in ["id_token", "access_token", "refresh_token"] {
            if let Some(t) = fresh[k].as_str() {
                next["tokens"][k] = json!(t);
            }
        }
        next["last_refresh"] = json!(iso_now());
        write_file(&self.path, &next)?;
        Ok(next)
    }
}

impl Auth for ChatGptAuth {
    fn headers(&self, force_refresh: bool) -> Result<Vec<(String, String)>, ProviderError> {
        let mut file = self.tokens()?;
        let exp = claims(file["tokens"]["access_token"].as_str().unwrap_or(""))["exp"]
            .as_u64()
            .unwrap_or(0);
        if force_refresh || exp < now_secs() + REFRESH_MARGIN_SECS {
            let rt = file["tokens"]["refresh_token"]
                .as_str()
                .ok_or_else(|| {
                    ProviderError::Auth("no refresh token; run `rig login codex`".into())
                })?
                .to_string();
            file = self.refresh(&rt)?;
        }
        let tokens = &file["tokens"];
        let mut h = vec![(
            "Authorization".to_string(),
            format!("Bearer {}", tokens["access_token"].as_str().unwrap_or("")),
        )];
        if let Some(id) = account_id_from(tokens) {
            h.push(("chatgpt-account-id".into(), id));
        }
        Ok(h)
    }

    fn refreshable(&self) -> bool {
        true
    }
}

// ---- interactive login ---------------------------------------------------------------

/// Summary for `rig doctor`: plan type and token lifetime, never the tokens themselves.
pub fn status(path: &Path) -> Result<String, ProviderError> {
    let f = read_file(path)?;
    let c = claims(f["tokens"]["access_token"].as_str().unwrap_or(""));
    let plan = c["https://api.openai.com/auth"]["chatgpt_plan_type"]
        .as_str()
        .unwrap_or("unknown");
    let left = c["exp"].as_u64().unwrap_or(0) as i64 - now_secs() as i64;
    Ok(format!(
        "ChatGPT plan: {plan}; access token {}",
        if left > 0 {
            format!("valid for {}h", left / 3600)
        } else {
            "expired (will refresh)".into()
        }
    ))
}

#[cfg(test)]
mod tests;
