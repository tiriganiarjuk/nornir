use std::io;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;

use crate::assembly::estimate_tokens;
use crate::model::{AppState, Section, TuiOutcome};

// --- Color palette ---
const ACCENT: Color = Color::Rgb(110, 180, 255);   // soft blue — active borders, keys
const GOLD: Color = Color::Rgb(255, 200, 80);       // warm gold — cursor row
const SELECTED: Color = Color::Rgb(120, 230, 160);  // green — selected items
const UNSELECTED: Color = Color::Rgb(150, 150, 165); // readable grey — unselected items
const DIM: Color = Color::Rgb(90, 90, 105);         // dim but legible — disabled/annotations
const TITLE: Color = Color::Rgb(190, 190, 205);     // light grey — section titles
const SUMMARY_BORDER: Color = Color::Rgb(120, 230, 160); // green — summary panel
const TOKEN_COLOR: Color = Color::Rgb(255, 160, 80); // orange — token estimate
const SPACE_COLOR: Color = Color::Rgb(200, 140, 255); // purple — auto-loaded space
const LABEL: Color = Color::Rgb(160, 160, 175);     // label text

// --- Selection indicators ---
const RADIO_ON: &str = "◉";
const RADIO_OFF: &str = "○";
const CHECK_ON: &str = "◆";
const CHECK_OFF: &str = "◇";
const CURSOR_ARROW: &str = "▸";

pub fn run_tui(state: &mut AppState) -> Result<TuiOutcome, String> {
    enable_raw_mode().map_err(|e| format!("enable raw mode: {e}"))?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(|e| format!("enter alt screen: {e}"))?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| format!("create terminal: {e}"))?;

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(panic_info);
    }));

    let outcome = run_event_loop(&mut terminal, state);

    disable_raw_mode().map_err(|e| format!("disable raw mode: {e}"))?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .map_err(|e| format!("leave alt screen: {e}"))?;
    terminal
        .show_cursor()
        .map_err(|e| format!("show cursor: {e}"))?;

    outcome
}

fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut AppState,
) -> Result<TuiOutcome, String> {
    loop {
        terminal
            .draw(|frame| draw_ui(frame, state))
            .map_err(|e| format!("draw: {e}"))?;

        if let Event::Key(key) = event::read().map_err(|e| format!("read event: {e}"))? {
            match handle_key(state, key) {
                KeyAction::Continue => {}
                KeyAction::Launch => return Ok(TuiOutcome::Launch),
                KeyAction::Quit => return Ok(TuiOutcome::Quit),
            }
        }
    }
}

enum KeyAction {
    Continue,
    Launch,
    Quit,
}

fn handle_key(state: &mut AppState, key: KeyEvent) -> KeyAction {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => return KeyAction::Quit,
        KeyCode::Enter => return KeyAction::Launch,
        KeyCode::Tab => {
            state.active_section = if key.modifiers.contains(KeyModifiers::SHIFT) {
                state.active_section.prev()
            } else {
                state.active_section.next()
            };
            state.section_cursor = 0;
            return KeyAction::Continue;
        }
        KeyCode::BackTab => {
            state.active_section = state.active_section.prev();
            state.section_cursor = 0;
            return KeyAction::Continue;
        }
        _ => {}
    }

    let section_len = state.section_len(state.active_section);

    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            if state.section_cursor > 0 {
                state.section_cursor -= 1;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.section_cursor + 1 < section_len {
                state.section_cursor += 1;
            }
        }
        KeyCode::Char(' ') => handle_space(state),
        _ => {}
    }

    KeyAction::Continue
}

