# Fleet Product Fundamentals
## 1. Executive Summary

Galleon is a **local-first, self-hosted AI organization workspace**.

It allows a user—represented in the product world as the **Pirate King**—to build and operate an AI Fleet using their own model providers, API keys, local models, data, infrastructure, and rules.

The Fleet contains persistent AI Ships. Each Ship is an operational home for one specialist AI team. Ships contain Squads, Crew Members, a Navigator, shared operational memory, a Charter, a defined tool scope, budget rules, and a history of work. A Fleet-level Quartermaster acts as the user’s AI executive: it understands goals, reads operational summaries, tracks provider cost and health, receives reports from Ships, coordinates priorities, and brings important decisions back to the Owner.

The product does not sell model credits as its primary value. Users bring their own intelligence:

```text
Your key.
Your model.
Your infrastructure.
Your Fleet.
Your rules.
```

Galleon provides the operational layer:

```text
Goals
→ Quartermaster
→ Mission Board
→ Ships and Navigators
→ Squads and Crew Specialists
→ Quests and Maps
→ Voyages
→ Artifacts and Discoveries
→ Treasures
→ Owner Decisions
```

The central product promise is:

# Your Fleet. Your Rules.

> A self-hosted AI workspace where Quartermaster coordinates specialist Crew across your Ships, turning SOPs into Quests, work into Artifacts, and approved outcomes into Treasures.

---
## 2. Founder Backstory

Galleon originated from a practical frustration, not from an abstract attempt to build another AI platform.

The founder valued the experience provided by persistent AI workspaces:

- AI that continues work after a chat session ends.
- AI that remembers project context and user corrections.
- AI that can schedule background work and coordinate multiple tasks.
- A desktop/workspace experience that feels alive rather than being only a CLI/runtime.
- A fast, minimal, and capable engineering-oriented product.

Kiro Crew demonstrated much of this product experience. However, usage was connected to a credit system. When credits were exhausted, the workspace could no longer provide the expected work experience, even when the user might have access to their own model accounts, API keys, local models, or infrastructure.

The founder explored alternatives:

| Product family | What was attractive | What was missing |
|---|---|---|
| Kiro Crew | Persistent autonomous work, memory, jobs, workspace experience | Credit-gated access and a constrained provider/economic experience |
| OpenClaw | Self-hosted personal assistant, channels, community, extensibility | Chat/channel-first experience rather than a structured work organization |
| Hermes | Learning loop, memory, skills, delegation | Personal agent/skill-first model rather than persistent specialist teams |
| ZeroClaw | Rust, small footprint, portability, provider flexibility, local-first runtime | The autonomous workspace/team interaction experience desired by the founder |

Galleon therefore combines the desired qualities:

```text
Kiro-like continuity
+ ZeroClaw-like flexibility and lightweight Rust foundation
+ OpenClaw/Hermes-style local ownership and extensibility
+ Persistent specialist team collaboration
+ Owner-controlled governance
```

The goal is not to clone any competitor. It is to give users the persistent AI work experience they want without forcing them into a platform credit gate, single model provider, or opaque cloud dependency.

---
## 3. Product Thesis

### 3.1 Core thesis

> AI work should not stop when platform credits run out.
>
> Users should be able to choose the intelligence, infrastructure, team structure, and operating rules that fit their work.

### 3.2 Product definition

> A local-first AI organization workspace where an Owner builds a Fleet of persistent specialist teams. A Quartermaster coordinates objectives, costs, reports, artifacts, and decisions while the Owner retains final command.

### 3.3 What the product is

```text
A self-hosted AI organization workspace.
A persistent autonomous-work environment.
A Squad and Crew builder.
A governed AI work control plane.
A BYOK/BYOM/BYOI-friendly product layer over user-chosen intelligence.
```

### 3.4 What the product is not

```text
Not a credit reseller.
Not a generic chat assistant only.
Not a container-management product.
Not an enterprise “AI workforce replacement” claim.
Not a no-code workflow builder competing on node count.
Not an autonomous system that acts without user-defined boundaries.
Not a marketplace-first product.
```

---
## 4. Vision, Mission, Principles
## 4.1 Vision

> Every person should be able to operate a capable AI organization that works persistently on their goals—without surrendering control of models, data, infrastructure, cost, or final decisions.
## 4.2 Mission

> Make persistent specialist AI work simple enough for an individual to start locally, while keeping it powerful enough for technical users and teams to operate safely, extend deeply, and grow over time.
## 4.3 Product principles

| Principle | Meaning |
|---|---|
| **Freedom of intelligence** | User chooses model provider, API account, local model, endpoint, and routing strategy |
| **Ownership by default** | User owns data, memory, artifacts, configuration, Squad Charters, and export path |
| **Persistent work** | Work continues through Quests, schedules, events, and approval-resume flows—not only chat sessions |
| **Specialization over generic autonomy** | Different Crew Members have clear jobs, skills, tool scopes, quality gates, and outputs |
| **Autonomy under command** | AI can continue bounded work; Owner retains authority over high-impact actions |
| **Artifact before chat** | Valuable work ends in reviewable evidence, deliverables, or decisions—not only prose in a chat transcript |
| **Learning by consent** | Feedback becomes proposed, scoped, versioned, reversible memory or rules only after approval |
| **Progressive complexity** | New users see goals and outcomes; professionals and engineers can open deep operational controls |
| **Small, fast, capable** | Prefer useful defaults, lightweight local operation, and modular growth over bloated platform complexity |
| **Fair monetization** | Charge for expansion, maintenance, collaboration, and support—not for basic access to user-owned intelligence |

---
