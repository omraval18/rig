//! Credential sources. A provider asks its [`Auth`] for request headers.

use rig_core::provider::ProviderError;

pub trait Auth: Send + Sync {
    /// Headers to attach. `force_refresh` is set after the server rejected the previous credential.
    fn headers(&self, force_refresh: bool) -> Result<Vec<(String, String)>, ProviderError>;
    /// Whether a rejected credential can be renewed, making one retry worthwhile.
    fn refreshable(&self) -> bool {
        false
    }
}

pub struct StaticHeader {
    name: &'static str,
    value: String,
}

impl StaticHeader {
    pub fn bearer(key: String) -> Self {
        Self {
            name: "Authorization",
            value: format!("Bearer {key}"),
        }
    }

    pub fn x_api_key(key: String) -> Self {
        Self {
            name: "x-api-key",
            value: key,
        }
    }

    /// No credential, for local servers such as Ollama.
    pub fn none() -> Self {
        Self {
            name: "",
            value: String::new(),
        }
    }
}

impl Auth for StaticHeader {
    fn headers(&self, _: bool) -> Result<Vec<(String, String)>, ProviderError> {
        if self.name.is_empty() {
            return Ok(Vec::new());
        }
        Ok(vec![(self.name.to_string(), self.value.clone())])
    }
}
