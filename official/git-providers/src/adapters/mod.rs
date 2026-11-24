//! One adapter per provider, behind [`Provider`].

pub mod gitlab;

use crate::errors::{Failure, codes, fail};
use crate::provider::{ConnectionInfo, Provider, ProviderKind};

/// The adapter for a connection's type.
pub fn for_connection(connection: &ConnectionInfo) -> Result<Box<dyn Provider>, Failure> {
    match connection.kind()? {
        ProviderKind::Gitlab => Ok(Box::new(gitlab::Gitlab::new(connection)?)),
        ProviderKind::Github => Err(fail(
            codes::CAPABILITY_UNSUPPORTED,
            "GitHub connections are not supported by this version of the plugin yet",
        )),
    }
}
