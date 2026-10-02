with open('src/main.rs', 'r', encoding='utf-8') as f:
    content = f.read()

content = content.replace(
    '        memory_command: MemoryCommands,\n    },\n\n    /// Manage configuration',
    '        memory_command: MemoryCommands,\n    },\n\n    /// Backup and Restore (P3.2)\n    Backup {\n        #[command(subcommand)]\n        backup_command: BackupCommands,\n    },\n\n    /// Manage Ecosystem Apps (P3.3)\n    App {\n        #[command(subcommand)]\n        app_command: AppCommands,\n    },\n\n    /// Manage configuration'
)

with open('src/main.rs', 'w', encoding='utf-8') as f:
    f.write(content)
