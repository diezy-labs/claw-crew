---
title: Discord Notifications Setup — galleon-fleet Phase 3 Automation
---

# Discord Notifications for 3-Tier Automation

This guide explains how to set up Discord notifications for the galleon-fleet Phase 3 automation model.

---

## Overview

**What:** Discord notifications for all 3 tiers of automation (routine, critical, strategic).

**Where:** #galleon-fleet channel (or custom channel).

**Cost:** Free (Discord native webhook).

**Coverage:**
- ✅ Tier 1 (Routine auto-approve) → "✅ Auto-approved & merged"
- ✅ Tier 2 (Critical escalation) → "🔴 Lead crew analyzing"
- ✅ Tier 3 (Strategic decision) → Manual post by Lead

---

## Setup Steps

### Step 1: Create Discord Webhook

1. **Open Discord server** → #galleon-fleet channel (or create)
2. **Channel settings** → Integrations → Webhooks
3. **Create Webhook** → Name: "Kiro Galleon Automation"
4. **Copy webhook URL** — looks like: `https://discordapp.com/api/webhooks/123456789/abcdefghijk...`

### Step 2: Add to GitHub Secrets

1. **GitHub repo** → Settings → Secrets and variables → Actions
2. **New secret** → Name: `DISCORD_WEBHOOK_URL`
3. **Value:** Paste the webhook URL from Step 1
4. **Save**

### Step 3: Verify Workflows

The following workflows now post to Discord:

| Workflow | Trigger | Notification |
|----------|---------|---------------|
| `principle-review-trigger.yml` | PR opened to testing | 📋 "New PR submitted for review" |
| `principle-command-parser.yml` | Tier 1 command | ✅ "Routine command spawned" |
| `principle-command-parser.yml` | Tier 2 critical | 🔴 "Critical issue escalated" |
| `tier1-auto-approval-notification.yml` | Principle approves | ✅ "Auto-approved & merged" |

---

## Notification Examples

### Tier 1: Routine Auto-Approved
```
✅ Tier 1 — Routine Auto-Approved & Merged

PR #42 → testing branch
Title: fix(proto): add workspace validation
Status: ✅ Auto-approved (Principle gate pass)
Severity: normal
Next: QA testing → auto-merge to testing (if tests pass)
Timeline: No human review needed
```

### Tier 2: Critical Escalation
```
🔴 Tier 2 — Critical Issue Escalated

PR #40 → Lead Crew Analyzing
Severity: CRITICAL
Trigger: RCE
Issue: Unauthenticated /api/engine/execute endpoint
Next: Lead analysis + Achmad approval
```

### Tier 3: Strategic Decision
```
📋 Tier 3 — Strategic Decision Needed

PR #35 → Phase 3B Timeline
Status: Awaiting Achmad approval
Decision: Move Track B to Phase 3B?
Lead recommendation: Yes, update roadmap
Timeline: User approval needed
```

---

## Testing the Setup

### Test Tier 1 (Normal command)
1. **Create test PR** to testing branch with title "fix: test notification"
2. **Verify Discord notification:** "New PR submitted for review"
3. **Verify GitHub notification:** "Principle Code Review Triggered" comment added

### Test Tier 2 (Critical escalation)
1. **Create test PR** to testing branch with title "fix: test RCE detection"
2. **Comment:** `@squad-backend fix: CRITICAL RCE endpoint vulnerability`
3. **Verify GitHub comment:** "Critical Issue Escalation" posted
4. **Verify Discord notification:** "🔴 Critical Issue Escalated"
5. **Verify webhook:** Lead crew session created

### Test Tier 1 Auto-Approval
1. **Principle crew approves** normal fix PR with comment containing "✅ Approved (auto)"
2. **Verify Discord notification:** "✅ Auto-approved & merged"

---

## Troubleshooting

### Discord notification not sending

**Check:**
1. Is `DISCORD_WEBHOOK_URL` secret set? (GitHub Settings → Secrets)
2. Is webhook URL valid? (Test with `curl` manually)
3. Are workflow permissions correct? (Check workflow file permissions)
4. Check GitHub Actions logs for errors

**Fix:**
```bash
# Manual test (replace URL)
curl -X POST -H "Content-Type: application/json" \
  -d '{"content":"Test notification"}' \
  https://discordapp.com/api/webhooks/YOUR_WEBHOOK_URL
```