fn handle_space(state: &mut AppState) {
    match state.active_section {
        Section::Workspace => {
            if state.section_cursor == 0 {
                state.clear_workspace_profile();
            } else {
                let profile_index = state.section_cursor - 1;
                if profile_index < state.profiles.len() {
                    state.apply_workspace_profile(profile_index);
                }
            }
        }
        Section::Persona => state.choose_persona(state.section_cursor),
        Section::Descriptors => {
            // Map cursor position to actual descriptor index (systems only)
            if let Some(desc_index) = state.system_cursor_to_index(state.section_cursor) {
                if let Some(selected) = state.selected_descriptors.get_mut(desc_index) {
                    *selected = !*selected;
                }
                state.sync_languages_from_descriptors();
            }
        }
        Section::Coding => handle_coding_space(state),
        Section::Expertise => {
            if let Some(selected) = state.selected_expertise.get_mut(state.section_cursor) {
                *selected = !*selected;
            }
        }
    }
}

fn handle_coding_space(state: &mut AppState) {
    if state.section_cursor == 0 {
        state.toggle_coding();
        return;
    }
    if !state.coding_enabled {
        return; // languages locked when coding disabled
    }
    let lang_index = state.section_cursor - 1;
    if lang_index == 0 || lang_index >= state.library.languages.len() {
        return; // python always on, bounds check
    }
    if let Some(selected) = state.selected_languages.get_mut(lang_index) {
        *selected = !*selected;
    }
}

// --- Drawing ---

fn draw_ui(frame: &mut ratatui::Frame, state: &AppState) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(2)])
        .split(frame.area());

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Percentage(35),
            Constraint::Percentage(35),
        ])
        .split(outer[0]);

    draw_identity_column(frame, columns[0], state);
    draw_content_column(frame, columns[1], state);
    draw_summary_panel(frame, columns[2], state);
    draw_key_bar(frame, outer[1], state.update_mode);
}

fn draw_identity_column(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let workspace_height = 2 + state.profiles.len() as u16 + 1;
    let persona_height = 2 + state.library.personas.len() as u16 + 1;

    // Persona is drawn first because it is chosen first.
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(persona_height),
            Constraint::Length(workspace_height),
            Constraint::Min(0), // Expertise fills remaining
        ])
        .split(area);

    draw_persona_section(frame, sections[0], state);
    draw_workspace_section(frame, sections[1], state);
    draw_expertise_section(frame, sections[2], state);
}

fn draw_content_column(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let coding_height = 2 + 1 + state.library.languages.len() as u16;

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0), // Descriptors fill remaining
            Constraint::Length(coding_height),
        ])
        .split(area);

    draw_descriptor_section(frame, sections[0], state);
    draw_coding_section(frame, sections[1], state);
}

fn section_block(title: &str, active: bool) -> Block<'_> {
    Block::default()
        .title(format!(" {title} "))
        .title_style(if active {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(TITLE)
        })
        .borders(Borders::ALL)
        .border_style(if active {
            Style::default().fg(ACCENT)
        } else {
            Style::default().fg(DIM)
        })
}

/// Build a styled line for a radio-select item (one-of-many)
fn radio_line(label: &str, selected: bool, active: bool, is_cursor: bool) -> Line<'static> {
    let prefix = if active && is_cursor {
        Span::styled(format!(" {CURSOR_ARROW} "), Style::default().fg(GOLD).add_modifier(Modifier::BOLD))
    } else {
        Span::raw("   ")
    };

    let marker = if selected { RADIO_ON } else { RADIO_OFF };
    let marker_color = if selected { SELECTED } else { UNSELECTED };
    let marker_span = Span::styled(format!("{marker} "), Style::default().fg(marker_color));

    let label_style = if active && is_cursor {
        Style::default().fg(GOLD).add_modifier(Modifier::BOLD)
    } else if selected {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(UNSELECTED)
    };

    Line::from(vec![prefix, marker_span, Span::styled(label.to_string(), label_style)])
}

/// Build a styled line for a checkbox item (multi-select)
fn check_line(label: &str, selected: bool, active: bool, is_cursor: bool, suffix: Option<&str>) -> Line<'static> {
    let prefix = if active && is_cursor {
        Span::styled(format!(" {CURSOR_ARROW} "), Style::default().fg(GOLD).add_modifier(Modifier::BOLD))
    } else {
        Span::raw("   ")
    };

    let marker = if selected { CHECK_ON } else { CHECK_OFF };
    let marker_color = if selected { SELECTED } else { UNSELECTED };
    let marker_span = Span::styled(format!("{marker} "), Style::default().fg(marker_color));

    let label_style = if active && is_cursor {
        Style::default().fg(GOLD).add_modifier(Modifier::BOLD)
    } else if selected {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(UNSELECTED)
    };

    let mut spans = vec![prefix, marker_span, Span::styled(label.to_string(), label_style)];

    if let Some(s) = suffix {
        spans.push(Span::styled(format!(" {s}"), Style::default().fg(DIM)));
    }

    Line::from(spans)
}

