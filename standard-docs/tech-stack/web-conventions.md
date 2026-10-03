# Web Conventions (React 19 + TypeScript)

**Status:** Normative (code MUST follow this)  
**Last updated:** 2026-10-03  
**Stack:** React 19, TypeScript 5.7+, Vite 6, Tailwind CSS 4

---

## Project Structure

```
web-2/
├── src/
│   ├── components/          # React components
│   ├── lib/
│   │   ├── apiClient.ts     # HTTP client for Go engine
│   │   └── tauri.ts         # Tauri IPC client
│   ├── hooks/               # Custom React hooks
│   ├── pages/               # Page-level components (routing)
│   ├── types/               # TypeScript type definitions
│   ├── App.tsx              # Root component
│   └── main.tsx             # Entrypoint (React.StrictMode)
├── public/                  # Static assets (icons, images)
├── server.ts                # Express dev server (TEMP ADR 0002 violations)
├── vite.config.ts           # Vite config (dev server, build)
├── tailwind.config.js       # Tailwind config (colors, theme)
└── package.json
```

**Convention:** `src/` = frontend-only (React components, HTTP client), `server.ts` = temp dev server (should be removed per ADR 0002).

---

## Zero Backend Logic Policy (ADR 0002)

### Rule: UI Tier = Rendering Only

**UI owns:**
- React components (view rendering, Tailwind styling)
- Client-side routing (React Router)
- HTTP client (`apiClient.ts`)
- View models (transform API responses for display)

**UI does NOT own:**
- Backend logic (HTTP handlers, business rules)
- QR code generation (should be Go HTTP endpoint)
- OS telemetry (should be Go HTTP endpoint)
- Secret management (never in frontend)

### ✅ Correct: Call Go API

```typescript
// src/lib/apiClient.ts
export async function getFleetMetrics(): Promise<FleetMetrics> {
  const response = await fetch('http://localhost:9090/api/system/metrics');
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return response.json();
}
```

### ❌ Wrong: Backend Logic in Express (ADR 0002 Violation)

```typescript
// server.ts — TEMPORARY, should be removed
app.get('/api/network/qrcode', (req, res) => {
  const qr = qrcode.generate(req.query.data); // Business logic in UI tier
  res.send(qr);
});
```

**Status:** Tracked in ADR 0002 as "next-phase work" (move to Go `/api/network/qrcode` + Nginx auto-start).

---

## HTTP Client (`apiClient.ts`)

### Base Client

```typescript
// src/lib/apiClient.ts
const BASE_URL = 'http://localhost:9090';

async function request<T>(path: string, options?: RequestInit): Promise<T> {
  const response = await fetch(`${BASE_URL}${path}`, {
    ...options,
    headers: {
      'Content-Type': 'application/json',
      ...options?.headers,
    },
  });
  
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${await response.text()}`);
  }
  
  return response.json();
}

export const apiClient = {
  get: <T>(path: string) => request<T>(path),
  post: <T>(path: string, body: unknown) =>
    request<T>(path, { method: 'POST', body: JSON.stringify(body) }),
};
```

**Conventions:**
- Base URL = `http://localhost:9090` (Go engine)
- Throw on non-2xx (let React error boundary catch)
- Type-safe with generics (`request<T>`)

### API Functions

```typescript
// src/lib/api/fleet.ts
import { apiClient } from '../apiClient';

export interface Crew {
  id: string;
  name: string;
  risk_tier: 'low' | 'medium' | 'high' | 'critical';
}

export async function listCrews(): Promise<Crew[]> {
  return apiClient.get<Crew[]>('/api/fleet/crews');
}

export async function getCrew(id: string): Promise<Crew> {
  return apiClient.get<Crew>(`/api/fleet/crews/${id}`);
}

export async function startCrewTurn(id: string, task: string): Promise<TurnResponse> {
  return apiClient.post<TurnResponse>(`/api/fleet/crews/${id}/run`, { task });
}
```

**Conventions:**
- One file per domain (`fleet.ts`, `system.ts`, `crews.ts`)
- Export TypeScript interfaces (match Go API response shape)
- Function names match HTTP verbs (`listCrews` = GET, `startCrewTurn` = POST)

---

## React Component Patterns

### Functional Components (Hooks)

```typescript
// src/components/CrewCard.tsx
import { useState, useEffect } from 'react';
import { getCrew, type Crew } from '@/lib/api/fleet';

interface CrewCardProps {
  crewId: string;
}

export function CrewCard({ crewId }: CrewCardProps) {
  const [crew, setCrew] = useState<Crew | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  
  useEffect(() => {
    getCrew(crewId)
      .then(setCrew)
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, [crewId]);
  
  if (loading) return <div>Loading...</div>;
  if (error) return <div className="text-red-500">Error: {error}</div>;
  if (!crew) return null;
  
  return (
    <div className="rounded-lg border p-4">
      <h3 className="text-lg font-semibold">{crew.name}</h3>
      <p className="text-sm text-gray-600">Risk: {crew.risk_tier}</p>
    </div>
  );
}
```

