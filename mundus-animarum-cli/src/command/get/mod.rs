//! `get` — read the value of a single key from an agent's soul.

use clap::Args as ClapArgs;

use crate::context::Context;
use crate::error::Error;
use crate::run::Config;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// The soul key to read.
    #[arg(long)]
    pub key: String,
    /// Full id of the agent whose soul to read. Falls back to the
    /// configured `OBJECTIVEAI_AGENT_FULL_ID` when omitted.
    #[arg(long)]
    pub agent_full_id: Option<String>,
}

impl Args {
    pub async fn run(self, ctx: &Context) -> Result<serde_json::Value, Error> {
        let agent = resolve_agent_full_id(self.agent_full_id, &ctx.config)?;
        let db = ctx.db().await?;
        // Reading your own soul: reader and target are the same agent.
        // The value is returned as a JSON string; an unset key is null.
        let value = db.get_key(&agent, &agent, &self.key).await?;
        Ok(value.map_or(serde_json::Value::Null, serde_json::Value::String))
    }
}

/// Resolve the agent full id: the explicit `--agent-full-id` if given,
/// otherwise the configured one. Errors when neither is available.
fn resolve_agent_full_id(arg: Option<String>, cfg: &Config) -> Result<String, Error> {
    arg.or_else(|| cfg.objectiveai_agent_full_id.clone())
        .ok_or(Error::AgentFullIdRequired)
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

    #[test]
    fn arg_takes_precedence() {
        let got = resolve_agent_full_id(Some("explicit".into()), &cfg(Some("from-config"))).unwrap();
        assert_eq!(got, "explicit");
    }

    #[test]
    fn falls_back_to_config() {
        let got = resolve_agent_full_id(None, &cfg(Some("from-config"))).unwrap();
        assert_eq!(got, "from-config");
    }

    #[test]
    fn errors_when_neither_set() {
        let err = resolve_agent_full_id(None, &cfg(None)).unwrap_err();
        assert!(matches!(err, Error::AgentFullIdRequired));
    }
}
