with open('xtask/src/cmd/mdbook/peer_groups.rs', 'r', encoding='utf-8') as f:
    content = f.read()

content = content.replace(
    'C::CloudEndpoint => "Cloud AI endpoints",\n    };',
    'C::CloudEndpoint => "Cloud AI endpoints",\n        C::AiRouter => "AI Model Routers",\n    };'
)

with open('xtask/src/cmd/mdbook/peer_groups.rs', 'w', encoding='utf-8') as f:
    f.write(content)
