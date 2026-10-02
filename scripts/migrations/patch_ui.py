import re

# lazyPages.tsx
with open('web/src/router/lazyPages.tsx', 'r', encoding='utf-8') as f:
    text = f.read()
if 'export const Apps' not in text:
    text += "\nexport const Apps = lazy(() => import('../pages/Apps'));\nexport const Instances = lazy(() => import('../pages/Instances'));\nexport const Recovery = lazy(() => import('../pages/Recovery'));\n"
    with open('web/src/router/lazyPages.tsx', 'w', encoding='utf-8') as f:
        f.write(text)

# router.tsx
with open('web/src/router/router.tsx', 'r', encoding='utf-8') as f:
    text = f.read()
if 'Apps,' not in text:
    text = text.replace('AgentsList,', 'AgentsList,\n  Apps,\n  Instances,\n  Recovery,')
    routes = """        <Route path="/apps" element={<Apps />} />
        <Route path="/instances" element={<Instances />} />
        <Route path="/recovery" element={<Recovery />} />
        <Route path="/tasks\""""
    text = text.replace('<Route path="/tasks"', routes)
    with open('web/src/router/router.tsx', 'w', encoding='utf-8') as f:
        f.write(text)

# Sidebar.tsx
with open('web/src/components/layout/Sidebar.tsx', 'r', encoding='utf-8') as f:
    text = f.read()
if 'nav.apps' not in text:
    text = text.replace('import {', 'import {\n  AppWindow,\n  Network,\n  LifeBuoy,', 1)
    home_group = """  {
    headingKey: 'nav.group.home',
    items: [
      { to: '/', icon: LayoutDashboard, labelKey: 'nav.dashboard' },
      { to: '/apps', icon: AppWindow, labelKey: 'nav.apps' },
    ],
  },"""
    text = re.sub(r'\{\s*headingKey:\s*\'nav\.group\.home\',.*?\},', home_group, text, flags=re.DOTALL)
    
    op_group = """{ to: '/instances', icon: Network, labelKey: 'nav.instances' },
      { to: '/recovery', icon: LifeBuoy, labelKey: 'nav.recovery' },
      { to: '/audit'"""
    text = text.replace("{ to: '/audit'", op_group)
    with open('web/src/components/layout/Sidebar.tsx', 'w', encoding='utf-8') as f:
        f.write(text)

# i18n.ts
with open('web/src/lib/i18n.ts', 'r', encoding='utf-8') as f:
    text = f.read()
if 'nav.apps' not in text:
    text = text.replace("'nav.dashboard': 'Dashboard',", "'nav.dashboard': 'Dashboard',\n    'nav.apps': 'Apps',\n    'nav.instances': 'Instances',\n    'nav.recovery': 'Recovery',")
    with open('web/src/lib/i18n.ts', 'w', encoding='utf-8') as f:
        f.write(text)

