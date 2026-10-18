//! The change log (FR-274–FR-278, MVP-040): a concise, local log of people's changes to spec
//! items, which agents read from a remembered sequence number and clear as they handle them.

use crate::error::Result;
use crate::store::GraphRead;

/// Metadata key prefix of change-log entries: `change:<seq, zero-padded>`.
pub const PREFIX: &str = "change:";

/// The number of pending entries.
pub fn pending_count(r: &dyn GraphRead) -> Result<usize> {
    Ok(r.meta_prefix(PREFIX)?.len())
}