### Webhook URL invalid

**Regenerate webhook:**
1. Discord → Channel settings → Integrations → Webhooks
2. Delete old webhook
3. Create new webhook
4. Update `DISCORD_WEBHOOK_URL` secret

### Too many / too few notifications

**Adjust trigger logic** in workflow files:
- `principle-command-parser.yml` — Check `if` conditions for Tier 1/2 detection
- `tier1-auto-approval-notification.yml` — Adjust auto-approval keyword matching

---

## Customization

### Change Discord channel

Instead of using a single webhook, create webhooks for each channel:

| Channel | Purpose | Secret |
|---------|---------|--------|
| #galleon-fleet | All notifications | `DISCORD_WEBHOOK_URL` |
| #galleon-alerts | Critical only | `DISCORD_ALERTS_WEBHOOK_URL` |
| #galleon-approvals | Auto-approval only | `DISCORD_APPROVALS_WEBHOOK_URL` |

**Update workflows** to use appropriate webhook based on tier.

### Customize notification format

Edit the `-d '{...}'` JSON in workflow files:

```yaml
- name: Discord notification
  run: |
    curl -X POST \
      -H "Content-Type: application/json" \
      -d '{
        "content": "Custom message",
        "embeds": [{
          "title": "Custom title",
          "color": 3066993,
          "fields": [
            {"name": "Field 1", "value": "Value 1"}
          ]
        }]
      }' \
      "${{ secrets.DISCORD_WEBHOOK_URL }}"
```

**Discord colors (hex):**
- ✅ Green: `3066993` (#2ecc71)
- 🔴 Red: `15158332` (#e74c3c)
- 📋 Blue: `3447003` (#3498db)
- ⚠️ Orange: `15105570` (#ffa500)

---

## Disabling Notifications

To temporarily disable Discord notifications:

**Option 1:** Comment out the Discord notification step in workflow
```yaml
# - name: Discord notification
#   run: ...
```

**Option 2:** Remove the secret (notifications will skip with `continue-on-error: true`)

**Option 3:** Delete the webhook from Discord (existing workflows will fail gracefully)

---

## Monitoring

**What to watch in Discord:**

1. **Tier 1 (✅ Green):** Routine approvals — should be frequent, low-noise
2. **Tier 2 (🔴 Red):** Critical escalations — should be rare (1-2 per week)
3. **Tier 3 (📋 Blue):** Strategic decisions — manual, high-value decisions

**Success metrics:**
- Tier 1: 0 manual review (fully automated)
- Tier 2: Lead responds within 5 min
- Tier 3: You see decision clearly before approving

---

## FAQ

**Q: Can I mute Discord notifications?**  
A: Yes. Discord → Mute channel / @mention settings. Or set different webhooks for different importance levels.

**Q: What if Discord webhook is down?**  
A: Workflows use `continue-on-error: true`, so GitHub automation continues. Discord notification is best-effort, not critical.

**Q: Can I add more channels?**  
A: Yes. Create multiple webhooks (one per channel) + add secrets to GitHub. Update workflows to post to appropriate webhooks.

**Q: Do I need Discord to use 3-tier automation?**  
A: No. Discord is optional. GitHub notifications (comments, PR status) always work. Discord is for faster visibility + mobile notifications.

---

## Deployment Checklist

- [ ] Discord server created / channel #galleon-fleet ready
- [ ] Webhook URL generated and copied
- [ ] `DISCORD_WEBHOOK_URL` secret added to GitHub
- [ ] Workflows committed and pushed to phase-3-track-ab
- [ ] Manual test: Create PR → verify GitHub + Discord notifications
- [ ] Manual test: Post normal command → verify Tier 1 notification
- [ ] Manual test: Post critical command → verify Tier 2 notification
- [ ] Principle crew knows to use auto-approval comment format
- [ ] Squad watches Discord for notifications
- [ ] Achmad has Discord mobile app for Tier 2/3 decisions

---

## Next Steps

1. **Create Discord server** (if not exists)
2. **Add webhook secret** to GitHub
3. **Test workflows** with dummy PRs
4. **Monitor for 1 week** — adjust notification format/frequency as needed
5. **Scale:** Add more channels for different teams (QA, PO, etc.)

