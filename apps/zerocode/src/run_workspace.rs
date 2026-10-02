//! KiroCrew-inspired Run Workspace Experience Layer.
//! Provides unified task timeline, crew roster status, artifact diff viewer,
//! progressive disclosure sub-drawers, and run summary metrics.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

/// Terminal color standards according to TASK-6.7
pub fn status_color(status: &str) -> Color {
    match status.to_lowercase().as_str() {
        "completed" | "success" | "ready" => Color::Green,
        "waiting_approval" | "waiting_for_input" | "waiting" => Color::Yellow,
        "running" | "executing_tool" | "thinking" => Color::Cyan,
        "failed" | "error" | "cancelled" => Color::Red,
        _ => Color::DarkGray,
    }
}

/// Agent roster entry for the Crew Panel (TASK-6.2)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrewMember {
    pub id: String,
    pub name: String,
    pub role: String,
    pub status: String,
}

/// Task entry for the Task Timeline DAG visualization (TASK-6.3)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineTask {
    pub id: String,
    pub title: String,
    pub status: String,
    pub assigned_to: String,
    pub dependencies: Vec<String>,
}

/// Artifact summary entry (TASK-6.5)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactItem {
    pub id: String,
    pub path: String,
    pub size: usize,
    pub checksum: String,
}

/// Run summary metrics (TASK-6.6)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunSummaryMetrics {
    pub run_id: String,
    pub status: String,
    pub duration_secs: u64,
    pub tasks_total: usize,
    pub tasks_completed: usize,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub artifacts_count: usize,
}

/// Run Workspace UI state
#[derive(Debug, Clone, Default)]
pub struct RunWorkspaceState {
    pub run_id: Option<String>,
    pub crew_members: Vec<CrewMember>,
    pub tasks: Vec<TimelineTask>,
    pub artifacts: Vec<ArtifactItem>,
    pub summary: Option<RunSummaryMetrics>,
    pub show_raw_logs_drawer: bool, // Progressive disclosure (TASK-6.4)
    pub selected_task_idx: usize,
}

impl RunWorkspaceState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn toggle_raw_logs(&mut self) {
        self.show_raw_logs_drawer = !self.show_raw_logs_drawer;
    }

    /// Render the Crew Panel (TASK-6.2)
    pub fn render_crew_panel(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(" Crew Roster ", Style::default().add_modifier(Modifier::BOLD)));

        if self.crew_members.is_empty() {
            let empty = Paragraph::new(Span::styled("No active agents in crew.", Style::default().fg(Color::DarkGray)))
                .block(block);
            f.render_widget(empty, area);
            return;
        }

        let items: Vec<ListItem> = self
            .crew_members
            .iter()
            .map(|member| {
                let col = status_color(&member.status);
                let line = Line::from(vec![
                    Span::styled(" ● ", Style::default().fg(col)),
                    Span::styled(&member.name, Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" ({}) ", member.role), Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("[{}]", member.status), Style::default().fg(col)),
                ]);
                ListItem::new(line)
            })
            .collect();

        let list = List::new(items).block(block);
        f.render_widget(list, area);
    }

    /// Render Task Timeline (TASK-6.3)
    pub fn render_task_timeline(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(" Task Timeline (DAG) ", Style::default().add_modifier(Modifier::BOLD)));

        if self.tasks.is_empty() {
            let empty = Paragraph::new(Span::styled("No tasks scheduled for this run.", Style::default().fg(Color::DarkGray)))
                .block(block);
            f.render_widget(empty, area);
            return;
        }

        let items: Vec<ListItem> = self
            .tasks
            .iter()
            .enumerate()
            .map(|(i, task)| {
                let col = status_color(&task.status);
                let dep_str = if task.dependencies.is_empty() {
                    String::new()
                } else {
                    format!(" <- deps: {}", task.dependencies.join(", "))
                };

                let line = Line::from(vec![
                    Span::styled(format!(" {:02}. ", i + 1), Style::default().fg(Color::DarkGray)),
                    Span::styled("■ ", Style::default().fg(col)),
                    Span::styled(&task.title, Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" [{}]", task.status), Style::default().fg(col)),
                    Span::styled(dep_str, Style::default().fg(Color::DarkGray)),
                ]);
                ListItem::new(line)
            })
            .collect();

        let list = List::new(items).block(block);
        f.render_widget(list, area);
    }

    /// Render Artifact Panel (TASK-6.5)
    pub fn render_artifact_panel(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(" Artifacts & Diff Review ", Style::default().add_modifier(Modifier::BOLD)));

        if self.artifacts.is_empty() {
            let empty = Paragraph::new(Span::styled("No artifacts generated yet.", Style::default().fg(Color::DarkGray)))
                .block(block);
            f.render_widget(empty, area);
            return;
        }

        let items: Vec<ListItem> = self
            .artifacts
            .iter()
            .map(|art| {
                let line = Line::from(vec![
                    Span::styled(" 📄 ", Style::default().fg(Color::Cyan)),
                    Span::styled(&art.path, Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" ({} bytes)", art.size), Style::default().fg(Color::DarkGray)),
                    Span::styled(format!(" [{:.8}]", art.checksum), Style::default().fg(Color::DarkGray)),
                ]);
                ListItem::new(line)
            })
            .collect();

        let list = List::new(items).block(block);
        f.render_widget(list, area);
    }

    /// Render Run Summary (TASK-6.6)
    pub fn render_run_summary(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(" Run Summary ", Style::default().add_modifier(Modifier::BOLD)));

        let summary = match &self.summary {
            Some(s) => s,
            None => {
                let empty = Paragraph::new(Span::styled("Run in progress...", Style::default().fg(Color::DarkGray)))
                    .block(block);
                f.render_widget(empty, area);
                return;
            }
        };

        let col = status_color(&summary.status);
        let text = vec![
            Line::from(vec![
                Span::raw("Status: "),
                Span::styled(&summary.status, Style::default().fg(col).add_modifier(Modifier::BOLD)),
                Span::raw(format!(" | Duration: {}s", summary.duration_secs)),
            ]),
            Line::from(vec![
                Span::raw(format!("Tasks: {}/{} completed", summary.tasks_completed, summary.tasks_total)),
                Span::raw(format!(" | Artifacts: {}", summary.artifacts_count)),
            ]),
            Line::from(vec![
                Span::raw(format!("Token Usage: prompt={} completion={} total={}",
                    summary.prompt_tokens,
                    summary.completion_tokens,
                    summary.prompt_tokens + summary.completion_tokens
                )),
            ]),
        ];

        let p = Paragraph::new(text).block(block).wrap(Wrap { trim: true });
        f.render_widget(p, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_color_palette() {
        assert_eq!(status_color("completed"), Color::Green);
        assert_eq!(status_color("waiting_approval"), Color::Yellow);
        assert_eq!(status_color("running"), Color::Cyan);
        assert_eq!(status_color("failed"), Color::Red);
        assert_eq!(status_color("unknown_state"), Color::DarkGray);
    }

    #[test]
    fn test_progressive_disclosure_toggle() {
        let mut state = RunWorkspaceState::new();
        assert!(!state.show_raw_logs_drawer);
        state.toggle_raw_logs();
        assert!(state.show_raw_logs_drawer);
        state.toggle_raw_logs();
        assert!(!state.show_raw_logs_drawer);
    }
}
