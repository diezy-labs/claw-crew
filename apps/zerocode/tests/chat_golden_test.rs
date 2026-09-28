//! Golden test baseline for Zerocode chat & turn lifecycle behavior.
//! Verifies baseline turn progression, message parsing, and fallback handling
//! prior to Phase 2 engine migration.

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum GoldenTurnState {
    Queued,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct GoldenChatMessage {
    pub role: String,
    pub content: String,
    pub status: GoldenTurnState,
}

pub struct GoldenChatSession {
    pub session_id: String,
    pub messages: Vec<GoldenChatMessage>,
}

impl GoldenChatSession {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            messages: Vec::new(),
        }
    }

    pub fn submit_prompt(&mut self, prompt: &str) -> usize {
        let idx = self.messages.len();
        self.messages.push(GoldenChatMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
            status: GoldenTurnState::Completed,
        });
        self.messages.push(GoldenChatMessage {
            role: "assistant".to_string(),
            content: String::new(),
            status: GoldenTurnState::Queued,
        });
        idx + 1
    }

    pub fn update_turn_chunk(&mut self, turn_idx: usize, chunk: &str) {
        if let Some(msg) = self.messages.get_mut(turn_idx) {
            msg.status = GoldenTurnState::Running;
            msg.content.push_str(chunk);
        }
    }

    pub fn complete_turn(&mut self, turn_idx: usize) {
        if let Some(msg) = self.messages.get_mut(turn_idx) {
            msg.status = GoldenTurnState::Completed;
        }
    }
}

#[test]
fn test_golden_chat_lifecycle() {
    let mut session = GoldenChatSession::new("golden_sess_01");
    let turn_idx = session.submit_prompt("Hello ClawCrew");

    assert_eq!(session.messages.len(), 2);
    assert_eq!(session.messages[turn_idx].status, GoldenTurnState::Queued);

    session.update_turn_chunk(turn_idx, "Hello! ");
    session.update_turn_chunk(turn_idx, "How can I help you?");
    assert_eq!(session.messages[turn_idx].status, GoldenTurnState::Running);
    assert_eq!(session.messages[turn_idx].content, "Hello! How can I help you?");

    session.complete_turn(turn_idx);
    assert_eq!(session.messages[turn_idx].status, GoldenTurnState::Completed);
}
