## 2. Existing Repository Assessment

## 2.1 Relevant existing frontend surface

The existing web app already exposes many useful concepts through pages including:

```text
Dashboard
Agent Chat
Chat Workspace
Agents List
Task Board
Runs / Run Detail
Approvals
Audit
Config
Cron
SOPs / SOP Canvas
Tools
Skills
Integrations
Apps
Instances
Logs
Metrics
Doctor
Recovery
Provider Health
Session Health
Pairing
ACP Console
```

The current application shell is React/TypeScript/Vite-based, and the desktop application is built around Tauri with a daemon, gateway client, health checks, commands, and state. The Go engine adds streaming, memory query, multi-agent orchestration, tool dispatch, Prometheus metrics, tracing/interceptors, and clean-architecture domains. This means the product has sufficient technical primitives to support an advanced desktop control experience without inventing a second UI/runtime model.

## 2.2 Key UX problem

The current surface is capability-rich but navigation-heavy:

```text
Current mental model:
Agent / Config / Cron / Tools / Logs / Metrics / SOP / Audit / Health

Target mental model:
Owner objective → Quartermaster → Quest → Ship → Artifact → Decision
```

The existing pages should not be deleted wholesale. They should be reorganized through progressive disclosure:

| Existing page/capability | New product surface | Visibility |
|---|---|---|
| AgentChat / ChatWorkspace | Quartermaster Office | Primary |
| Dashboard | Quartermaster Office + Fleet Overview | Primary |
| AgentsList | Crew Members | Primary when managing a Ship |
| TaskBoard | Mission Board | Primary |
| Runs / RunDetail | Voyage History / Voyage Detail | Secondary-primary |
| Approvals | Captain’s Approval | Primary when pending |
| Audit | Logbook | Secondary-primary |
| SOPs / SopCanvas | Quest Maps / Map Studio | Primary for advanced workflow editing |
| Cron | Quest Schedule | Secondary |
| Tools / Skills | Ship Capabilities | Advanced / contextual |
| Integrations / Apps / Pairing | Harbor | Secondary |
| Config / ProvidersHealth | Fleet Settings / Treasury / Provider Health | Advanced / contextual |
| Logs / Metrics / SessionsHealth / Doctor / Recovery | Crow’s Nest | Advanced technical control |
| Instances / ACP Console | Fleet Control / Developer Console | Advanced technical control |

## 2.3 Design implication

The product needs an **information architecture refactor**, not merely a visual refresh. The main goal is to make the rich existing functionality feel coherent under the Fleet fundamental model.

---