/// Dimmed checkbox line for disabled state
fn check_line_dim(label: &str, selected: bool) -> Line<'static> {
    let marker = if selected { CHECK_ON } else { CHECK_OFF };
    Line::from(Span::styled(
        format!("   {marker} {label}"),
        Style::default().fg(DIM),
    ))
}

fn draw_workspace_section(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let active = state.active_section == Section::Workspace;
    let block = section_block("Workspace", active);

    let mut lines = Vec::new();

    let auto_selected = state.selected_workspace.is_none();
    lines.push(radio_line("Auto (current dir)", auto_selected, active, state.section_cursor == 0));

    for (index, profile) in state.profiles.iter().enumerate() {
        let selected = state.selected_workspace == Some(index);
        let name = capitalize(&profile.name);
        lines.push(radio_line(&name, selected, active, state.section_cursor == index + 1));
    }

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_persona_section(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let active = state.active_section == Section::Persona;
    let block = section_block("Persona", active);

    let mut lines = Vec::new();

    for (index, persona) in state.library.personas.iter().enumerate() {
        let selected = state.selected_persona == Some(index);
        lines.push(radio_line(&persona.display_name, selected, active, state.section_cursor == index));
    }

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_descriptor_section(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let active = state.active_section == Section::Descriptors;
    let block = section_block("Systems", active);

    let system_indices = state.library.system_indices();
    let mut lines = Vec::new();

    for (cursor_pos, &desc_index) in system_indices.iter().enumerate() {
        let descriptor = match state.library.descriptors.get(desc_index) {
            Some(d) => d,
            None => continue,
        };
        let selected = state.selected_descriptors.get(desc_index).copied().unwrap_or(false);
        let is_cursor = active && state.section_cursor == cursor_pos;

        let parent_suffix = descriptor.parent.as_ref().map(|p| format!("({})", p));

        let mut spans = Vec::new();

        if is_cursor {
            spans.push(Span::styled(format!(" {CURSOR_ARROW} "), Style::default().fg(GOLD).add_modifier(Modifier::BOLD)));
        } else {
            spans.push(Span::raw("   "));
        }

        let marker = if selected { CHECK_ON } else { CHECK_OFF };
        let marker_color = if selected { SELECTED } else { UNSELECTED };
        spans.push(Span::styled(format!("{marker} "), Style::default().fg(marker_color)));

        // Name part
        let name_style = if is_cursor {
            Style::default().fg(GOLD).add_modifier(Modifier::BOLD)
        } else if selected {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(UNSELECTED)
        };
        spans.push(Span::styled(descriptor.display_name.clone(), name_style));

        // Parent annotation dimmed
        if let Some(ps) = &parent_suffix {
            spans.push(Span::styled(format!(" {ps}"), Style::default().fg(DIM)));
        }

        lines.push(Line::from(spans));
    }

    // Scroll to keep cursor visible
    let visible_height = area.height.saturating_sub(2) as usize;
    let scroll_offset = if active && state.section_cursor >= visible_height {
        (state.section_cursor - visible_height + 1) as u16
    } else {
        0
    };

    frame.render_widget(
        Paragraph::new(lines).block(block).scroll((scroll_offset, 0)),
        area,
    );
}

fn draw_coding_section(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let active = state.active_section == Section::Coding;
    let block = section_block("Coding", active);

    let mut lines = Vec::new();

    // Inverted: checked = coding DISABLED
    lines.push(check_line(
        "Disable coding mode",
        !state.coding_enabled,
        active,
        state.section_cursor == 0,
        None,
    ));

    for (index, language) in state.library.languages.iter().enumerate() {
        let selected = state.selected_languages.get(index).copied().unwrap_or(false);

        if state.coding_enabled {
            let suffix = if index == 0 { Some("(always)") } else { None };
            lines.push(check_line(
                language,
                selected,
                active,
                state.section_cursor == index + 1,
                suffix,
            ));
        } else {
            lines.push(check_line_dim(language, selected));
        }
    }

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_expertise_section(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let active = state.active_section == Section::Expertise;
    let block = section_block("Expertise", active);

    let mut lines = Vec::new();
    for (index, fragment) in state.library.expertise.iter().enumerate() {
        let selected = state.selected_expertise.get(index).copied().unwrap_or(false);
        lines.push(check_line(
            &fragment.display_name,
            selected,
            active,
            state.section_cursor == index,
            None,
        ));
    }

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_summary_panel(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .title(" Session Summary ")
        .title_style(Style::default().fg(SUMMARY_BORDER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SUMMARY_BORDER));

    let mut lines = Vec::new();
    summary_workspace(&mut lines, state);
    summary_persona(&mut lines, state);
    summary_context(&mut lines, state);
    summary_coding(&mut lines, state);
    summary_expertise(&mut lines, state);
    summary_permissions(&mut lines, state);
    summary_always_loaded(&mut lines, state);
    summary_token_estimate(&mut lines, state);

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn summary_workspace(lines: &mut Vec<Line<'_>>, state: &AppState) {
    let workspace_name = match state.selected_workspace {
        Some(index) => state
            .profiles
            .get(index)
            .map(|profile| capitalize(&profile.name))
            .unwrap_or_else(|| "?".to_string()),
        None => "Auto (current dir)".to_string(),
    };
    lines.push(Line::from(vec![
        Span::styled("  Workspace  ", Style::default().fg(LABEL)),
        Span::styled(workspace_name, Style::default().fg(Color::White)),
    ]));
    lines.push(Line::from(""));
}

fn summary_persona<'a>(lines: &mut Vec<Line<'a>>, state: &'a AppState) {
    let persona_name = match state.selected_persona {
        Some(index) => state
            .library
            .personas
            .get(index)
            .map(|persona| persona.display_name.as_str())
            .unwrap_or("?"),
        None => "None",
    };
    lines.push(Line::from(vec![
        Span::styled("  Persona    ", Style::default().fg(LABEL)),
        Span::styled(persona_name, Style::default().fg(SELECTED).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from(""));
}

fn summary_context<'a>(lines: &mut Vec<Line<'a>>, state: &'a AppState) {
    let system_indices = state.ordered_descriptor_indices();
    let space_index = state.auto_space_index();
    let has_systems = !system_indices.is_empty();
    let has_space = space_index.is_some();

    if !has_systems && !has_space {
        lines.push(Line::from(vec![
            Span::styled("  Context    ", Style::default().fg(LABEL)),
            Span::styled("none", Style::default().fg(DIM)),
        ]));
    } else {
        lines.push(Line::from(Span::styled("  Context", Style::default().fg(LABEL))));
        let mut position = 1;

        // Space descriptor first (if auto-matched)
        if let Some(idx) = space_index {
            if let Some(d) = state.library.descriptors.get(idx) {
                lines.push(Line::from(vec![
                    Span::styled(format!("    {position}. "), Style::default().fg(LABEL)),
                    Span::styled(d.display_name.as_str(), Style::default().fg(SPACE_COLOR)),
                    Span::styled(" (auto)", Style::default().fg(DIM)),
                ]));
                position += 1;
            }
        }

        // Then system descriptors in order
        for &index in &system_indices {
            if let Some(d) = state.library.descriptors.get(index) {
                lines.push(Line::from(vec![
                    Span::styled(format!("    {position}. "), Style::default().fg(LABEL)),
                    Span::styled(d.display_name.as_str(), Style::default().fg(Color::White)),
                ]));
                position += 1;
            }
        }
    }
    lines.push(Line::from(""));
}

fn summary_coding(lines: &mut Vec<Line<'_>>, state: &AppState) {
    if state.coding_enabled {
        let langs = state.selected_language_names().join(", ");
        lines.push(Line::from(vec![
            Span::styled("  Coding     ", Style::default().fg(LABEL)),
            Span::styled(langs, Style::default().fg(ACCENT)),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled("  Coding     ", Style::default().fg(LABEL)),
            Span::styled("disabled", Style::default().fg(DIM)),
        ]));
    }
    lines.push(Line::from(""));
}

fn summary_expertise<'a>(lines: &mut Vec<Line<'a>>, state: &'a AppState) {
    let selected: Vec<&str> = state
        .library
        .expertise
        .iter()
        .enumerate()
        .filter(|(index, _)| state.selected_expertise.get(*index).copied().unwrap_or(false))
        .map(|(_, fragment)| fragment.display_name.as_str())
        .collect();

    if selected.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  Expertise  ", Style::default().fg(LABEL)),
            Span::styled("none", Style::default().fg(DIM)),
        ]));
    } else {
        lines.push(Line::from(Span::styled("  Expertise", Style::default().fg(LABEL))));
        for name in &selected {
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(format!("▹ {name}"), Style::default().fg(ACCENT)),
            ]));
        }
    }
    lines.push(Line::from(""));
}

