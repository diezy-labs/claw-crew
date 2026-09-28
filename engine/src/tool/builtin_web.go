package tool

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"regexp"
	"strings"
	"time"

	appErrors "github.com/diezy-labs/claw-crew/engine/core/errors"
)

// WebFetchTool implements web.fetch with robust SSRF protections
type WebFetchTool struct {
	client *http.Client
}

// NewWebFetchTool creates a new WebFetchTool with SSRF-safe HTTP client
func NewWebFetchTool() *WebFetchTool {
	dialer := &net.Dialer{
		Timeout:   10 * time.Second,
		KeepAlive: 15 * time.Second,
	}

	transport := &http.Transport{
		DialContext: func(ctx context.Context, network, addr string) (net.Conn, error) {
			host, port, err := net.SplitHostPort(addr)
			if err != nil {
				return nil, fmt.Errorf("invalid address: %w", err)
			}

			ips, err := net.DefaultResolver.LookupIP(ctx, "ip", host)
			if err != nil {
				return nil, fmt.Errorf("dns lookup failed for %s: %w", host, err)
			}

			if len(ips) == 0 {
				return nil, fmt.Errorf("no ip addresses resolved for host %s", host)
			}

			// Validate all resolved IPs against SSRF filter
			for _, ip := range ips {
				if err := ValidateSSRFSafeIP(ip); err != nil {
					return nil, err
				}
			}

			targetAddr := net.JoinHostPort(ips[0].String(), port)
			return dialer.DialContext(ctx, network, targetAddr)
		},
		ResponseHeaderTimeout: 15 * time.Second,
		MaxIdleConns:          20,
		IdleConnTimeout:       30 * time.Second,
	}

	return &WebFetchTool{
		client: &http.Client{
			Transport: transport,
			Timeout:   20 * time.Second,
			CheckRedirect: func(req *http.Request, via []*http.Request) error {
				if len(via) >= 5 {
					return fmt.Errorf("stopped after 5 redirects")
				}
				// Verify redirect URL is safe
				return ValidateSSRFURL(req.URL)
			},
		},
	}
}

func (t *WebFetchTool) Name() string { return "web.fetch" }
func (t *WebFetchTool) Description() string {
	return "Safely fetches web documentation or pages with SSRF and script stripping guards"
}
func (t *WebFetchTool) RiskTier() RiskTier { return RiskTierRead }
func (t *WebFetchTool) Definition() *ToolDefinition {
	return &ToolDefinition{
		ID:               t.Name(),
		Version:          "1.0.0",
		DisplayName:      "Fetch Web Resource",
		Description:      t.Description(),
		RiskTier:         RiskTierRead,
		RiskClass:        RiskClassNetworkRead,
		Capabilities:     []string{"network.read", "network.egress"},
		RequiresApproval: false,
		TimeoutSeconds:   20,
		MaxOutputBytes:   100000,
		IdempotencyMode:  IdempotencySafe,
		Source:           "native",
		InputSchema: json.RawMessage(`{
			"type": "object",
			"properties": {
				"url": {"type": "string", "description": "Target HTTP or HTTPS URL"},
				"max_bytes": {"type": "integer", "default": 100000, "maximum": 500000}
			},
			"required": ["url"]
		}`),
	}
}

func (t *WebFetchTool) Execute(ctx context.Context, args string, workspaceRoot string) (string, error) {
	out, _, err := t.ExecuteWithContext(ctx, nil, args)
	return out, err
}

func (t *WebFetchTool) ExecuteWithContext(ctx context.Context, execCtx *ExecutionContext, args string) (string, []string, error) {
	var input struct {
		URL      string `json:"url"`
		MaxBytes int64  `json:"max_bytes"`
	}
	if err := json.Unmarshal([]byte(args), &input); err != nil {
		return "", nil, appErrors.Wrap(err, appErrors.CodeInvalidArgument, "invalid web.fetch arguments", appErrors.LayerService)
	}

	targetURL, err := url.Parse(strings.TrimSpace(input.URL))
	if err != nil {
		return "", nil, appErrors.Wrap(err, appErrors.CodeInvalidArgument, "invalid url format", appErrors.LayerService)
	}

	if err := ValidateSSRFURL(targetURL); err != nil {
		return "", nil, err
	}

	if input.MaxBytes <= 0 || input.MaxBytes > 500000 {
		input.MaxBytes = 100000
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, targetURL.String(), nil)
	if err != nil {
		return "", nil, err
	}
	req.Header.Set("User-Agent", "ClawCrew-Engine-Bot/1.0 (+https://clawcrew.dev)")
	req.Header.Set("Accept", "text/html,application/xhtml+xml,application/xml,text/plain")

	resp, err := t.client.Do(req)
	if err != nil {
		return "", nil, appErrors.New(appErrors.CodeToolFailed, fmt.Sprintf("web fetch error: %v", err), appErrors.LayerExternal)
	}
	defer resp.Body.Close()

	if resp.StatusCode >= 400 {
		return "", nil, appErrors.New(appErrors.CodeToolFailed, fmt.Sprintf("http request failed with status: %d", resp.StatusCode), appErrors.LayerExternal)
	}

	reader := io.LimitReader(resp.Body, input.MaxBytes+1)
	bodyBytes, err := io.ReadAll(reader)
	if err != nil {
		return "", nil, fmt.Errorf("failed reading response body: %w", err)
	}

	truncated := false
	if int64(len(bodyBytes)) > input.MaxBytes {
		bodyBytes = bodyBytes[:input.MaxBytes]
		truncated = true
	}

	sanitized := sanitizeHTMLContent(string(bodyBytes))

	type webResult struct {
		URL       string `json:"url"`
		Status    int    `json:"status"`
		Content   string `json:"content"`
		Truncated bool   `json:"truncated"`
		Evidence  string `json:"evidence_classification"`
	}

	res := webResult{
		URL:       targetURL.String(),
		Status:    resp.StatusCode,
		Content:   sanitized,
		Truncated: truncated,
		Evidence:  "untrusted_external_evidence",
	}

	resBytes, err := json.Marshal(res)
	if err != nil {
		return sanitized, nil, nil
	}
	return string(resBytes), nil, nil
}

