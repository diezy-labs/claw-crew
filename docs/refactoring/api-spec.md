# API Specification (Inter-Process Communication)
## Protobuf Interface (Rust <-> Go)

Karena kita menggunakan gRPC, berikut adalah spesifikasi antarmuka di `proto/agent_service.proto`.

### 1. File: `agent_service.proto`

```protobuf
syntax = "proto3";

package clawcrew.agent;

option go_package = "clawcrew/engine/pb";

// Service yang dihosting oleh Go (AI Brain) dan dipanggil oleh Rust (UI/Gateway)
service AgentEngine {
    // Memulai Turn/Tugas baru untuk Agent. Mengembalikan stream respon.
    rpc StartTurn(TurnRequest) returns (stream TurnResponse);
    
    // Mengeksekusi instruksi khusus tanpa state penuh (misal RAG query cepat)
    rpc QuickQuery(QueryRequest) returns (QueryResponse);
}

// Service yang dihosting oleh Rust (Native System) dan dipanggil oleh Go (saat Agent butuh alat OS)
service SystemGateway {
    // Meminta Rust mengeksekusi tool bawaan (terminal, akses disk)
    rpc ExecuteNativeTool(ToolCallRequest) returns (ToolCallResponse);
    
    // Meminta kredensial tersandi dari Secret Vault Rust
    rpc GetDecryptedSecret(SecretRequest) returns (SecretResponse);
}

// ======================================
// Pesan (Messages)
// ======================================

message TurnRequest {
    string session_id = 1;
    string agent_id = 2; // merujuk ke konfig agent TOML
    string prompt = 3;
    bytes context_window = 4; // Tergantung arsitektur memori
}

message TurnResponse {
    enum EventType {
        THOUGHT_CHUNK = 0;
        TEXT_CHUNK = 1;
        TOOL_CALL_STARTED = 2;
        TOOL_CALL_FINISHED = 3;
        SUBAGENT_SPAWNED = 4;
        TURN_COMPLETED = 5;
    }
    
    EventType type = 1;
    string content = 2;   // potongan teks / JSON args
    string subagent_id = 3; // jika event berasal dari sub-agent
}

message ToolCallRequest {
    string tool_name = 1; // e.g. "bash", "write_to_file"
    string arguments_json = 2;
}

message ToolCallResponse {
    bool success = 1;
    string output = 2;
    string error = 3;
}
```

### 2. Protokol Jaringan Lokal
- **Transpor**: `localhost` TCP Port statis (e.g., `55051` atau deteksi *ephemeral port*).
- **Security**: Tidak perlu TLS karena ini komunikasi *local loopback* antar proses lokal, untuk memaksimalkan kecepatan.
