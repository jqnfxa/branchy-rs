//! Opaque identifiers.
//!
//! Both are newtypes over `u64` rather than bare integers, so a `NodeId` can
//! never be passed where an `AreaId` is expected, and so the representation can
//! change later (to something randomly generated, once two devices sync)
//! without touching a single call site.

/// Identifies a node within one [`Graph`](crate::Graph).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(u64);

/// Identifies an area within one [`Graph`](crate::Graph).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AreaId(u64);

impl NodeId {
    /// Wraps a raw value. Ids are handed out by the graph; construct one
    /// yourself only when reading a stored document back in.
    #[must_use]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// The underlying value, for storage and display.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

impl AreaId {
    /// Wraps a raw value. Ids are handed out by the graph; construct one
    /// yourself only when reading a stored document back in.
    #[must_use]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// The underlying value, for storage and display.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "n{}", self.0)
    }
}

impl std::fmt::Display for AreaId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "a{}", self.0)
    }
}
