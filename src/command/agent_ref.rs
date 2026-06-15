//! Shared `--agent-full-id` selector, flattened into every soul command.

use clap::Args;

use crate::error::Error;
use crate::run::Config;

/// The agent whose soul a command acts on, selected by `--agent-full-id`.
/// Resolves to the agent's full id: the explicit flag when given, otherwise
/// the configured `OBJECTIVEAI_AGENT_FULL_ID`. Errors when neither is set.
#[derive(Debug, Args)]
pub struct AgentRef {
    /// Full id of the agent whose soul to act on. Falls back to the
    /// configured `OBJECTIVEAI_AGENT_FULL_ID` when omitted.
    #[arg(long)]
    pub agent_full_id: Option<String>,
}

impl AgentRef {
    /// Resolve the agent full id, or fail with [`Error::AgentFullIdRequired`].
    pub fn resolve(&self, cfg: &Config) -> Result<String, Error> {
        self.agent_full_id
            .clone()
            .or_else(|| cfg.objectiveai_agent_full_id.clone())
            .ok_or(Error::AgentFullIdRequired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(full_id: Option<&str>) -> Config {
        Config {
            objectiveai_agent_full_id: full_id.map(Into::into),
            ..Config::default()
        }
    }

    fn agent(arg: Option<&str>) -> AgentRef {
        AgentRef {
            agent_full_id: arg.map(Into::into),
        }
    }

    #[test]
    fn arg_takes_precedence() {
        assert_eq!(
            agent(Some("explicit")).resolve(&cfg(Some("from-config"))).unwrap(),
            "explicit"
        );
    }

    #[test]
    fn falls_back_to_config() {
        assert_eq!(agent(None).resolve(&cfg(Some("from-config"))).unwrap(), "from-config");
    }

    #[test]
    fn errors_when_neither_set() {
        assert!(matches!(
            agent(None).resolve(&cfg(None)).unwrap_err(),
            Error::AgentFullIdRequired
        ));
    }
}
