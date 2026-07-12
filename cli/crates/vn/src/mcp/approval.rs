//! A shared gate that destructive MCP tools (currently just `stop_app`) call
//! through before acting, so an LLM — local (Ollama) or a connected external
//! MCP client — can request a destructive action but a human approves it.
//!
//! When the TUI is running, [`ApprovalGate::new`] pairs with a receiver the
//! TUI drains every frame, arming the existing confirm-input UI. When there is
//! no TUI attached (headless `vn mcp serve`), [`ApprovalGate::headless`]
//! auto-denies every request: there is no console free to prompt on (stdio is
//! the MCP protocol channel itself), so failing closed is the only sound
//! default. Run destructive-capable tools from inside the TUI to approve them
//! interactively.

use tokio::sync::{mpsc, oneshot};

/// One pending approval request, shown to the user and answered via `respond`.
pub struct PendingApproval {
    pub description: String,
    pub respond: oneshot::Sender<bool>,
}

#[derive(Clone)]
pub struct ApprovalGate {
    tx: mpsc::UnboundedSender<PendingApproval>,
}

impl ApprovalGate {
    /// Create a gate paired with a receiver for an interactive approver (the
    /// TUI) to drain and answer.
    pub fn new() -> (Self, mpsc::UnboundedReceiver<PendingApproval>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { tx }, rx)
    }

    /// Create a gate with no interactive approver attached: every request is
    /// auto-denied (fail-closed).
    pub fn headless() -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<PendingApproval>();
        tokio::spawn(async move {
            while let Some(pending) = rx.recv().await {
                let _ = pending.respond.send(false);
            }
        });
        Self { tx }
    }

    /// Request approval for `description`; resolves to `true` if approved,
    /// `false` if denied or the approver is gone.
    pub async fn request(&self, description: String) -> bool {
        let (respond, receive) = oneshot::channel();
        if self
            .tx
            .send(PendingApproval {
                description,
                respond,
            })
            .is_err()
        {
            return false;
        }
        receive.await.unwrap_or(false)
    }
}