fn summary_permissions(lines: &mut Vec<Line<'_>>, state: &AppState) {
    // Permissions are persona-bound, never selected. Show what the chosen
    // persona will carry so the grant is always visible before launch.
    let granted = state
        .selected_persona
        .and_then(|index| state.library.personas.get(index))
        .and_then(|persona| crate::permissions::permission_for_persona(&persona.id));

    match granted {
        None => {
            lines.push(Line::from(vec![
                Span::styled("  Permissions ", Style::default().fg(LABEL)),
                Span::styled("none", Style::default().fg(DIM)),
            ]));
        }
        Some(permission) => {
            lines.push(Line::from(vec![
                Span::styled("  Permissions ", Style::default().fg(LABEL)),
                Span::styled("(persona-bound)", Style::default().fg(DIM)),
            ]));
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(format!("▹ {}", permission.name), Style::default().fg(Color::Red)),
            ]));
        }
    }
    lines.push(Line::from(""));
}

fn summary_always_loaded(lines: &mut Vec<Line<'_>>, state: &AppState) {
    lines.push(Line::from(vec![
        Span::styled("  Always     ", Style::default().fg(LABEL)),
        Span::styled(format!("{} fragments", state.library.always.len()), Style::default().fg(DIM)),
    ]));
    lines.push(Line::from(""));
}

