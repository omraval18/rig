//! `/login`, `/logout`.

use crate::desktop;
use crate::runtime::Ev;
use crate::tui::Tui;
use rig_provider::chatgpt;

impl Tui {
    /// ChatGPT by default; any other provider asks for its key.
    pub(super) fn cmd_login(&mut self, provider: &str) {
        match provider {
            "" | "chatgpt" | "codex" => self.login_chatgpt(),
            name => match self.app.provider_spec(name) {
                Ok(spec) if spec.keyless => self.say(&format!("{name} needs no key.")),
                Ok(_) => self.open_key_entry(name),
                Err(_) => self.open_provider_picker(name),
            },
        }
    }

    pub(super) fn login_chatgpt(&mut self) {
        let path = self.app.paths.chatgpt_auth_file();
        let plan = match chatgpt::LoginPlan::new() {
            Ok(plan) => plan,
            Err(e) => return self.error(&e.to_string()),
        };
        self.say(&format!("Sign in with ChatGPT in your browser (profile '{}'). If it does not open, visit:\n{}\nWaiting up to 5 minutes…", self.app.paths.profile, plan.url));
        desktop::open_url(&plan.url);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = chatgpt::login(&path, &plan, &|_| {})
                .map(|()| format!("Signed in. {}", chatgpt::status(&path).unwrap_or_default()))
                .map_err(|e| e.to_string());
            let _ = tx.send(Ev::LoginDone(r));
        });
    }

    pub(super) fn cmd_logout(&mut self, provider: &str) {
        let name = match provider {
            "" | "codex" => "chatgpt",
            name => name,
        };
        self.disconnect(name);
    }
}
