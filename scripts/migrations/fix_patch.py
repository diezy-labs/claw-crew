import os
import re

missing_fields = """
                session_key: None,
                workspace: None,
                cancellation_state: crate::control_plane::task_registry::CancellationState::None,
"""

files = [
    "crates/clawcrew-runtime/src/tools/delegate.rs",
    "crates/clawcrew-runtime/src/tools/spawn_subagent.rs",
    "crates/clawcrew-runtime/src/control_plane/boot.rs",
    "crates/clawcrew-runtime/src/control_plane/reaper.rs",
    "crates/clawcrew-runtime/src/control_plane/task_store_sqlite/goal.rs",
    "crates/clawcrew-runtime/src/control_plane/task_store_sqlite.rs"
]

for file in files:
    if not os.path.exists(file): continue
    with open(file, "r") as f: content = f.read()
    
    # Use regex to match `principal_id` line and insert the missing fields after it, keeping indentation
    def replacer(m):
        prefix = m.group(1) # The line containing principal_id: ...
        indent = m.group(2) # The indentation of started_at
        return f"{prefix}\n{indent}session_key: None,\n{indent}workspace: None,\n{indent}cancellation_state: crate::control_plane::task_registry::CancellationState::None,\n{indent}started_at:"
        
    new_content = re.sub(r"(principal_id:\s*[^,]+,)\n(\s*)started_at:", replacer, content)
    
    with open(file, "w") as f: f.write(new_content)

