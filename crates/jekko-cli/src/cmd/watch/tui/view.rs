use anyhow::{Context, Result};
use ratatui::backend::TestBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::{Frame, Terminal};

use super::DashboardState;
use crate::cmd::watch::rule_label;

/// Render one frame into a `TestBackend`, dump the rendered text to stdout,
/// and return. The output is plain (no ANSI), so tests can grep it.
pub(super) fn render_once_snapshot(dash: &DashboardState) -> Result<()> {
    const W: u16 = 120;
    const H: u16 = 30;
    let backend = TestBackend::new(W, H);
    let mut terminal = Terminal::new(backend).context("init test backend")?;
    terminal
        .draw(|f| draw_dashboard(f, dash))
        .context("draw to test backend")?;
    // `Buffer::to_string` would lose line breaks; iterate row-by-row instead.
    let buf = terminal.backend().buffer().clone();
    let mut out = String::with_capacity((W as usize + 1) * H as usize);
    for y in 0..H {
        for x in 0..W {
            let cell = &buf[(x, y)];
            out.push_str(cell.symbol());
        }
        out.push('\n');
    }
    println!("{out}");
    Ok(())
}

/// Render the dashboard widgets into the frame. Pure with respect to `dash` -
/// callable from both the real terminal and `TestBackend`.
pub(super) fn draw_dashboard(f: &mut Frame<'_>, dash: &DashboardState) {
    let area = f.area();
    let outer_block = Block::default()
        .borders(Borders::ALL)
        .title(Line::from(vec![
            Span::raw(" ZYAL Watcher: "),
            Span::styled(
                dash.run_id.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw("   elapsed "),
            Span::styled(
                dash.elapsed_label(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]));
    let inner = outer_block.inner(area);
    f.render_widget(outer_block, area);

    // Vertical layout: top stats row, rules list, jankurai row, hint.
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6), // top stat panes
            Constraint::Min(3),    // active rules
            Constraint::Length(3), // jankurai row
            Constraint::Length(1), // key hint
        ])
        .split(inner);

    draw_stat_row(f, chunks[0], dash);
    draw_rules(f, chunks[1], dash);
    draw_jankurai(f, chunks[2], dash);
    draw_hint(f, chunks[3]);
}

fn draw_stat_row(f: &mut Frame<'_>, area: Rect, dash: &DashboardState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(area);

    let lanes = vec![
        Line::from(format!("started:      {}", dash.snap.lanes_started)),
        Line::from(format!("finished:     {}", dash.snap.lanes_finished)),
        Line::from(format!("workers ok:   {}", dash.snap.workers_pass)),
        Line::from(format!("workers fail: {}", dash.snap.workers_fail)),
    ];
    let lanes_widget =
        Paragraph::new(lanes).block(Block::default().borders(Borders::ALL).title(" Lanes "));
    f.render_widget(lanes_widget, cols[0]);

    let parity = vec![
        Line::from(format!("open:    {}", dash.snap.parity_gaps_open)),
        Line::from(format!("closed:  {}", dash.snap.parity_gaps_closed)),
    ];
    let parity_widget =
        Paragraph::new(parity).block(Block::default().borders(Borders::ALL).title(" Parity "));
    f.render_widget(parity_widget, cols[1]);

    let err_pct = dash.snap.error_rate() * 100.0;
    let model = vec![
        Line::from(format!("attempts:    {}", dash.snap.model_attempts)),
        Line::from(format!("failures:    {}", dash.snap.model_failures)),
        Line::from(format!("error rate:  {err_pct:.1}%")),
        Line::from(format!("spend (usd): ${:.2}", dash.snap.model_spend_usd)),
    ];
    let model_widget =
        Paragraph::new(model).block(Block::default().borders(Borders::ALL).title(" Model "));
    f.render_widget(model_widget, cols[2]);
}

fn draw_rules(f: &mut Frame<'_>, area: Rect, dash: &DashboardState) {
    let items: Vec<ListItem> = if dash.actions.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "none firing",
            Style::default().add_modifier(Modifier::DIM),
        )))]
    } else {
        dash.actions
            .iter()
            .skip(dash.rules_scroll)
            .map(|a| {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        rule_label(a.rule),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                    Span::raw(a.summary.clone()),
                ]))
            })
            .collect()
    };
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Active rules "),
    );
    f.render_widget(list, area);
}

fn draw_jankurai(f: &mut Frame<'_>, area: Rect, dash: &DashboardState) {
    let score = dash
        .snap
        .last_jankurai_score
        .map(|s| s.to_string())
        .unwrap_or_else(|| "-".into());
    let hard = dash
        .snap
        .last_jankurai_hard_findings
        .map(|h| h.to_string())
        .unwrap_or_else(|| "-".into());
    let line = Line::from(format!("score: {score}        hard_findings: {hard}"));
    let widget = Paragraph::new(line)
        .wrap(Wrap { trim: true })
        .block(Block::default().borders(Borders::ALL).title(" Jankurai "));
    f.render_widget(widget, area);
}

fn draw_hint(f: &mut Frame<'_>, area: Rect) {
    let widget = Paragraph::new(Line::from(Span::styled(
        " q quit  |  j/k scroll rules ",
        Style::default().add_modifier(Modifier::DIM),
    )));
    f.render_widget(widget, area);
}
