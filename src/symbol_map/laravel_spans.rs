//! The Laravel-specific pieces of string-key spans: config resources, and
//! the side table for gate subjects.

use super::SubjectText;

/// A Laravel resource whose short name is declared under a config subtree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum LaravelConfigResource {
    /// A guard under `auth.guards`.
    AuthGuard,
    /// A cache store under `cache.stores`.
    CacheStore,
    /// A logging channel under `logging.channels`.
    LogChannel,
    /// A filesystem disk under `filesystems.disks`.
    StorageDisk,
    /// A database connection under `database.connections`.
    DatabaseConnection,
    /// A queue connection under `queue.connections`.
    QueueConnection,
    /// A configured mailer under `mail.mailers`.
    Mailer,
    /// A broadcast connection under `broadcasting.connections`.
    BroadcastConnection,
}

impl LaravelConfigResource {
    /// The resource's unique bit in a zero-allocation family-presence set.
    pub(crate) const fn bit(self) -> u8 {
        match self {
            Self::AuthGuard => 1 << 0,
            Self::CacheStore => 1 << 1,
            Self::LogChannel => 1 << 2,
            Self::StorageDisk => 1 << 3,
            Self::DatabaseConnection => 1 << 4,
            Self::QueueConnection => 1 << 5,
            Self::Mailer => 1 << 6,
            Self::BroadcastConnection => 1 << 7,
        }
    }
}

/// The model a gate check names, recorded alongside its ability span.
///
/// Kept in a side table rather than inside the ability's
/// [`StringKey`](super::SymbolKind::StringKey) span so that the (very hot)
/// [`SymbolKind`](super::SymbolKind) enum does not grow for every span in
/// every file: only the handful of gate calls in a project contribute an
/// entry, and only the unknown-ability diagnostic reads them.
#[derive(Debug, Clone)]
pub(crate) struct GateSubject {
    /// Start offset of the ability span this describes.
    pub ability_start: u32,
    /// Source text of the model expression — `Post` for `Post::class`,
    /// `$post` for a variable.
    pub subject_text: SubjectText,
    /// Whether the subject is a class name (`Post::class`) rather than a
    /// runtime value, which decides how it is resolved to a type.
    pub is_static: bool,
}
