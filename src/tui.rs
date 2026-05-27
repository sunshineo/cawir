use std::{
    io::{self, BufRead, Write},
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui_textarea::{Input, Key, TextArea};
use serde_json::{Map, Value, json};

use crate::{
    Error, Result,
    app_client::{
        self, EventNotificationParams, METHOD_APPROVAL_PLAN, METHOD_APPROVAL_TOOL, METHOD_EVENT,
        METHOD_INITIALIZE, METHOD_SESSION_NEW, METHOD_SESSION_RESUME, METHOD_SHUTDOWN,
        METHOD_TURN_SUBMIT, PROTOCOL_VERSION, ServerMessage,
    },
};

const CLIENT_NAME: &str = "cawir-tui";

pub(crate) struct TuiOptions {
    pub(crate) provider: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) resume: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct SessionInfo {
    session_id: String,
    provider: String,
    model: String,
    mode: String,
}

#[derive(Debug, PartialEq)]
struct PendingApproval {
    id: Value,
    method: String,
    label: String,
    summary: String,
}

#[derive(Debug, PartialEq)]
struct ApprovalAnswer {
    id: Value,
    approved: bool,
}

#[derive(Debug)]
struct TuiState {
    session: SessionInfo,
    transcript: Vec<String>,
    tools: Vec<String>,
    input: TextArea<'static>,
    pending_approval: Option<PendingApproval>,
    streaming_assistant: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum KeyAction {
    Continue,
    Quit,
}

impl TuiState {
    fn new(session: SessionInfo) -> Self {
        Self {
            session,
            transcript: Vec::new(),
            tools: Vec::new(),
            input: input_textarea(""),
            pending_approval: None,
            streaming_assistant: None,
        }
    }

    fn handle_event(&mut self, event: Value) {
        match event.get("type").and_then(Value::as_str) {
            Some("assistant_text_delta") => {
                let text = event
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let streamed = self.streaming_assistant.get_or_insert_with(String::new);
                streamed.push_str(text);
                if self
                    .transcript
                    .last()
                    .is_some_and(|line| line.starts_with("assistant: "))
                {
                    if let Some(line) = self.transcript.last_mut() {
                        *line = format!("assistant: {streamed}");
                    }
                } else {
                    self.transcript.push(format!("assistant: {streamed}"));
                }
            }
            Some("assistant_text") => {
                let text = event
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if self.streaming_assistant.take().is_none() {
                    self.transcript.push(format!("assistant: {text}"));
                }
            }
            Some("pre_tool_use") => {
                if let Some(name) = event.get("name").and_then(Value::as_str) {
                    self.tools.push(format!("{name} requested"));
                }
            }
            Some("post_tool_use") => {
                if let Some(name) = event.get("name").and_then(Value::as_str) {
                    let status = if event
                        .get("is_error")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        "error"
                    } else {
                        "ok"
                    };
                    self.tools.push(format!("{name} {status}"));
                }
            }
            _ => {}
        }
    }

    fn handle_approval_request(&mut self, id: Value, method: String, params: Value) {
        let label = params
            .get("tool_name")
            .or_else(|| params.get("tool_use_id"))
            .and_then(Value::as_str)
            .unwrap_or("approval")
            .to_string();
        let detail = params
            .get("summary")
            .or_else(|| params.get("plan"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let summary = if detail.is_empty() {
            label.clone()
        } else {
            format!("{label}: {detail}")
        };

        self.pending_approval = Some(PendingApproval {
            id,
            method,
            label,
            summary,
        });
    }

    fn answer_approval(&mut self, approved: bool) -> Option<ApprovalAnswer> {
        let pending = self.pending_approval.take()?;
        let decision = if approved { "approved" } else { "denied" };
        self.tools.push(format!("{} {decision}", pending.label));
        Some(ApprovalAnswer {
            id: pending.id,
            approved,
        })
    }

    fn clear_input(&mut self) {
        self.input = input_textarea("");
    }

    fn input_text(&self) -> &str {
        self.input.lines().first().map(String::as_str).unwrap_or("")
    }

    #[cfg(test)]
    fn set_input_text(&mut self, text: &str) {
        self.input = input_textarea(text);
    }
}

fn input_textarea(text: impl Into<String>) -> TextArea<'static> {
    TextArea::from([text.into()])
}

pub(crate) fn run(options: TuiOptions) -> Result<()> {
    let mut process = app_client::AppServerProcess::spawn()?;
    let (session, next_id) = {
        let (reader, writer) = process.io_mut()?;
        initialize_session(reader, writer, &options)?
    };
    let (reader, mut writer) = process.take_io()?;
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let mut reader = reader;
        loop {
            match app_client::read_server_message(&mut reader) {
                Ok(message) => {
                    if sender.send(Ok(message)).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender.send(Err(error));
                    break;
                }
            }
        }
    });

    let terminal_result = run_terminal(session, next_id, receiver, &mut writer);
    if terminal_result.is_ok() {
        let _ =
            app_client::write_request(&mut writer, next_id + 100_000, METHOD_SHUTDOWN, json!({}));
    } else {
        let _ = process.kill();
    }
    let _ = process.wait();

    terminal_result
}