// ValidateSSRFURL verifies URL scheme and host restrictions
func ValidateSSRFURL(u *url.URL) error {
	scheme := strings.ToLower(u.Scheme)
	if scheme != "http" && scheme != "https" {
		return appErrors.New(appErrors.CodePermissionDenied, fmt.Sprintf("forbidden scheme: %s (only http and https allowed)", scheme), appErrors.LayerService)
	}

	hostname := strings.ToLower(u.Hostname())
	if hostname == "" {
		return appErrors.New(appErrors.CodeInvalidArgument, "empty host in url", appErrors.LayerService)
	}

	// Immediate string blocks for metadata, localhost, and loopback
	if hostname == "localhost" || hostname == "127.0.0.1" || hostname == "::1" ||
		hostname == "169.254.169.254" || strings.HasSuffix(hostname, ".internal") ||
		strings.HasSuffix(hostname, ".local") {
		return appErrors.New(appErrors.CodePermissionDenied, fmt.Sprintf("SSRF protection: access to %s is prohibited", hostname), appErrors.LayerService)
	}

	// If hostname is an IP literal, validate directly
	if ip := net.ParseIP(hostname); ip != nil {
		return ValidateSSRFSafeIP(ip)
	}

	return nil
}

// ValidateSSRFSafeIP verifies that an IP does not reside in private, loopback, link-local, or cloud metadata ranges
func ValidateSSRFSafeIP(ip net.IP) error {
	if ip.IsLoopback() {
		return appErrors.New(appErrors.CodePermissionDenied, fmt.Sprintf("SSRF blocked: loopback ip %s", ip.String()), appErrors.LayerService)
	}
	if ip.IsPrivate() {
		return appErrors.New(appErrors.CodePermissionDenied, fmt.Sprintf("SSRF blocked: private network ip %s", ip.String()), appErrors.LayerService)
	}
	if ip.IsLinkLocalUnicast() || ip.IsLinkLocalMulticast() {
		return appErrors.New(appErrors.CodePermissionDenied, fmt.Sprintf("SSRF blocked: link-local ip %s", ip.String()), appErrors.LayerService)
	}
	if ip.IsUnspecified() || ip.IsMulticast() {
		return appErrors.New(appErrors.CodePermissionDenied, fmt.Sprintf("SSRF blocked: invalid ip %s", ip.String()), appErrors.LayerService)
	}

	// Explicit check for AWS/GCP/Azure link-local metadata IP (169.254.169.254)
	if ip.String() == "169.254.169.254" {
		return appErrors.New(appErrors.CodePermissionDenied, "SSRF blocked: cloud metadata endpoint", appErrors.LayerService)
	}

	return nil
}

var (
	scriptTagRegex = regexp.MustCompile(`(?is)<script.*?>.*?</script>`)
	styleTagRegex  = regexp.MustCompile(`(?is)<style.*?>.*?</style>`)
	tagRegex       = regexp.MustCompile(`(?is)<[a-zA-Z\/][^>]*>`)
	spaceRegex     = regexp.MustCompile(`\s{2,}`)
)

// sanitizeHTMLContent strips active script/style tags and decodes text for LLM evidence
func sanitizeHTMLContent(html string) string {
	cleaned := scriptTagRegex.ReplaceAllString(html, "")
	cleaned = styleTagRegex.ReplaceAllString(cleaned, "")
	cleaned = tagRegex.ReplaceAllString(cleaned, " ")
	cleaned = spaceRegex.ReplaceAllString(cleaned, " ")
	return strings.TrimSpace(cleaned)
}
