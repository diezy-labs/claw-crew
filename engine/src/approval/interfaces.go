package approval

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
)

// ActionDigest creates a cryptographic hash of the intended action
type ActionDigest struct {
	PolicyVersion   string `json:"policy_version"`
	Identity        string `json:"identity"` // Fleet/Ship/Crew ID
	ToolName        string `json:"tool_name"`
	TargetResource  string `json:"target_resource"`
	RedactedArgs    string `json:"redacted_args"`
	CredentialScope string `json:"credential_scope"`
}

func (a *ActionDigest) Hash() string {
	payload := fmt.Sprintf("%s:%s:%s:%s:%s:%s",
		a.PolicyVersion, a.Identity, a.ToolName, a.TargetResource, a.RedactedArgs, a.CredentialScope)
	h := sha256.Sum256([]byte(payload))
	return hex.EncodeToString(h[:])
}
