//! Assembling an agent and its session from config and flags.

use crate::config::default_model;
use rig_core::agent::Agent;
use rig_core::permission::Policy;
use rig_core::prompt::SYSTEM_PROMPT;
use rig_core::provider::Effort;
use rig_core::session::{Session, archive_path};
use rig_core::skills::Skill;
use rig_core::sync::{MutexExt, RwLockExt};
use rig_core::tool::ToolCtx;
use rig_provider::ProviderSpec;

pub struct Started {
    pub agent: Agent,
    pub session: Session,
    pub spec: ProviderSpec,
    pub model: String,
    pub skills: Vec<Skill>,
}

use super::{App, RunOpts};

impl App {
    /// `interactive` lets the app open without credentials so the user can `/login` from inside it;
    /// scripts (`rig ask`) still fail fast.
    pub fn start(&self, opts: &RunOpts, interactive: bool) -> Result<Started, String> {
        let existing = if let Some(id) = &opts.resume {
            Some(
                Session::find(&self.paths.root, &self.cwd, id)
                    .ok_or_else(|| format!("no unique session matching '{id}' in this project"))?,
            )
        } else if opts.cont {
            Some(
                Session::latest(&self.paths.root, &self.cwd)
                    .ok_or("no earlier session in this project")?,
            )
        } else {
            None
        };
        // A resumed session comes back as it was (flags still win); every session keeps its own settings.
        let own = existing
            .as_deref()
            .map(Session::settings)
            .unwrap_or_default();
        let model_flag = opts.model.clone().or(own.model);
        let effort_flag = opts.effort.clone().or(own.effort);
        let auto_flag = opts.auto.clone().or(own.autonomy.filter(|_| interactive));
        let (spec, model) = match self.resolve_model(model_flag.as_deref()) {
            Ok(m) => m,
            Err(_) if interactive => (
                self.provider_spec("chatgpt")?,
                default_model("chatgpt").unwrap_or_default().to_string(),
            ),
            Err(e) => return Err(e),
        };
        let provider = match self.build_provider(&spec, &model) {
            Ok(p) => p,
            Err(_) if interactive => self.unauthenticated(&spec, &model),
            Err(e) => return Err(e),
        };
        let label = format!("{}/{}", spec.name, model);
        let skills = self.discover_skills();

        let (session, history) = match existing {
            Some(path) => {
                let history = Session::load(&path)
                    .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
                (Session::reopen(&path).map_err(|e| e.to_string())?, history)
            }
            None => {
                let mut s = Session::create(&self.paths.root, &self.cwd, &label)
                    .map_err(|e| format!("cannot create a session: {e}"))?;
                let reminder = self.reminder(&skills);
                s.append(&reminder).map_err(|e| e.to_string())?;
                (s, vec![reminder])
            }
        };

        let ctx = ToolCtx::new(self.cwd.clone(), session.id.clone());
        *ctx.shared.archive.locked() = Some(archive_path(&session.path));
        *ctx.shared.skills.write_locked() = skills.clone();
        let policy = Policy::new(self.autonomy(auto_flag.as_deref(), interactive)?);
        policy.set_allowlist(self.load_allowlist());
        let mut agent = Agent::new(
            provider,
            rig_tools::builtin().with(rig_mcp::tools(self.mcp.clone())),
            policy,
            SYSTEM_PROMPT.to_string(),
            ctx,
            history,
        );
        agent.effort = self.effort(effort_flag.as_deref())?;
        agent.max_turns = self.cfg.max_turns.unwrap_or(100);
        agent.context_window = self.context_window(&spec, None);
        agent.auto_compact_percent = self.auto_compact_percent();
        let mut session = session;
        if opts.resume.is_none() && !opts.cont {
            // A new session records where it starts, so resuming it later restores the same setup.
            let _ = session.append_settings(
                &label,
                agent.effort.map(Effort::as_str),
                agent.policy.autonomy().as_str(),
            );
        }
        Ok(Started {
            agent,
            session,
            spec,
            model,
            skills,
        })
    }
}
