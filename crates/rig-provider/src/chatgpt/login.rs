//! The browser sign-in flow: PKCE, the localhost callback, and the code exchange.

use super::*;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use rig_core::provider::ProviderError;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::time::{Duration, Instant};

pub(super) fn random_b64(n: usize) -> Result<String, ProviderError> {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b)
        .map_err(|e| ProviderError::Auth(format!("system randomness unavailable: {e}")))?;
    Ok(B64.encode(b))
}

pub(super) fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub(super) fn urldecode(s: &str) -> String {
    let b = s.as_bytes();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < b.len() {
        match b[i] {
            b'%' => {
                let hex = b
                    .get(i + 1..i + 3)
                    .and_then(|h| std::str::from_utf8(h).ok());
                if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    out.push(v);
                    i += 3;
                    continue;
                }
                out.push(b'%');
            }
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(super) fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| urldecode(v))
    })
}

pub struct LoginPlan {
    pub url: String,
    verifier: String,
    state: String,
}

impl LoginPlan {
    pub fn new() -> Result<Self, ProviderError> {
        let verifier = random_b64(48)?;
        let challenge = B64.encode(Sha256::digest(verifier.as_bytes()));
        let state = random_b64(24)?;
        let redirect = format!("http://localhost:{CALLBACK_PORT}/auth/callback");
        let url = format!(
            "{ISSUER}/oauth/authorize?response_type=code&client_id={CLIENT_ID}&redirect_uri={}&scope={}&code_challenge={challenge}&code_challenge_method=S256&id_token_add_organizations=true&codex_cli_simplified_flow=true&state={state}&originator=rig",
            urlencode(&redirect),
            urlencode(SCOPE)
        );
        Ok(Self {
            url,
            verifier,
            state,
        })
    }
}

/// Run the browser login: wait for the redirect on localhost:1455, exchange the code, store tokens.
/// `announce` receives the URL to open (and may open it).
pub fn login(path: &Path, plan: &LoginPlan, announce: &dyn Fn(&str)) -> Result<(), ProviderError> {
    let listener = TcpListener::bind(("127.0.0.1", CALLBACK_PORT)).map_err(|e| {
        ProviderError::Auth(format!("cannot listen on localhost:{CALLBACK_PORT} ({e}). Another login (e.g. `codex login`) may be running."))
    })?;
    listener
        .set_nonblocking(true)
        .map_err(|e| ProviderError::Auth(e.to_string()))?;
    announce(&plan.url);

    let deadline = Instant::now() + Duration::from_secs(300);
    let code = loop {
        if Instant::now() > deadline {
            return Err(ProviderError::Auth(
                "timed out waiting for the browser sign-in".into(),
            ));
        }
        let mut stream = match listener.accept() {
            Ok((s, _)) => s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(e) => return Err(ProviderError::Auth(e.to_string())),
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buf = [0u8; 8192];
        let n = stream.read(&mut buf).unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]);
        let target = req
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap_or("");
        let (route, query) = target.split_once('?').unwrap_or((target, ""));
        let mut reply = |status: &str, body: &str| {
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        };
        if route != "/auth/callback" {
            reply("404 Not Found", "not found");
            continue;
        }
        if let Some(err) = query_param(query, "error") {
            reply("400 Bad Request", "Sign-in failed. You can close this tab.");
            return Err(ProviderError::Auth(format!("sign-in was rejected: {err}")));
        }
        if query_param(query, "state").as_deref() != Some(plan.state.as_str()) {
            reply("400 Bad Request", "State mismatch.");
            return Err(ProviderError::Auth(
                "sign-in state mismatch; try again".into(),
            ));
        }
        match query_param(query, "code") {
            Some(c) => {
                reply(
                    "200 OK",
                    "<h3>Signed in to rig.</h3><p>You can close this tab and return to the terminal.</p>",
                );
                break c;
            }
            None => reply("400 Bad Request", "Missing code."),
        }
    };

    let form = format!(
        "grant_type=authorization_code&code={}&redirect_uri={}&client_id={CLIENT_ID}&code_verifier={}",
        urlencode(&code),
        urlencode(&format!("http://localhost:{CALLBACK_PORT}/auth/callback")),
        urlencode(&plan.verifier)
    );
    let mut resp = crate::http::agent()
        .post(&format!("{ISSUER}/oauth/token"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(form)
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    if !resp.status().is_success() {
        let body = resp.body_mut().read_to_string().unwrap_or_default();
        return Err(ProviderError::Http {
            status: resp.status().as_u16(),
            body,
        });
    }
    let tok: Value = resp
        .body_mut()
        .read_json()
        .map_err(|e| ProviderError::Protocol(format!("token response: {e}")))?;
    let (Some(id), Some(access), Some(refresh)) = (
        tok["id_token"].as_str(),
        tok["access_token"].as_str(),
        tok["refresh_token"].as_str(),
    ) else {
        return Err(ProviderError::Protocol(
            "token response is missing tokens".into(),
        ));
    };
    let mut tokens = json!({"id_token": id, "access_token": access, "refresh_token": refresh});
    if let Some(acc) = account_id_from(&tokens) {
        tokens["account_id"] = json!(acc);
    }
    write_file(
        path,
        &json!({"auth_mode": "chatgpt", "OPENAI_API_KEY": null, "tokens": tokens, "last_refresh": iso_now()}),
    )
}