**Conventions:**
- Functional components (not class components)
- Props interface above component
- Loading + error + data states
- Tailwind classes for styling (no inline styles)

### Custom Hooks (Reusable Logic)

```typescript
// src/hooks/useCrew.ts
import { useState, useEffect } from 'react';
import { getCrew, type Crew } from '@/lib/api/fleet';

export function useCrew(crewId: string) {
  const [crew, setCrew] = useState<Crew | null>(null);
  const [error, setError] = useState<Error | null>(null);
  const [loading, setLoading] = useState(true);
  
  useEffect(() => {
    getCrew(crewId)
      .then(setCrew)
      .catch(setError)
      .finally(() => setLoading(false));
  }, [crewId]);
  
  return { crew, error, loading };
}

// Usage in component
function CrewCard({ crewId }: CrewCardProps) {
  const { crew, error, loading } = useCrew(crewId);
  
  if (loading) return <div>Loading...</div>;
  // ...
}
```

**Conventions:**
- Hook name starts with `use` (React convention)
- Return object (not array) for named destructuring
- Extract repeated logic (data fetching, form handling, WebSocket)

---

## TypeScript Conventions

### Strict Mode

```json
// tsconfig.json
{
  "compilerOptions": {
    "strict": true,
    "noImplicitAny": true,
    "strictNullChecks": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true
  }
}
```

**Convention:** Strict mode enabled (no `any`, explicit null checks).

### Type-Safe API Responses

```typescript
// src/types/api.ts
export interface FleetMetrics {
  gateway_latency_ms: number;
  memory_db_mb: number;
  active_crews: number;
}

// ✅ Correct: type-safe fetch
const metrics = await apiClient.get<FleetMetrics>('/api/system/metrics');
console.log(metrics.gateway_latency_ms); // TypeScript knows this exists

// ❌ Wrong: untyped fetch
const metrics = await fetch('/api/system/metrics').then(r => r.json());
console.log(metrics.gateway_latency_ms); // No type safety, runtime error if missing
```

**Convention:** Define interfaces for every API response shape.

---

## Styling (Tailwind CSS 4)

### Utility-First Classes

```tsx
// ✅ Correct: Tailwind utility classes
<button className="rounded-lg bg-blue-500 px-4 py-2 text-white hover:bg-blue-600">
  Start Crew
</button>

// ❌ Wrong: inline styles
<button style={{ borderRadius: '8px', backgroundColor: '#3b82f6', padding: '8px 16px' }}>
  Start Crew
</button>
```

**Convention:** Use Tailwind utilities (no inline `style` prop, no CSS-in-JS).

### Custom Theme

```javascript
// tailwind.config.js
export default {
  theme: {
    extend: {
      colors: {
        'galleon-blue': '#1e40af',
        'galleon-gray': '#374151',
      },
      fontFamily: {
        sans: ['Inter', 'system-ui', 'sans-serif'],
      },
    },
  },
};
```

**Convention:** Extend Tailwind theme for brand colors, fonts (not arbitrary values in JSX).

---

## Routing (React Router)

### Setup

```typescript
// src/main.tsx
import { BrowserRouter, Routes, Route } from 'react-router-dom';
import { FleetPage } from './pages/FleetPage';
import { CrewDetailPage } from './pages/CrewDetailPage';

function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route path="/" element={<FleetPage />} />
        <Route path="/crews/:id" element={<CrewDetailPage />} />
      </Routes>
    </BrowserRouter>
  );
}
```

**Convention:** Pages in `src/pages/`, route params with `:id`.

### Navigation

```tsx
import { Link, useNavigate } from 'react-router-dom';

function CrewList() {
  const navigate = useNavigate();
  
  return (
    <div>
      {/* Declarative navigation */}
      <Link to="/crews/123">View Crew</Link>
      
      {/* Programmatic navigation */}
      <button onClick={() => navigate('/crews/123')}>Go to Crew</button>
    </div>
  );
}
```

---

## Error Handling

### Error Boundaries