fn summary_token_estimate(lines: &mut Vec<Line<'_>>, state: &AppState) {
    let tokens = estimate_tokens(state);
    lines.push(Line::from(vec![
        Span::styled("  Tokens     ", Style::default().fg(LABEL)),
        Span::styled(format!("~{tokens}"), Style::default().fg(TOKEN_COLOR).add_modifier(Modifier::BOLD)),
    ]));
}

fn draw_key_bar(frame: &mut ratatui::Frame, area: Rect, update_mode: bool) {
    let action_label = if update_mode { " update  " } else { " launch  " };
    let keys = Line::from(vec![
        Span::raw(" "),
        Span::styled("Tab", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
        Span::styled(" section  ", Style::default().fg(LABEL)),
        Span::styled("↑↓", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
        Span::styled(" navigate  ", Style::default().fg(LABEL)),
        Span::styled("Space", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
        Span::styled(" toggle  ", Style::default().fg(LABEL)),
        Span::styled("Enter", Style::default().fg(SELECTED).add_modifier(Modifier::BOLD)),
        Span::styled(action_label, Style::default().fg(LABEL)),
        Span::styled("q", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::styled(" quit", Style::default().fg(LABEL)),
    ]);

    frame.render_widget(Paragraph::new(keys), area);
}

fn capitalize(input: &str) -> String {
    let mut chars = input.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().to_string() + chars.as_str(),
    }
}