fn initialize_session(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    options: &TuiOptions,
) -> Result<(SessionInfo, u64)> {
    let mut next_id = 1;
    blocking_request_result(
        reader,
        writer,
        &mut next_id,
        METHOD_INITIALIZE,
        json!({
            "protocol_version": PROTOCOL_VERSION,
            "client_name": CLIENT_NAME,
            "client_version": env!("CARGO_PKG_VERSION")
        }),
    )?;

    let session_result = if let Some(session_id) = &options.resume {
        blocking_request_result(
            reader,
            writer,
            &mut next_id,
            METHOD_SESSION_RESUME,
            json!({ "session_id": session_id }),
        )?
    } else {
        blocking_request_result(
            reader,
            writer,
            &mut next_id,
            METHOD_SESSION_NEW,
            session_new_params(options),
        )?
    };

    Ok((session_info_from_result(&session_result)?, next_id))
}

fn blocking_request_result(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    next_id: &mut u64,
    method: &str,
    params: Value,
) -> Result<Value> {
    let id = *next_id;
    *next_id += 1;
    app_client::write_request(writer, id, method, params)?;

    loop {
        match app_client::read_server_message(reader)? {
            ServerMessage::Response(response) => {
                if response.id() != &json!(id) {
                    return Err(Error::Env(format!(
                        "unexpected app-server response id: expected {id}, got {}",
                        app_client::compact_json(response.id())
                    )));
                }
                return response.into_result();
            }
            ServerMessage::Notification(_) => {}
            ServerMessage::Request(request) => {
                return Err(Error::Env(format!(
                    "unexpected app-server request during setup: {}",
                    request.method
                )));
            }
        }
    }
}

fn session_new_params(options: &TuiOptions) -> Value {
    let mut params = Map::new();
    if let Some(provider) = &options.provider {
        params.insert("provider".to_string(), json!(provider));
    }
    if let Some(model) = &options.model {
        params.insert("model".to_string(), json!(model));
    }
    Value::Object(params)
}

fn session_info_from_result(value: &Value) -> Result<SessionInfo> {
    Ok(SessionInfo {
        session_id: required_string(value, "session_id")?,
        provider: required_string(value, "provider")?,
        model: required_string(value, "model")?,
        mode: required_string(value, "mode")?,
    })
}

fn required_string(value: &Value, key: &str) -> Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| Error::Env(format!("app-server result missing string field `{key}`")))
}

