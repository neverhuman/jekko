//! Slash-command popup state and overlay widget.
//!
//! Detects when `/` appears at column 0 and exposes a filterable command list.
//! The actual command catalog is owned by the host crate (jekko-cli); the
//! prompt module ships with a small built-in command list so the widget is usable
//! in isolation.
//!
//! [`SlashPopupOverlay`] is a Ratatui widget that renders the filtered list
//! above the prompt panel when the popup is open.

/// One row in the slash popup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlashCommand {
    /// Stable identifier (e.g. `"help"`, `"quit"`).
    pub id: String,
    /// Display label without the leading slash (e.g. `"help"`).
    pub label: String,
    /// Optional one-line description.
    pub description: Option<String>,
}

impl SlashCommand {
    /// Construct a command with no description.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: None,
        }
    }

    /// Attach a description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Built-in slash commands.
pub fn builtin_commands() -> Vec<SlashCommand> {
    vec![
        SlashCommand::new("help", "help").with_description("Show keybind reference"),
        SlashCommand::new("quit", "quit").with_description("Exit Jekko"),
        SlashCommand::new("new", "new").with_description("Start a new session"),
        SlashCommand::new("model", "model").with_description("Pick a model"),
        SlashCommand::new("theme", "theme").with_description("Pick a theme"),
        SlashCommand::new("audit", "audit").with_description("Run jankurai audit"),
        SlashCommand::new("audit-check", "audit-check")
            .with_description("Check for duplicate code"),
        SlashCommand::new("jankurai-status", "jankurai-status")
            .with_description("Show current jankurai score"),
        SlashCommand::new("jankurai", "jankurai")
            .with_description("Full cycle: audit → analyze → fix → verify → reaudit"),
    ]
}

/// Slash popup state.
#[derive(Clone, Debug)]
pub struct SlashPopup {
    catalog: Vec<SlashCommand>,
    /// Lowercased query (whatever follows the leading `/`).
    query: String,
    /// Cursor into `filtered()`.
    cursor: usize,
    open: bool,
}

impl Default for SlashPopup {
    fn default() -> Self {
        Self::with_catalog(builtin_commands())
    }
}

impl SlashPopup {
    /// Build a popup over an explicit catalog.
    pub fn with_catalog(catalog: Vec<SlashCommand>) -> Self {
        Self {
            catalog,
            query: String::new(),
            cursor: 0,
            open: false,
        }
    }

    /// Replace the catalog (e.g. once Packet B populates it).
    pub fn set_catalog(&mut self, catalog: Vec<SlashCommand>) {
        self.catalog = catalog;
        self.cursor = 0;
    }

    /// Whether the popup is currently visible.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Open the popup with an empty query.
    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.cursor = 0;
    }

    /// Close the popup.
    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.cursor = 0;
    }

    /// Update the query string. Resets the cursor.
    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into().to_lowercase();
        self.cursor = 0;
    }

    /// Current cursor position.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Move cursor by `delta`, wrapping around the filtered list.
    pub fn move_cursor(&mut self, delta: isize) {
        let len = self.filtered().len();
        if len == 0 {
            self.cursor = 0;
            return;
        }
        let len_i = len as isize;
        let mut idx = self.cursor as isize + delta;
        while idx < 0 {
            idx += len_i;
        }
        self.cursor = (idx % len_i) as usize;
    }

    /// Filtered command list (prefix match against the query).
    pub fn filtered(&self) -> Vec<SlashCommand> {
        if self.query.is_empty() {
            return self.catalog.clone();
        }
        self.catalog
            .iter()
            .filter(|c| c.label.to_lowercase().starts_with(&self.query))
            .cloned()
            .collect()
    }

    /// Currently selected entry, if any.
    pub fn selected(&self) -> Option<SlashCommand> {
        self.filtered().get(self.cursor).cloned()
    }

    /// Inspect the current query.
    pub fn query(&self) -> &str {
        &self.query
    }
}

/// True if `buffer` matches the slash-popup trigger condition. The popup opens
/// when the buffer begins with `/` and still contains a slash command token.
pub fn buffer_triggers_slash(buffer: &str) -> bool {
    buffer
        .strip_prefix('/')
        .map(|rest| !rest.trim_start().is_empty())
        .unwrap_or(false)
}

/// Extract the current slash-token query from a triggering buffer (slash
/// already stripped; only the command token before any args is returned).
pub fn query_from_buffer(buffer: &str) -> &str {
    let rest = buffer.strip_prefix('/').unwrap_or(buffer);
    rest.trim_start().split_whitespace().next().unwrap_or("")
}

// ---------------------------------------------------------------------------
// SlashPopupOverlay — Ratatui widget
// ---------------------------------------------------------------------------

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget};

const GOLD: Color = Color::Rgb(0xd4, 0xa8, 0x43);
const TEXT: Color = Color::Rgb(0xd8, 0xde, 0xe9);
const TEXT_DIM: Color = Color::Rgb(0x52, 0x57, 0x60);
const BORDER: Color = Color::Rgb(0x4a, 0x52, 0x60);
const BG_PANEL: Color = Color::Rgb(0x0b, 0x0f, 0x14);

/// Overlay widget that renders the slash popup above the composer panel.
///
/// The caller is responsible for computing the `Rect` and passing it to
/// `frame.render_widget(SlashPopupOverlay::new(popup), rect)`. The widget
/// clears its area first so it paints on top of transcript content.
pub struct SlashPopupOverlay<'a> {
    popup: &'a SlashPopup,
}

impl<'a> SlashPopupOverlay<'a> {
    pub fn new(popup: &'a SlashPopup) -> Self {
        Self { popup }
    }

    /// Number of items in the filtered list — used by callers to size the rect.
    pub fn item_count(&self) -> usize {
        self.popup.filtered().len()
    }
}

impl Widget for SlashPopupOverlay<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(BORDER))
            .style(Style::default().bg(BG_PANEL));
        let inner = block.inner(area);
        block.render(area, buf);

        let filtered = self.popup.filtered();
        let cursor = self.popup.cursor();
        let lines: Vec<Line> = filtered
            .iter()
            .enumerate()
            .map(|(i, cmd)| {
                let active = i == cursor;
                let label_style = if active {
                    Style::default().fg(GOLD).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(TEXT)
                };
                let prefix = if active { "\u{25b8} " } else { "  " };
                let mut spans = vec![
                    Span::styled(prefix, label_style),
                    Span::styled(format!("/{}", cmd.label), label_style),
                ];
                if let Some(desc) = &cmd.description {
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(desc.clone(), Style::default().fg(TEXT_DIM)));
                }
                Line::from(spans)
            })
            .collect();

        if lines.is_empty() {
            let hint = Line::from(Span::styled(
                "  no commands match",
                Style::default().fg(TEXT_DIM),
            ));
            Paragraph::new(vec![hint]).render(inner, buf);
        } else {
            Paragraph::new(lines).render(inner, buf);
        }
    }
}
