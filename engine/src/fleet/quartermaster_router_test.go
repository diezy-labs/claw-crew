package fleet

import "testing"

// TestClassifyIntent verifies F2-2 Layer-1 routing per docs/finalize/01.
func TestClassifyIntent(t *testing.T) {
	cases := []struct {
		prompt string
		want   QuartermasterIntent
	}{
		{"apa itu Galleon?", IntentChat},
		{"berapa RAM engine sekarang?", IntentEngineRoom},
		{"ringkas status semua ship", IntentReport},
		{"siapkan rilis v1.4", IntentObjective},
		{"kerjakan quest baru untuk dev ship", IntentObjective},
		{"halo quartermaster", IntentChat},
	}
	for _, tc := range cases {
		got := ClassifyIntent(tc.prompt)
		if got.Intent != tc.want {
			t.Errorf("ClassifyIntent(%q) = %q, want %q", tc.prompt, got.Intent, tc.want)
		}
		if got.Confidence <= 0 || got.Reason == "" {
			t.Errorf("ClassifyIntent(%q) missing confidence/reason: %+v", tc.prompt, got)
		}
	}
}
