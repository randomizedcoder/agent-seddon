//! The planner (`docs/design/campaigns/03-decomposition.md`): for one `ready`
//! non-leaf node, build the prompt, ask the model for one structured decision,
//! validate it fail-closed, and write the outcome through the [`CampaignStore`]
//! seam — one attempt row inside the finishing transaction.
//!
//! Modules, in the order `plan_node` uses them:
//!
//! * [`hash`] — `prompt_hash` and `idem_key`.
//! * [`schema`] — the decision schema and its per-depth `decision` enum.
//!
//! The model is untrusted: every input it wrote earlier (goals, titles) is screened
//! before it enters a prompt, and every field it answers with is capped, screened
//! and resolved before a store call.

pub mod hash;
pub mod schema;
