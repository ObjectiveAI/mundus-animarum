use crate::{Database, Remark};
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
        _agent: &str,
    ) -> impl Future<Output = Result<Vec<String>, Self::Error>> + Send {
        async { Ok(Vec::new()) }
    }

    fn get_key(
        &self,
        _agent: &str,
        _key: &str,
    ) -> impl Future<Output = Result<Option<String>, Self::Error>> + Send {
        async { Ok(None) }
    }

    fn set_key(
        &self,
        _agent: &str,
        _key: &str,
        _value: &str,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async { Ok(()) }
    }

    fn delete_key(
        &self,
        _agent: &str,
        _key: &str,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send {
        // Nothing existed to delete.
        async { Ok(false) }
    }

    fn add_remark(
        &self,
        _author: &str,
        _target: &str,
        _key: &str,
        _body: &str,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async { Ok(()) }
    }

    fn list_remarks(
        &self,
        _target: &str,
        _key: &str,
        _offset: u64,
        _count: u32,
        _unread_only: bool,
    ) -> impl Future<Output = Result<Vec<Remark>, Self::Error>> + Send {
        async { Ok(Vec::new()) }
    }

    fn unread_remarks(
        &self,
        _target: &str,
    ) -> impl Future<Output = Result<Vec<(String, u64)>, Self::Error>> + Send {
        async { Ok(Vec::new()) }
    }
}
