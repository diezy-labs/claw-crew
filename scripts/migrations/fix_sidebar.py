import re

with open('web/src/components/layout/Sidebar.tsx', 'r', encoding='utf-8') as f:
    text = f.read()

# Revert the wrong import from react-router-dom
text = re.sub(r'import \{\s*AppWindow,\s*Network,\s*LifeBuoy,\s*useLocation\s*\} from \'react-router-dom\';', 'import { useLocation } from \'react-router-dom\';', text)

# Insert the icons into lucide-react import
if 'AppWindow' not in text.split('lucide-react')[0].split('import {')[-1]:
    # find lucide-react import
    text = re.sub(r'import \{\s*Activity,', 'import { Activity, AppWindow, Network, LifeBuoy,', text)

with open('web/src/components/layout/Sidebar.tsx', 'w', encoding='utf-8') as f:
    f.write(text)
