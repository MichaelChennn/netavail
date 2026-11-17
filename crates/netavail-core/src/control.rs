//! Hooks the adapter layer uses to cancel, time and limit a computation (architecture.md §3.5; D29).

use crate::types::NodeId;

/// Returned by [`Control::check`] when the user cancelled; `?` turns it into `Error::Cancelled`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cancelled;

/// One stage of the computation for a node pair.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Paths,
    Cuts,
    Prepare,
    Evaluate,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StageEvent {
    Started,
    Finished,
}

/// Implemented by the CLI and the browser; every method has a do-nothing default.
pub trait Control: Sync {
    /// Called by algorithms in their loops; returns `Err` once the user cancels.
    fn check(&self) -> Result<(), Cancelled> {
        Ok(())
    }

    /// Called by the pipeline around each stage, e.g. to measure its time.
    fn on_stage(&self, _src: NodeId, _dst: NodeId, _stage: Stage, _event: StageEvent) {}

    /// Largest number of sets an algorithm may produce; `None` means no limit.
    fn max_sets(&self) -> Option<usize> {
        None
    }
}

/// A [`Control`] that never cancels, records nothing and sets no limit.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoControl;

impl Control for NoControl {}
