//! A one-line text box for something typed once: an API key, or a new profile's name.
//! The TUI opens it over the composer; the view draws it; `tui/entry.rs` acts on it.

pub enum Purpose {
    /// Save an API key for this provider in the active profile.
    ApiKey {
        provider: String,
    },
    /// Create a profile and switch to it.
    NewProfile,
    /// Adding a provider, one answer per step: name, then (after the API style is picked) the
    /// base URL, the key, and the model. Each step carries what the earlier ones gave.
    ProviderName,
    ProviderUrl {
        name: String,
        kind: String,
    },
    /// A model picked from models.dev: the key is the last step, and the rest is already known.
    ProviderKey {
        name: String,
        kind: String,
        url: String,
        /// Set when the provider came from the catalog: its model and its catalog id.
        model: Option<String>,
        catalog: Option<String>,
    },
    /// A few words to search models.dev for.
    CatalogSearch,
    ProviderModel {
        name: String,
        kind: String,
        url: String,
        key: String,
    },
}

pub struct Entry {
    pub purpose: Purpose,
    pub title: String,
    /// Shown after the title, e.g. where the value is stored.
    pub hint: String,
    pub value: String,
    /// Draw the value as bullets, so a key cannot be read over your shoulder.
    pub secret: bool,
}