fn run_terminal(
    session: SessionInfo,
    mut next_id: u64,
    receiver: Receiver<Result<ServerMessage>>,
    writer: &mut impl Write,
) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;
    let mut state = TuiState::new(session);
    let result = run_event_loop(&mut terminal, &receiver, writer, &mut state, &mut next_id);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_event_loop(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    receiver: &Receiver<Result<ServerMessage>>,
    writer: &mut impl Write,
    state: &mut TuiState,
    next_id: &mut u64,
) -> Result<()> {
    let mut pending_turn_id = None;

    loop {
        drain_server_messages(receiver, state, &mut pending_turn_id)?;
        terminal.draw(|frame| render_state(frame, state))?;

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) if should_quit(key) => return Ok(()),
                Event::Key(key) => {
                    match handle_key(key, writer, state, next_id, &mut pending_turn_id)? {
                        KeyAction::Continue => {}
                        KeyAction::Quit => return Ok(()),
                    }
                }
                _ => {}
            }
        }
    }
}

fn drain_server_messages(
    receiver: &Receiver<Result<ServerMessage>>,
    state: &mut TuiState,
    pending_turn_id: &mut Option<u64>,
) -> Result<()> {
    while let Ok(message) = receiver.try_recv() {
        match message? {
            ServerMessage::Notification(notification) => {
                if notification.method == METHOD_EVENT {
                    let params: EventNotificationParams =
                        serde_json::from_value(notification.params)
                            .map_err(|error| Error::Env(error.to_string()))?;
                    state.handle_event(params.event);
                }
            }
            ServerMessage::Request(request) => {
                if request.method == METHOD_APPROVAL_TOOL || request.method == METHOD_APPROVAL_PLAN
                {
                    state.handle_approval_request(request.id, request.method, request.params);
                } else {
                    return Err(Error::Env(format!(
                        "unsupported app-server request: {}",
                        request.method
                    )));
                }
            }
            ServerMessage::Response(response) => {
                if let Some(turn_id) = *pending_turn_id
                    && response.id() == &json!(turn_id)
                {
                    let result = response.into_result()?;
                    if let Some(mode) = result.get("mode").and_then(Value::as_str) {
                        state.session.mode = mode.to_string();
                    }
                    *pending_turn_id = None;
                }
            }
        }
    }

    Ok(())
}

fn handle_key(
    key: KeyEvent,
    writer: &mut impl Write,
    state: &mut TuiState,
    next_id: &mut u64,
    pending_turn_id: &mut Option<u64>,
) -> Result<KeyAction> {
    if state.pending_approval.is_some() {
        match key.code {
            KeyCode::Char('a') | KeyCode::Char('A') => {
                if let Some(answer) = state.answer_approval(true) {
                    app_client::write_response(
                        writer,
                        answer.id,
                        json!({ "approved": answer.approved }),
                    )?;
                }
            }
            KeyCode::Char('d') | KeyCode::Char('D') | KeyCode::Esc => {
                if let Some(answer) = state.answer_approval(false) {
                    app_client::write_response(
                        writer,
                        answer.id,
                        json!({ "approved": answer.approved }),
                    )?;
                }
            }
            _ => {}
        }
        return Ok(KeyAction::Continue);
    }

    if is_submit_key(key) {
        if pending_turn_id.is_none() && !state.input_text().trim().is_empty() {
            let prompt = state.input_text().trim().to_string();
            if is_exit_command(&prompt) {
                state.clear_input();
                return Ok(KeyAction::Quit);
            }
            state.transcript.push(format!("user: {prompt}"));
            state.clear_input();
            let id = *next_id;
            *next_id += 1;
            *pending_turn_id = Some(id);
            app_client::write_request(
                writer,
                id,
                METHOD_TURN_SUBMIT,
                json!({
                    "session_id": state.session.session_id.clone(),
                    "prompt": prompt
                }),
            )?;
        }
        return Ok(KeyAction::Continue);
    }

    state.input.input(textarea_input_from_key(key));

    Ok(KeyAction::Continue)
}

fn should_quit(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('q'))
        || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c'))
}

fn is_submit_key(key: KeyEvent) -> bool {
    key.code == KeyCode::Enter
        || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('m'))
}

