//! `/profile`: switch, create and remove profiles.

use crate::picker::{CREATE, Create, PickItem, PickKind};
use crate::profile;
use crate::tui::Tui;

impl Tui {
    /// Every profile as a row; the value is its name.
    pub(crate) fn profile_items(&self) -> Vec<PickItem> {
        let paths = &self.app.paths;
        profile::list(&paths.base)
            .into_iter()
            .map(|name| {
                PickItem::labeled(&name)
                    .detail(profile::describe(&paths.base, &name))
                    .current(name == paths.profile)
            })
            .collect()
    }

    /// The list, where typing a name that does not exist offers to create it.
    pub(crate) fn open_profile_picker(&mut self, filter: &str) {
        let items = self.profile_items();
        self.open_picker(PickKind::Profile, "Switch or create a profile", items);
        let copy_from = self.app.paths.profile.clone();
        if let Some(p) = self.prompt.picker.as_mut() {
            p.set_create(Create::Profile { copy_from });
            p.set_filter(filter);
        }
    }

    pub(crate) fn cmd_profile(&mut self, arg: &str) {
        let (verb, rest) = arg
            .split_once(char::is_whitespace)
            .map_or((arg, ""), |(v, r)| (v, r.trim()));
        match verb {
            "" => self.open_profile_picker(""),
            "new" | "create" if rest.is_empty() => self.open_profile_entry(),
            "new" | "create" => {
                if let Err(e) = self.create_profile(rest, None) {
                    self.error(&e);
                }
            }
            "remove" | "rm" if rest.is_empty() => self.say("Usage: /profile remove <name>"),
            "remove" | "rm" => self.confirm_remove_profile(rest),
            name if profile::exists(&self.app.paths.base, name) => self.switch_profile_to(name),
            // Not a profile yet: the list opens on "+ Create …".
            name => self.open_profile_picker(name),
        }
    }

    pub(super) fn pick_profile(&mut self, value: &str) {
        let Some(created) = value.strip_prefix(CREATE) else {
            return self.switch_profile_to(value);
        };
        let (name, from) = created
            .split_once(CREATE)
            .map_or((created, None), |(n, f)| (n, Some(f)));
        if let Err(e) = self.create_profile(name, from) {
            self.error(&e);
        }
    }

    /// Create a profile, with the keys and settings of `from` when given, and switch to it.
    pub(crate) fn create_profile(&mut self, name: &str, from: Option<&str>) -> Result<(), String> {
        if self.busy() {
            return Err(super::BUSY.into());
        }
        let base = self.app.paths.base.clone();
        match from {
            Some(from) => profile::create_from(&base, name, from)?,
            None => profile::create(&base, name)?,
        };
        // Switching restarts rig in the new profile, which starts by connecting a provider.
        self.switch_profile_to(name);
        Ok(())
    }

    fn busy(&self) -> bool {
        self.turn.running.is_some() || self.turn.compacting
    }

    fn switch_profile_to(&mut self, name: &str) {
        if name == self.app.paths.profile {
            return self.say(&format!("Already using profile '{name}'."));
        }
        if self.busy() {
            return self.error(super::BUSY);
        }
        if let Err(e) = profile::set_active(&self.app.paths.base, name) {
            return self.error(&e);
        }
        // An unsent draft stays in this profile's history, one Up away on return.
        let draft = self.prompt.composer.text();
        if !draft.trim().is_empty() {
            self.app.append_history(&draft);
        }
        self.switch_profile = Some(name.to_string());
        self.quit = true;
    }

    fn confirm_remove_profile(&mut self, name: &str) {
        if let Err(e) = profile::check_exists(&self.app.paths.base, name) {
            return self.error(&e);
        }
        if name == profile::DEFAULT || name == self.app.paths.profile {
            return self.error("the default profile and the one in use cannot be removed");
        }
        self.open_picker(
            PickKind::ProfileRemove,
            &format!("Delete profile '{name}' with its logins, keys and sessions?"),
            vec![
                PickItem::new("Keep it", ""),
                PickItem::new(format!("Delete '{name}'"), name).detail("cannot be undone"),
            ],
        );
    }

    pub(super) fn remove_profile(&mut self, name: &str) {
        if name.is_empty() {
            return;
        }
        match profile::remove(&self.app.paths.base, name) {
            Ok(dir) => self.say(&format!("Removed {}", dir.display())),
            Err(e) => self.error(&e),
        }
    }
}
