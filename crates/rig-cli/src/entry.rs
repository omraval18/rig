//! A one-line text box for something typed once: an API key, or a new profile's name.
//! The TUI opens it over the composer; the view draws it; `tui/entry.rs` acts on it.

use rig_provider::Kind;

pub enum Purpose {
    /// Save an API key for this provider in the active profile.
    ApiKey {
        provider: String,
    },
    /// Create a profile and switch to it.
    NewProfile,
    /// A custom endpoint: its URL, then its key. The rest is found by asking the server.
    ProviderUrl,
    ProviderKey {
        url: String,
    },
    /// A provider from models.dev, where only the key is missing. `model` is set when one was picked.
    ListedKey {
        name: String,
        model: Option<String>,
    },
    /// A few words to search models.dev for.
    CatalogSearch,
    /// The server did not say what it serves: the model is typed.
    ProviderModel {
        kind: Kind,
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
