//! Shared HTTP + Server-Sent-Events plumbing for provider adapters.

use crate::auth::Auth;
use rig_core::provider::ProviderError;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

/// What every adapter needs to reach its backend: who it is, where it lives, how it signs in.
pub(crate) struct Endpoint {
    pub name: String,
    pub model: String,
    pub base_url: String,
    pub auth: Box<dyn Auth>,
    pub http: ureq::Agent,
}

impl Endpoint {
    pub fn new(name: String, model: String, base_url: String, auth: Box<dyn Auth>) -> Self {
        Self {
            name,
            model,
            base_url: base_url.trim_end_matches('/').to_string(),
            auth,
            http: agent(),
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }
}

/// Model entries for a list of ids, sorted, with the id as the display name.
pub(crate) fn models_from_ids(mut ids: Vec<String>) -> Vec<rig_core::provider::ModelInfo> {
    ids.sort();
    ids.into_iter()
        .map(|id| rig_core::provider::ModelInfo {
            display_name: id.clone(),
            id,
            ..Default::default()
        })
        .collect()
}

/// The `id` of every entry of `v["data"]`, the shape OpenAI-style `/models` endpoints answer with.
pub(crate) fn data_ids(v: &Value) -> Vec<String> {
    v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["id"].as_str().map(String::from))
        .collect()
}

/// A 401/403 means the credentials are wrong, which retrying cannot fix.
pub(crate) fn auth_rejected(e: ProviderError) -> ProviderError {
    match e {
        ProviderError::Http {
            status: 401 | 403,
            body,
        } => ProviderError::Auth(format!(
            "credentials rejected: {}",
            body.chars().take(300).collect::<String>()
        )),
        other => other,
    }
}

pub(crate) fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        // Time to first response byte. The body (a long stream) is deliberately unbounded.
        .timeout_recv_response(Some(Duration::from_secs(180)))
        .http_status_as_error(false)
        .build()
        .into()
}

/// For small metadata requests (model lists): fail fast instead of hanging for minutes.
pub(crate) fn quick_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(12)))
        .http_status_as_error(false)
        .build()
        .into()
}

/// POST a JSON body. A non-2xx status becomes `ProviderError::Http` carrying the response body.
pub(crate) fn post_json(
    agent: &ureq::Agent,
    url: &str,
    headers: &[(String, String)],
    body: &Value,
) -> Result<ureq::http::Response<ureq::Body>, ProviderError> {
    let payload =
        serde_json::to_string(body).map_err(|e| ProviderError::Protocol(e.to_string()))?;
    let mut req = agent.post(url);
    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }
    let mut resp = req
        .header("Content-Type", "application/json")
        .send(payload)
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        let body = resp.body_mut().read_to_string().unwrap_or_default();
        return Err(ProviderError::Http { status, body });
    }
    Ok(resp)
}

pub(crate) fn get_json(
    agent: &ureq::Agent,
    url: &str,
    headers: &[(String, String)],
) -> Result<Value, ProviderError> {
    let mut req = agent.get(url);
    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }
    let mut resp = req
        .call()
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(ProviderError::Http {
            status,
            body: resp.body_mut().read_to_string().unwrap_or_default(),
        });
    }
    resp.body_mut()
        .read_json()
        .map_err(|e| ProviderError::Protocol(format!("bad JSON from {url}: {e}")))
}

/// Read a response body as SSE on a helper thread so `cancel` takes effect immediately, even
/// while the server is silent. After a cancel the helper ends when its socket does.
pub(crate) fn stream_events(
    resp: ureq::http::Response<ureq::Body>,
    cancel: &AtomicBool,
    f: impl FnMut(Option<&str>, &str) -> Result<bool, ProviderError>,
) -> Result<(), ProviderError> {
    let (tx, rx) = mpsc::channel::<std::io::Result<String>>();
    std::thread::spawn(move || {
        let reader = BufReader::new(resp.into_body().into_reader());
        for line in reader.lines() {
            let failed = line.is_err();
            if tx.send(line).is_err() || failed {
                return;
            }
        }
    });
    let lines = std::iter::from_fn(|| {
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Some(Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "cancelled",
                )));
            }
            match rx.recv_timeout(Duration::from_millis(40)) {
                Ok(line) => return Some(line),
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => return None,
            }
        }
    });
    for_each_line(lines, f)
}

/// Feed each SSE event to `f` as `(event name, data)`. Return `Ok(false)` from `f` to stop early.
#[cfg(test)]
pub(crate) fn for_each_event(
    reader: impl BufRead,
    f: impl FnMut(Option<&str>, &str) -> Result<bool, ProviderError>,
) -> Result<(), ProviderError> {
    for_each_line(reader.lines(), f)
}

fn for_each_line(
    lines: impl Iterator<Item = std::io::Result<String>>,
    mut f: impl FnMut(Option<&str>, &str) -> Result<bool, ProviderError>,
) -> Result<(), ProviderError> {
    let mut event: Option<String> = None;
    let mut data = String::new();
    let mut has_data = false;
    for line in lines {
        let line = line.map_err(|e| {
            if e.kind() == std::io::ErrorKind::Interrupted {
                ProviderError::Cancelled
            } else {
                ProviderError::Network(format!("stream interrupted: {e}"))
            }
        })?;
        if line.is_empty() {
            if has_data {
                let keep_going = f(event.as_deref(), &data)?;
                event = None;
                data.clear();
                has_data = false;
                if !keep_going {
                    return Ok(());
                }
            }
            continue;
        }
        if line.starts_with(':') {
            continue;
        }
        let (field, value) = line.split_once(':').map_or((line.as_str(), ""), |(f, v)| {
            (f, v.strip_prefix(' ').unwrap_or(v))
        });
        match field {
            "event" => event = Some(value.to_string()),
            "data" => {
                if has_data {
                    data.push('\n');
                }
                data.push_str(value);
                has_data = true;
            }
            _ => {}
        }
    }
    if has_data {
        f(event.as_deref(), &data)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_events_comments_and_multiline_data() {
        let raw = ": keepalive\nevent: a\ndata: {\"x\":1}\n\ndata: one\ndata: two\n\nevent: tail\ndata: end";
        let mut got = Vec::new();
        for_each_event(raw.as_bytes(), |e, d| {
            got.push((e.map(str::to_string), d.to_string()));
            Ok(true)
        })
        .unwrap();
        assert_eq!(got.len(), 3);
        assert_eq!(got[0], (Some("a".into()), "{\"x\":1}".into()));
        assert_eq!(got[1], (None, "one\ntwo".into()));
        assert_eq!(got[2], (Some("tail".into()), "end".into()));
    }
}
