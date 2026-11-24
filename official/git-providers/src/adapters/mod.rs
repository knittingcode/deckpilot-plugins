//! One adapter per provider, behind [`Provider`].

pub mod github;
pub mod gitlab;

use crate::errors::Failure;
use crate::provider::{ConnectionInfo, Provider, ProviderKind};

/// The adapter for a connection's type.
pub fn for_connection(connection: &ConnectionInfo) -> Result<Box<dyn Provider>, Failure> {
    match connection.kind()? {
        ProviderKind::Gitlab => Ok(Box::new(gitlab::Gitlab::new(connection)?)),
        ProviderKind::Github => Ok(Box::new(github::Github::new(connection)?)),
    }
}