fn textarea_input_from_key(key: KeyEvent) -> Input {
    let textarea_key = match key.code {
        KeyCode::Char(ch) => Key::Char(ch),
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Enter => Key::Enter,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Tab | KeyCode::BackTab => Key::Tab,
        KeyCode::Delete => Key::Delete,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Esc => Key::Esc,
        KeyCode::F(number) => Key::F(number),
        _ => Key::Null,
    };

    Input {
        key: textarea_key,
        ctrl: key.modifiers.contains(KeyModifiers::CONTROL),
        alt: key.modifiers.contains(KeyModifiers::ALT),
        shift: key.modifiers.contains(KeyModifiers::SHIFT) || key.code == KeyCode::BackTab,
    }
}

fn is_exit_command(input: &str) -> bool {
    matches!(input, "/exit" | "/quit")
}

fn render_state(frame: &mut ratatui::Frame<'_>, state: &TuiState) {
    use ratatui::{
        layout::{Constraint, Direction, Layout, Position},
        style::{Color, Modifier, Style},
        text::Line,
        widgets::{Block, Borders, Paragraph, Wrap},
    };

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(8),
            Constraint::Length(5),
            Constraint::Length(3),
        ])
        .split(frame.area());

    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(outer[0]);

    let side = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(3)])
        .split(main[1]);

    let transcript = if state.transcript.is_empty() {
        "No messages yet".to_string()
    } else {
        state.transcript.join("\n")
    };
    frame.render_widget(
        Paragraph::new(transcript)
            .block(Block::default().title("Transcript").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        main[0],
    );

    let status = vec![
        Line::from(format!(
            "provider: {}  model: {}",
            state.session.provider, state.session.model
        )),
        Line::from(format!(
            "mode: {}  session: {}",
            state.session.mode, state.session.session_id
        )),
        Line::from("Enter send | a/d approval | q quit"),
    ];
    frame.render_widget(
        Paragraph::new(status)
            .block(Block::default().title("Status").borders(Borders::ALL))
            .style(Style::default().fg(Color::Cyan)),
        side[0],
    );

    let tools = if state.tools.is_empty() {
        "No tools yet".to_string()
    } else {
        state.tools.join("\n")
    };
    frame.render_widget(
        Paragraph::new(tools)
            .block(Block::default().title("Tools").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        side[1],
    );

    let approval = state
        .pending_approval
        .as_ref()
        .map(|approval| format!("{}\n[a] approve    [d] deny", approval.summary))
        .unwrap_or_else(|| "No pending approval".to_string());
    frame.render_widget(
        Paragraph::new(approval)
            .block(Block::default().title("Approval").borders(Borders::ALL))
            .style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .wrap(Wrap { trim: false }),
        outer[1],
    );

    let input_area = outer[2];
    frame.render_widget(
        Paragraph::new(state.input_text())
            .block(Block::default().title("Input").borders(Borders::ALL)),
        input_area,
    );
    let input_inner_width = input_area.width.saturating_sub(2);
    let cursor = state.input.cursor();
    let cursor_column = (cursor.1 as u16).min(input_inner_width.saturating_sub(1));
    frame.set_cursor_position(Position {
        x: input_area.x.saturating_add(1).saturating_add(cursor_column),
        y: input_area.y.saturating_add(1),
    });
}

#[cfg(test)]
fn render_state_to_text(state: &TuiState, width: u16, height: u16) -> Result<String> {
    use ratatui::{Terminal, backend::TestBackend};

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|frame| render_state(frame, state))?;
    let buffer = terminal.backend().buffer();
    let area = buffer.area();
    let mut output = String::new();

    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            output.push_str(buffer[(x, y)].symbol());
        }
        output.push('\n');
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend, layout::Position};
    use ratatui_textarea::CursorMove;
    use serde_json::json;

    use super::*;

    #[test]
    fn tui_state_tracks_status_transcript_tools_and_approval() {
        let mut state = TuiState::new(SessionInfo {
            session_id: "session-1".to_string(),
            provider: "openai".to_string(),
            model: "gpt-5.4".to_string(),
            mode: "default".to_string(),
        });

        state.handle_event(json!({
            "type": "assistant_text_delta",
            "provider": "openai",
            "text": "hel"
        }));
        state.handle_event(json!({
            "type": "assistant_text_delta",
            "provider": "openai",
            "text": "lo"
        }));
        state.handle_event(json!({
            "type": "pre_tool_use",
            "id": "tool-1",
            "name": "read_file",
            "input": { "path": "docs/status.md" }
        }));
        state.handle_approval_request(
            json!("server-1"),
            "approval/tool".to_string(),
            json!({
                "tool_name": "write_file",
                "summary": "write src/main.rs"
            }),
        );

        assert_eq!(state.session.provider, "openai");
        assert_eq!(state.transcript, vec!["assistant: hello"]);
        assert_eq!(state.tools, vec!["read_file requested"]);
        assert_eq!(
            state.pending_approval.as_ref().unwrap().summary,
            "write_file: write src/main.rs"
        );
    }

    #[test]
    fn tui_state_builds_approval_response_and_clears_prompt() {
        let mut state = TuiState::new(SessionInfo {
            session_id: "session-1".to_string(),
            provider: "openai".to_string(),
            model: "gpt-5.4".to_string(),
            mode: "default".to_string(),
        });
        state.handle_approval_request(
            json!("server-1"),
            "approval/tool".to_string(),
            json!({ "tool_name": "shell", "summary": "run cargo test" }),
        );

        let response = state.answer_approval(true).unwrap();

        assert_eq!(
            response,
            ApprovalAnswer {
                id: json!("server-1"),
                approved: true,
            }
        );
        assert!(state.pending_approval.is_none());
        assert_eq!(state.tools, vec!["shell approved"]);
    }

    #[test]
    fn rendered_tui_contains_the_mvp_panes() {
        let mut state = TuiState::new(SessionInfo {
            session_id: "session-1".to_string(),
            provider: "openai".to_string(),
            model: "gpt-5.4".to_string(),
            mode: "default".to_string(),
        });
        state.transcript.push("assistant: hello".to_string());
        state.tools.push("read_file ok".to_string());
        state.handle_approval_request(
            json!("server-1"),
            "approval/tool".to_string(),
            json!({ "tool_name": "write_file", "summary": "write src/main.rs" }),
        );
        state.set_input_text("next prompt");
        state.input.move_cursor(CursorMove::End);

        let rendered = render_state_to_text(&state, 100, 32).unwrap();

        for expected in [
            "Transcript",
            "Status",
            "Tools",
            "Approval",
            "Input",
            "openai",
            "gpt-5.4",
            "write_file: write src/main.rs",
            "next prompt",
        ] {
            assert!(
                rendered.contains(expected),
                "expected rendered TUI to contain {expected:?}, got:\n{rendered}"
            );
        }
    }

    #[test]
    fn slash_exit_quits_without_submitting_a_turn() {
        let mut state = TuiState::new(SessionInfo {
            session_id: "session-1".to_string(),
            provider: "openai".to_string(),
            model: "gpt-5.4".to_string(),
            mode: "default".to_string(),
        });
        state.set_input_text("/exit");
        let mut writer = Vec::new();
        let mut next_id = 3;
        let mut pending_turn_id = None;

        let action = handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut writer,
            &mut state,
            &mut next_id,
            &mut pending_turn_id,
        )
        .unwrap();

        assert_eq!(action, KeyAction::Quit);
        assert!(writer.is_empty());
        assert!(pending_turn_id.is_none());
        assert!(state.input_text().is_empty());
        assert!(state.transcript.is_empty());
    }

    #[test]
    fn input_cursor_moves_and_edits_inside_line() {
        let mut state = TuiState::new(SessionInfo {
            session_id: "session-1".to_string(),
            provider: "openai".to_string(),
            model: "gpt-5.4".to_string(),
            mode: "default".to_string(),
        });
        let mut writer = Vec::new();
        let mut next_id = 3;
        let mut pending_turn_id = None;

        for ch in ['a', 'b', 'd'] {
            handle_key(
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
                &mut writer,
                &mut state,
                &mut next_id,
                &mut pending_turn_id,
            )
            .unwrap();
        }
        handle_key(
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            &mut writer,
            &mut state,
            &mut next_id,
            &mut pending_turn_id,
        )
        .unwrap();
        handle_key(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
            &mut writer,
            &mut state,
            &mut next_id,
            &mut pending_turn_id,
        )
        .unwrap();
        handle_key(
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            &mut writer,
            &mut state,
            &mut next_id,
            &mut pending_turn_id,
        )
        .unwrap();
        handle_key(
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
            &mut writer,
            &mut state,
            &mut next_id,
            &mut pending_turn_id,
        )
        .unwrap();
        handle_key(
            KeyEvent::new(KeyCode::Home, KeyModifiers::NONE),
            &mut writer,
            &mut state,
            &mut next_id,
            &mut pending_turn_id,
        )
        .unwrap();
        handle_key(
            KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE),
            &mut writer,
            &mut state,
            &mut next_id,
            &mut pending_turn_id,
        )
        .unwrap();
        handle_key(
            KeyEvent::new(KeyCode::End, KeyModifiers::NONE),
            &mut writer,
            &mut state,
            &mut next_id,
            &mut pending_turn_id,
        )
        .unwrap();

        assert_eq!(state.input_text(), "cd");
        assert_eq!(
            state.input.cursor(),
            (0, state.input_text().chars().count())
        );
    }

    #[test]
    fn input_editor_uses_readline_home_and_end_shortcuts() {
        let mut state = TuiState::new(SessionInfo {
            session_id: "session-1".to_string(),
            provider: "openai".to_string(),
            model: "gpt-5.4".to_string(),
            mode: "default".to_string(),
        });
        let mut writer = Vec::new();
        let mut next_id = 3;
        let mut pending_turn_id = None;

        for key in [
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
        ] {
            handle_key(
                key,
                &mut writer,
                &mut state,
                &mut next_id,
                &mut pending_turn_id,
            )
            .unwrap();
        }

        assert_eq!(state.input_text(), "bacd");
    }

    #[test]
    fn input_editor_uses_readline_word_shortcuts() {
        let mut state = TuiState::new(SessionInfo {
            session_id: "session-1".to_string(),
            provider: "openai".to_string(),
            model: "gpt-5.4".to_string(),
            mode: "default".to_string(),
        });
        let mut writer = Vec::new();
        let mut next_id = 3;
        let mut pending_turn_id = None;

        for ch in "hello world".chars() {
            handle_key(
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
                &mut writer,
                &mut state,
                &mut next_id,
                &mut pending_turn_id,
            )
            .unwrap();
        }
        handle_key(
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT),
            &mut writer,
            &mut state,
            &mut next_id,
            &mut pending_turn_id,
        )
        .unwrap();
        handle_key(
            KeyEvent::new(KeyCode::Char('_'), KeyModifiers::NONE),
            &mut writer,
            &mut state,
            &mut next_id,
            &mut pending_turn_id,
        )
        .unwrap();

        assert_eq!(state.input_text(), "hello _world");
    }

    #[test]
    fn rendered_tui_places_cursor_inside_input_box() {
        let mut state = TuiState::new(SessionInfo {
            session_id: "session-1".to_string(),
            provider: "openai".to_string(),
            model: "gpt-5.4".to_string(),
            mode: "default".to_string(),
        });
        state.set_input_text("abcd");
        state.input.move_cursor(CursorMove::Jump(0, 2));
        let backend = TestBackend::new(100, 32);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| render_state(frame, &state)).unwrap();

        terminal
            .backend_mut()
            .assert_cursor_position(Position { x: 3, y: 30 });
    }
}
