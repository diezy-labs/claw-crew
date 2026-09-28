package tool

import (
	"fmt"
	"strings"
	"sync"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
)

// MemoryRegistry manages tools in-memory with thread-safety and namespace resolution
type MemoryRegistry struct {
	mu      sync.RWMutex
	tools   map[string]Tool
	aliases map[string]string
}

// NewRegistry creates a new tool registry
func NewRegistry() Registry {
	r := &MemoryRegistry{
		tools:   make(map[string]Tool),
		aliases: make(map[string]string),
	}
	registerBuiltinTools(r)
	return r
}

// Register adds a tool and indexes its namespaced ID and short name alias
func (r *MemoryRegistry) Register(t Tool) {
	if t == nil {
		return
	}
	r.mu.Lock()
	defer r.mu.Unlock()

	name := t.Name()
	r.tools[name] = t

	// Index short name or namespaced alias
	if strings.Contains(name, ".") {
		parts := strings.Split(name, ".")
		shortName := parts[len(parts)-1]
		if _, exists := r.tools[shortName]; !exists {
			r.aliases[shortName] = name
		}
	}
}

// Get finds a tool by name, checking exact match then alias
func (r *MemoryRegistry) Get(name string) (Tool, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()

	if t, ok := r.tools[name]; ok {
		return t, nil
	}

	if target, ok := r.aliases[name]; ok {
		if t, ok := r.tools[target]; ok {
			return t, nil
		}
	}

	return nil, appErrors.New(appErrors.CodeNotFound, fmt.Sprintf("tool not found: %s", name), appErrors.LayerService)
}

// List returns all registered tools
func (r *MemoryRegistry) List() []Tool {
	r.mu.RLock()
	defer r.mu.RUnlock()

	list := make([]Tool, 0, len(r.tools))
	for _, t := range r.tools {
		list = append(list, t)
	}
	return list
}

// ListByScope filters tools by workspace scope and allowed capabilities
func (r *MemoryRegistry) ListByScope(workspaceID string, allowedCapabilities []string) []Tool {
	r.mu.RLock()
	defer r.mu.RUnlock()

	if len(allowedCapabilities) == 0 {
		return r.List()
	}

	hasWildcard := false
	capMap := make(map[string]bool, len(allowedCapabilities))
	for _, c := range allowedCapabilities {
		if c == "*" {
			hasWildcard = true
			break
		}
		capMap[strings.ToLower(c)] = true
	}

	if hasWildcard {
		return r.List()
	}

	filtered := make([]Tool, 0)
	for _, t := range r.tools {
		def := t.Definition()
		if def == nil || len(def.Capabilities) == 0 {
			filtered = append(filtered, t)
			continue
		}

		matched := true
		for _, req := range def.Capabilities {
			reqLower := strings.ToLower(req)
			if !capMap[reqLower] {
				parts := strings.Split(reqLower, ".")
				if len(parts) > 1 && capMap[parts[0]+".*"] {
					continue
				}
				matched = false
				break
			}
		}

		if matched {
			filtered = append(filtered, t)
		}
	}

	return filtered
}
