//! A one-line text box for something typed once: an API key, or a new profile's name.
//! The TUI opens it over the composer; the view draws it; `tui/entry.rs` acts on it.

pub enum Purpose {
    /// Save an API key for this provider in the active profile.
    ApiKey { provider: String },
    /// Create a profile and switch to it.
    NewProfile,
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