```tsx
// src/components/ErrorBoundary.tsx
import { Component, type ReactNode } from 'react';

interface Props {
  children: ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { hasError: false, error: null };
  
  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }
  
  render() {
    if (this.state.hasError) {
      return (
        <div className="flex h-screen items-center justify-center">
          <div className="rounded-lg border border-red-300 bg-red-50 p-4">
            <h2 className="text-lg font-semibold text-red-900">Something went wrong</h2>
            <p className="text-sm text-red-700">{this.state.error?.message}</p>
          </div>
        </div>
      );
    }
    
    return this.props.children;
  }
}

// Usage
<ErrorBoundary>
  <App />
</ErrorBoundary>
```

**Convention:** Wrap root app in `ErrorBoundary` (catch unhandled errors).

---

## Performance

### Code Splitting (Lazy Loading)

```tsx
import { lazy, Suspense } from 'react';

// Lazy load heavy components
const CrewDetailPage = lazy(() => import('./pages/CrewDetailPage'));

function App() {
  return (
    <Suspense fallback={<div>Loading...</div>}>
      <Routes>
        <Route path="/crews/:id" element={<CrewDetailPage />} />
      </Routes>
    </Suspense>
  );
}
```

**Convention:** Lazy load page-level components (smaller initial bundle).

### Memoization (Avoid Re-renders)

```tsx
import { memo, useMemo } from 'react';

// Memoize expensive component
export const CrewCard = memo(function CrewCard({ crew }: CrewCardProps) {
  return <div>{crew.name}</div>;
});

// Memoize expensive computation
function CrewList({ crews }: { crews: Crew[] }) {
  const sortedCrews = useMemo(() => 
    crews.sort((a, b) => a.name.localeCompare(b.name)),
    [crews]
  );
  
  return <div>{sortedCrews.map(crew => <CrewCard key={crew.id} crew={crew} />)}</div>;
}
```

**Convention:** Use `memo` for pure components, `useMemo` for expensive computations.

---

## Testing

### Unit Tests (Vitest)

```typescript
// src/components/CrewCard.test.tsx
import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { CrewCard } from './CrewCard';

describe('CrewCard', () => {
  it('renders crew name', () => {
    render(<CrewCard crewId="test-crew" />);
    expect(screen.getByText('Test Crew')).toBeInTheDocument();
  });
  
  it('shows error on API failure', async () => {
    // Mock API to fail
    vi.mock('@/lib/api/fleet', () => ({
      getCrew: vi.fn().mockRejectedValue(new Error('API error')),
    }));
    
    render(<CrewCard crewId="test-crew" />);
    expect(await screen.findByText(/API error/)).toBeInTheDocument();
  });
});
```

**Conventions:**
- `*.test.tsx` next to component file
- Use `@testing-library/react` (not Enzyme)
- Mock API calls with `vi.mock()`

---

## Build & Dev

### Dev Server

```bash
npm run dev  # Vite dev server :5173 + Express :3000 (proxy)
```

**Proxy:** Vite forwards `/api/*` → `http://localhost:9090` (Go engine).

### Production Build

```bash
npm run build  # Outputs to dist/
```

**Output:** `dist/` is static bundle (copied to Tauri `apps/tauri-2/dist/`).

### Vite Config

```typescript
// vite.config.ts
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      '/api': 'http://localhost:9090', // Proxy API calls to Go engine
    },
  },
});
```

---

## Naming Conventions

| Type | Pattern | Example |
|------|---------|---------|
| Component | `PascalCase` | `CrewCard`, `FleetPage` |
| File | `PascalCase.tsx` | `CrewCard.tsx`, `FleetPage.tsx` |
| Hook | `camelCase`, starts with `use` | `useCrew`, `useFleetMetrics` |
| Utility | `camelCase` | `formatDate`, `calculateRisk` |
| Type/Interface | `PascalCase` | `Crew`, `FleetMetrics` |

---

## Code Organization

### Feature-Based Folders

```
src/
├── components/
│   ├── crews/
│   │   ├── CrewCard.tsx
│   │   ├── CrewList.tsx
│   │   └── CrewForm.tsx
│   └── system/
│       ├── MetricsCard.tsx
│       └── HealthStatus.tsx
├── pages/
│   ├── FleetPage.tsx
│   └── CrewDetailPage.tsx
└── lib/
    ├── api/
    │   ├── fleet.ts
    │   └── system.ts
    └── utils/
        ├── formatDate.ts
        └── calculateRisk.ts
```

**Convention:** Group by feature (crews, system), not by type (components, hooks).

---

## Related Documents

- [Architecture Overview](../architecture/00-overview.md) — 4-tier system design
- [Boundary Policy](../architecture/01-boundary-policy.md) — What UI owns vs delegates
- [ADR 0002: web-2 Backend Policy](../decisions/0002-web2-backend-policy.md) — Zero logic in UI tier
- [Tauri Conventions](./tauri-conventions.md) — Desktop shell patterns (IPC)
