//! Verification: `test <ref>` runs linked tests and records evidence (FR-135).

use resonance_core::error::{Error, Result};

use super::{Ctx, reconciled};
use crate::render;

pub fn test(ctx: &Ctx, reference: &str) -> Result<()> {
    let (mut engine, _) = reconciled(ctx)?;
    let report = engine.run_tests(reference)?;
    ctx.emit(&report, render::test_report)?;
    if report.failed + report.errors > 0 {
        // The evidence is recorded; the exit code tells scripts that not everything passed.
        return Err(Error::Invalid(format!(
            "{} of {} test runs did not pass",
            report.failed + report.errors,
            report.runs.len()
        )));
    }
    Ok(())
}
