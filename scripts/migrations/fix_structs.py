import os
import re

directories = ["crates/clawcrew-runtime/src"]

def process_file(filepath):
    with open(filepath, "r", encoding="utf-8") as f:
        content = f.read()

    # Find `ResolvedIo { ... }` blocks and ensure `app_registry:` is inside
    def repl(m):
        block = m.group(1)
        if "app_registry" not in block:
            return m.group(0).replace("}", "    app_registry: None,\n}")
        return m.group(0)

    # Simple regex won't balance braces well, so let's just do it directly on the string
    # Actually, if we just find `ResolvedIo {` and then insert before the closing `}` if missing
    # But `}` might be nested. 
    # Since these are struct initializations, we can just replace `receipt_generator: None,` with `receipt_generator: None, app_registry: None,` or similar.
    # Or just `tools_registry:` -> `app_registry: None, tools_registry:`
    new_content = re.sub(r'(ResolvedIo\s*\{)(?![^\}]*app_registry)', r'\1\n    app_registry: None,', content, flags=re.DOTALL)
    
    if new_content != content:
        with open(filepath, "w", encoding="utf-8") as f:
            f.write(new_content)
        print(f"Updated {filepath}")

for root, _, files in os.walk(directories[0]):
    for file in files:
        if file.endswith(".rs"):
            process_file(os.path.join(root, file))

# Fix AppManifest
compat_path = "crates/clawcrew-runtime/src/platform/compat.rs"
with open(compat_path, "r", encoding="utf-8") as f:
    c = f.read()
c = re.sub(r'(AppManifest\s*\{)(?![^\}]*mcp_server)', r'\1\n        mcp_server: None,', c, flags=re.DOTALL)
with open(compat_path, "w", encoding="utf-8") as f:
    f.write(c)
print(f"Updated {compat_path}")

