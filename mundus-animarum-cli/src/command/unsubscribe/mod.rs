//! `unsubscribe` — stop watching another agent's soul: a single `--key` or
//! the whole key set (`--keys`).

use clap::Args as ClapArgs;
use mundus_animarum_db::Scope;

use crate::command::subscription::SubscriptionArgs;
use crate::context::Context;
use crate::error::Error;

#[derive(Debug, ClapArgs)]
pub struct Args {
    #[command(flatten)]
    pub subscription: SubscriptionArgs,
}

impl Args {
    pub async fn run(self, ctx: &Context) -> Result<serde_json::Value, Error> {
        let r = self.subscription.resolve(ctx);
        let db = ctx.db().await?;
        match r.scope {
            Scope::Key(key) => db.unsubscribe_key(&r.caller, &r.target, &key).await?,
            Scope::Soul => db.unsubscribe_soul(&r.caller, &r.target).await?,
        }
        Ok(serde_json::Value::Null)
    }
}
