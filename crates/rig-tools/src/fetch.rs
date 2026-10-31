use crate::util::str_arg;
use rig_core::provider::ToolSpec;
use rig_core::text::head_tail;
use rig_core::tool::{Access, Tool, ToolCtx, ToolOutput};
use serde_json::{Value, json};
use std::time::Duration;

pub struct FetchPage;

const MAX_BODY: u64 = 2 * 1024 * 1024;
const MAX_OUTPUT: usize = 100 * 1024;
const MAX_REDIRECTS: usize = 5;

impl Tool for FetchPage {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fetch_page".into(),
            description: "Fetch a public HTTP(S) page and return it as readable text. Do not use it for localhost, private or link-local addresses, *.local or *.internal hosts, URLs that embed credentials or tokens, or anything needing a non-GET request. If a fetch fails, say what blocked you instead of substituting another page.".into(),
            parameters: json!({
                "type": "object",
                "properties": {"url": {"type": "string", "description": "The URL to fetch."}},
                "required": ["url"]
            }),
        }
    }

    fn access(&self, _: &Value) -> Access {
        Access::Network
    }

    fn describe(&self, args: &Value) -> String {
        str_arg(args, "url").unwrap_or("?").to_string()
    }

    fn run(&self, args: &Value, _: &ToolCtx) -> ToolOutput {
        let Some(url) = str_arg(args, "url") else {
            return ToolOutput::err("url is required");
        };
        match fetch(url) {
            Ok(text) => ToolOutput::ok(text),
            Err(e) => ToolOutput::err(e),
        }
    }
}

fn fetch(start: &str) -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into();
    let mut url = start.to_string();
    for _ in 0..=MAX_REDIRECTS {
        check_url(&url)?;
        let mut resp = agent
            .get(&url)
            .header("User-Agent", "rig/0.1 (+coding agent)")
            .header(
                "Accept",
                "text/html,text/markdown,text/plain,application/json;q=0.9,*/*;q=0.5",
            )
            .call()
            .map_err(|e| format!("request failed: {e}"))?;
        let status = resp.status().as_u16();
        if (300..400).contains(&status) {
            let loc = resp
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or("redirect without Location")?;
            url = absolutize(&url, loc);
            continue;
        }
        let ctype = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();
        let body = resp
            .body_mut()
            .with_config()
            .limit(MAX_BODY)
            .read_to_string()
            .map_err(|e| format!("could not read the response: {e}"))?;
        if status >= 400 {
            return Err(format!("HTTP {status} from {url}"));
        }
        let text = if ctype.contains("html") {
            html_to_text(&body)
        } else {
            body
        };
        return Ok(head_tail(&text, MAX_OUTPUT));
    }
    Err("too many redirects".into())
}

fn absolutize(base: &str, loc: &str) -> String {
    if loc.starts_with("http://") || loc.starts_with("https://") {
        return loc.to_string();
    }
    let scheme_end = base.find("://").map_or(0, |i| i + 3);
    let host_end = base[scheme_end..]
        .find('/')
        .map_or(base.len(), |i| scheme_end + i);
    if loc.starts_with('/') {
        format!("{}{loc}", &base[..host_end])
    } else {
        format!("{}/{loc}", &base[..host_end])
    }
}

/// Reject non-HTTP schemes, embedded credentials, and local/private destinations.
/// Hostnames are not resolved here, so this does not defend against DNS rebinding.
fn check_url(url: &str) -> Result<(), String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or("only http and https URLs are allowed")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return Err("URLs with embedded credentials are not allowed".into());
    }
    if authority.starts_with('[') {
        return Err("IPv6 literal hosts are not allowed".into());
    }
    let host = authority.split(':').next().unwrap_or("").to_lowercase();
    if host.is_empty() {
        return Err("URL has no host".into());
    }
    let blocked_name = host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host.ends_with(".lan");
    if blocked_name || is_private_ipv4(&host) {
        return Err(format!(
            "{host} is a local or private address and cannot be fetched"
        ));
    }
    if !host.contains('.') {
        return Err(format!("{host} is not a public hostname"));
    }
    Ok(())
}

fn is_private_ipv4(host: &str) -> bool {
    let parts: Vec<u8> = host.split('.').filter_map(|p| p.parse().ok()).collect();
    if parts.len() != 4 || host.split('.').count() != 4 {
        return false;
    }
    matches!(
        (parts[0], parts[1]),
        (0, _) | (10, _) | (127, _) | (169, 254) | (192, 168) | (100, 64..=127)
    ) || (parts[0] == 172 && (16..=31).contains(&parts[1]))
}

/// Good-enough HTML to text for reading docs: drops script/style, keeps block structure.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 3);
    let lower = html.to_lowercase();
    let bytes = html.as_bytes();
    let mut i = 0;
    let mut in_pre = false;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let Some(end) = html[i..].find('>') else {
                break;
            };
            let tag = &lower[i + 1..i + end];
            let name: String = tag
                .trim_start_matches('/')
                .chars()
                .take_while(char::is_ascii_alphanumeric)
                .collect();
            let closing = tag.starts_with('/');
            i += end + 1;
            if !closing && (name == "script" || name == "style" || name == "noscript") {
                let close = format!("</{name}");
                i = lower[i..].find(&close).map_or(bytes.len(), |p| i + p);
                continue;
            }
            match name.as_str() {
                "h1" | "h2" | "h3" if !closing => out.push_str("\n\n# "),
                "pre" => {
                    in_pre = !closing;
                    out.push('\n');
                }
                "br" | "p" | "div" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                | "ul" | "ol" | "table" | "section" | "article" | "header" | "footer"
                | "blockquote" => out.push('\n'),
                _ => {}
            }
            if name == "li" && !closing {
                out.push_str("- ");
            }
        } else {
            let next = html[i..].find('<').map_or(bytes.len(), |p| i + p);
            let chunk = decode_entities(&html[i..next]);
            if in_pre {
                out.push_str(&chunk);
            } else {
                let mut prev_space = out.ends_with([' ', '\n']);
                for c in chunk.chars() {
                    if c.is_whitespace() {
                        if !prev_space {
                            out.push(' ');
                        }
                        prev_space = true;
                    } else {
                        out.push(c);
                        prev_space = false;
                    }
                }
            }
            i = next;
        }
    }
    let mut cleaned = String::with_capacity(out.len());
    let mut blank = 0;
    for line in out.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        cleaned.push_str(line.trim_start_matches(' '));
        cleaned.push('\n');
    }
    cleaned.trim().to_string()
}

fn decode_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests;
