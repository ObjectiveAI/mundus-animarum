use crate::{Database, Notification};
use std::convert::Infallible;
use std::future::Future;

/// A no-op [`Database`]: every operation succeeds and returns empty data.
///
/// Useful for wiring up the MCP layer and tests before a real backend exists.
#[derive(Debug, Clone, Copy, Default)]
pub struct Mock;

impl Database for Mock {
    type Error = Infallible;

    fn list_keys(
        &self,
        _reader: &str,
        _target: &str,
    ) -> impl Future<Output = Result<Vec<String>, Self::Error>> + Send {
        async { Ok(Vec::new()) }
    }

    fn get_key(
        &self,
        _reader: &str,
        _target: &str,
        _key: &str,
    ) -> impl Future<Output = Result<Option<String>, Self::Error>> + Send {
        async { Ok(None) }
    }

    fn set_key(
        &self,
        _owner: &str,
        _key: &str,
        _value: &str,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async { Ok(()) }
    }

    fn delete_key(
        &self,
        _owner: &str,
        _key: &str,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send {
        // Nothing existed to delete.
        async { Ok(false) }
    }

    fn subscribe_key(
        &self,
        _subscriber: &str,
        _target: &str,
        _key: &str,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async { Ok(()) }
    }

    fn subscribe_soul(
        &self,
        _subscriber: &str,
        _target: &str,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async { Ok(()) }
    }

    fn unsubscribe_key(
        &self,
        _subscriber: &str,
        _target: &str,
        _key: &str,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async { Ok(()) }
    }

    fn unsubscribe_soul(
        &self,
        _subscriber: &str,
        _target: &str,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async { Ok(()) }
    }

    fn notifications(
        &self,
        _subscriber: &str,
    ) -> impl Future<Output = Result<Vec<Notification>, Self::Error>> + Send {
        async { Ok(Vec::new()) }
    }
}
