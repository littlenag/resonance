//! Services: `mcp`, the MCP server over stdio (MVP-003).

use resonance_core::error::Result;
use resonance_core::mcp;

use super::{Ctx, engine};

pub fn mcp(ctx: &Ctx) -> Result<()> {
    let mut engine = engine(ctx)?;
    // Paths given by agents are relative to the repository root.
    let root = engine.repo.root.clone();
    engine.set_cwd(root);
    let stdin = std::io::stdin().lock();
    let stdout = std::io::stdout().lock();
    mcp::serve(engine, stdin, stdout)
}
