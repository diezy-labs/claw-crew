// Package treasury reports provider cost (BYOK/BYOM) read-only.
//
// INVARIANT F1 (see .kiro/steering/galleon-product-fundamentals.md): Treasury
// REPORTS cost, it never deducts. No internal credit balance debited per
// inference/token; no markup over provider price. Budget caps gate actions via
// approval, they do not spend a balance. Timber = organizational capacity, not
// an inference-consumption meter. Any type/method added here must satisfy this.
package treasury

// TODO: Define treasury domain interfaces (must honor INVARIANT F1 above).
