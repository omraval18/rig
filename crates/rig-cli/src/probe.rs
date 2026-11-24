//! Finding out what a custom endpoint is from its URL alone: which API it speaks, its exact base
//! URL, and the models it serves.

use rig_core::provider::{ModelInfo, ProviderError};
use rig_provider::{BuildEnv, Kind, ProviderSpec};
use std::path::PathBuf;

/// An endpoint the user named by URL.
pub fn spec(name: &str, kind: Kind, url: &str, key: &str) -> ProviderSpec {
    ProviderSpec {
        name: name.into(),
        kind,
        base_url: url.into(),
        api_key_env: None,
        api_key: None,
        chatgpt_plan: false,
        keyless: key.is_empty(),
        web_search: false,
        default_model: None,
        catalog_id: None,
        subscription: false,
        routes: Vec::new(),
    }
}

/// The API root of a pasted URL, which may be a full endpoint. Plain http only for this machine.
pub fn clean_url(typed: &str) -> Result<String, String> {
    let mut url = typed.trim().trim_end_matches('/');
    for tail in ["/chat/completions", "/v1/messages", "/models"] {
        url = url.strip_suffix(tail).unwrap_or(url);
    }
    let secure = url.strip_prefix("https://").is_some_and(|h| !h.is_empty());
    if !secure && !is_local(url) {
        return Err("the URL must start with https:// (http:// only for this machine)".into());
    }
    if url.contains(char::is_whitespace) || host(url).contains('@') {
        return Err("that is not a URL rig can use".into());
    }
    Ok(url.to_string())
}

/// `host[:port]` of a URL.
fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split(['/', '?', '#']).next().unwrap_or(rest)
}

/// A server on this machine may use plain http and may have no key.
pub fn is_local(url: &str) -> bool {
    url.starts_with("http://")
        && (host(url).starts_with("[::1]")
            || matches!(host(url).split(':').next(), Some("localhost" | "127.0.0.1")))
}

/// A name for the provider at `url`: `https://api.example.com/v1` is `example`,
/// `http://localhost:8000` is `local-8000`. `taken` names get a number.
pub fn name_for(url: &str, taken: impl Fn(&str) -> bool) -> String {
    let host = host(url);
    let base = if is_local(url) {
        match host.rsplit_once(':') {
            Some((_, port)) if port.chars().all(|c| c.is_ascii_digit()) => format!("local-{port}"),
            _ => "local".into(),
        }
    } else {
        let bare = host.split(':').next().unwrap_or(host);
        let numeric = bare.chars().all(|c| c.is_ascii_digit() || c == '.');
        let labels: Vec<&str> = bare.split('.').collect();
        match labels.as_slice() {
            [.., owner, _] if !numeric => (*owner).to_string(),
            _ => bare.replace('.', "-"),
        }
    };
    let base: String = base
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(28)
        .collect();
    let base = Some(base.trim_start_matches('-'))
        .filter(|b| !b.is_empty())
        .unwrap_or("custom");
    std::iter::once(base.to_string())
        .chain((2..).map(|n| format!("{base}-{n}")))
        .find(|n| !taken(n))
        .unwrap_or_default()
}

/// Ask the server for its models in each API style until one answers.
pub fn probe(url: &str, key: &str) -> Result<(Kind, String, Vec<ModelInfo>), String> {
    let bare = url.strip_suffix("/v1").unwrap_or(url);
    // The Anthropic adapter adds /v1 itself; OpenAI-style roots usually include it.
    let mut tries = vec![(Kind::Chat, url.to_string())];
    if bare == url {
        tries.push((Kind::Chat, format!("{url}/v1")));
    }
    tries.push((Kind::Anthropic, bare.to_string()));
    if url.contains("anthropic") {
        tries.rotate_right(1);
    }
    let mut rejected = false;
    let mut first_error = None;
    for (kind, base) in tries {
        let mut spec = spec("endpoint", kind, &base, key);
        spec.api_key = Some(key.to_string()).filter(|k| !k.is_empty());
        let env = BuildEnv {
            chatgpt_auth_file: PathBuf::new(),
            getenv: &|_| None,
        };
        let listed = rig_provider::build(&spec, "", &env).and_then(|p| p.list_models());
        match listed {
            Ok(models) if !models.is_empty() => return Ok((kind, base, models)),
            Ok(_) => {}
            Err(e) => {
                rejected |= e.is_rejected();
                if let ProviderError::Network(_) = e {
                    return Err(e.to_string());
                }
                first_error.get_or_insert(e.to_string());
            }
        }
    }
    Err(if rejected {
        "the server rejected the key, or does not list its models".into()
    } else {
        first_error.unwrap_or_else(|| "the server lists no models".into())
    })
}

#[cfg(test)]
mod tests;
