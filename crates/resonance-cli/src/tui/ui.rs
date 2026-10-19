//! Drawing the TUI's screens (MVP-041).

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Wrap};
use serde_json::Value;

use super::app::{App, Confirm, FormRow, Screen};

const HELP: &str = "\
List
  up/down, j/k   move            enter   details
  /              filter by text  t       cycle the type filter
  s              cycle the status filter                esc   clear filters
  a              add an item     e       edit            r     retire
  l              the change log  g       reload          q     quit

Details
  up/down, j/k   scroll the text          n/p     next/previous item
  pgup/pgdn      scroll a page            esc     back

Change log
  up/down, j/k   select an entry          pgup/pgdn  move a page
  d              clear the selected entry D          clear them all (asks first)
  esc            back

Edit and add
  up/down, tab   move between fields
  typing         edits short fields; enter on a long field opens $VISUAL or $EDITOR
  left/right     (adding) change the type
  ctrl-s or F2   save            esc     cancel

Items your agent brought in from a document show where they came from; editing one marks
the document for your agent to update.
Changes you make here are recorded in the change log (l) for your agent to pick up; items
with a pending change are marked •. Clear an entry yourself when nothing more is needed.

Press any key to go back.";

fn one_line(s: &str, max: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        let mut t: String = flat.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(2),
    ])
    .areas(f.area());
    draw_header(f, app, header);
    match app.screen {
        Screen::Confirm if matches!(app.confirm, Some(Confirm::ClearLog { .. })) => {
            draw_log(f, app, body)
        }
        Screen::List | Screen::Confirm => draw_list(f, app, body),
        Screen::Detail => draw_detail(f, app, body),
        Screen::Form => draw_form(f, app, body),
        Screen::Log => draw_log(f, app, body),
        Screen::Help => {
            f.render_widget(
                Paragraph::new(HELP).block(Block::default().borders(Borders::ALL).title(" Help ")),
                body,
            );
        }
    }
    if app.screen == Screen::Confirm {
        draw_confirm(f, app, body);
    }
    draw_footer(f, app, footer);
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![Span::styled(
        " Resonance spec ",
        Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED),
    )];
    let filters = [
        app.filter_type.map(|t| format!("type: {t}")),
        app.filter_status.as_ref().map(|s| format!("status: {s}")),
        (!app.filter_text.is_empty()).then(|| format!("text: {}", app.filter_text)),
    ];
    for x in filters.into_iter().flatten() {
        spans.push(Span::raw(format!("  [{x}]")));
    }
    spans.push(Span::raw(format!("  {} item(s)", app.items.len())));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let hints = match (app.screen, app.input.is_some()) {
        (_, true) => "type to filter · enter apply · esc cancel",
        (Screen::List, _) => {
            "enter details · / filter · t type · s status · a add · e edit · r retire · l log · ? help · q quit"
        }
        (Screen::Detail, _) => {
            "j/k scroll · n/p item · pgup/pgdn page · esc back · e edit · r retire · ? help · q quit"
        }
        (Screen::Form, _) => {
            "tab/↑↓ move · type to edit · enter: long field in $EDITOR · ctrl-s save · esc cancel"
        }
        (Screen::Confirm, _) => match app.confirm {
            Some(Confirm::ClearLog { .. }) => "y clear them all · any other key: keep",
            _ => "y retire · any other key: keep",
        },
        (Screen::Log, _) => "j/k select · d clear the entry · D clear all · esc back · q quit",
        (Screen::Help, _) => "any key: back",
    };
    let mut lines = vec![Line::from(Span::styled(
        hints,
        Style::default().fg(Color::DarkGray),
    ))];
    if let Some(input) = &app.input {
        lines.push(Line::from(format!("/{}▏", input.buffer)));
    } else if let Some((m, err)) = &app.message {
        let style = if *err {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::Green)
        };
        lines.push(Line::from(Span::styled(
            one_line(m, area.width as usize),
            style,
        )));
    }
    f.render_widget(Paragraph::new(lines), area);
}

fn draw_list(f: &mut Frame, app: &mut App, area: Rect) {
    let rows: Vec<Row> = app
        .items
        .iter()
        .map(|i| {
            let links = match (i.links, i.stale_links) {
                (0, _) => String::new(),
                (n, 0) => format!("{n}"),
                (n, s) => format!("{n} ({s} stale)"),
            };
            let style = if i.status == "retired" || i.status == "superseded" {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default()
            };
            Row::new(vec![
                // • marks an item whose change your agent has not handled yet (MVP-040).
                Cell::from(format!(
                    "{}{}",
                    if i.pending_change.is_some() {
                        "• "
                    } else {
                        "  "
                    },
                    i.key.clone().unwrap_or_default()
                )),
                Cell::from(i.ntype.to_string()),
                Cell::from(i.status.clone()),
                Cell::from(one_line(&i.title, 200)),
                Cell::from(links),
                Cell::from(if i.source.is_some() { "doc" } else { "" }),
            ])
            .style(style)
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(16),
            Constraint::Length(11),
            Constraint::Length(11),
            Constraint::Min(20),
            Constraint::Length(13),
            Constraint::Length(4),
        ],
    )
    .header(
        Row::new(["  Key", "Type", "Status", "Title", "Links", "Src"])
            .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .block(Block::default().borders(Borders::ALL))
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    if app.items.is_empty() {
        let text = if app.filter_text.is_empty()
            && app.filter_type.is_none()
            && app.filter_status.is_none()
        {
            "No spec items yet. Press a to add one, or ask your agent to bring your requirements documents into Resonance."
        } else {
            "Nothing matches the filters (esc clears them)."
        };
        f.render_widget(
            Paragraph::new(text).block(Block::default().borders(Borders::ALL)),
            area,
        );
        return;
    }
    f.render_stateful_widget(table, area, &mut app.table);
}

fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn push_field(lines: &mut Vec<Line<'static>>, bold: Style, key: &str, text: &str) {
    let mut rows = text.lines();
    let Some(first) = rows.next() else {
        lines.push(Line::from(Span::styled(format!("{key}: "), bold)));
        return;
    };
    lines.push(Line::from(vec![
        Span::styled(format!("{key}: "), bold),
        Span::raw(first.to_string()),
    ]));
    for l in rows {
        lines.push(Line::from(format!("  {l}")));
    }
}

fn detail_lines(d: &Value) -> Vec<Line<'static>> {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let mut lines: Vec<Line> = Vec::new();
    let node = &d["node"];
    lines.push(Line::from(vec![
        Span::styled(format!("{} ", value_text(&node["label"])), bold),
        Span::raw(format!(
            "{} · {}",
            value_text(&node["type"]),
            value_text(&node["status"])
        )),
    ]));
    let props = d["properties"].as_object().cloned().unwrap_or_default();
    if let Some(p) = d.get("pendingChange").filter(|p| !p.is_null()) {
        let fields: Vec<String> = p["fields"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|f| f.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        lines.push(Line::from(Span::styled(
            format!(
                "• Pending change #{} ({}{}): not handled by your agent yet",
                value_text(&p["seq"]),
                value_text(&p["kind"]),
                if fields.is_empty() {
                    String::new()
                } else {
                    format!(": {}", fields.join(", "))
                }
            ),
            Style::default().fg(Color::Yellow),
        )));
    }
    if let Some(src) = d.get("source").filter(|s| !s.is_null()) {
        lines.push(Line::from(format!(
            "From: {}:{} (edits here mark the document for your agent to update)",
            value_text(&src["path"]),
            value_text(&src["line"])
        )));
    }
    lines.push(Line::from(""));
    let skip = ["source", "alsoDefinedIn", "key", "specStatus", "origin"];
    for (k, v) in &props {
        if skip.contains(&k.as_str()) || v.is_null() {
            continue;
        }
        push_field(&mut lines, bold, k, &value_text(v));
    }
    lines.push(Line::from(""));
    let links = d["links"].as_array().cloned().unwrap_or_default();
    lines.push(Line::from(Span::styled(
        format!("Links ({})", links.len()),
        bold,
    )));
    for l in &links {
        let mut s = format!(
            "  {} {} [{}]",
            value_text(&l["relation"]),
            value_text(&l["other"]["label"]),
            value_text(&l["status"])
        );
        if let Some(e) = l.get("latestEvidence").filter(|e| !e.is_null()) {
            s.push_str(&format!(
                " · latest test: {} at {}",
                value_text(&e["result"]),
                value_text(&e["timestamp"])
            ));
        }
        lines.push(Line::from(s));
    }
    let notes = d["annotations"].as_array().cloned().unwrap_or_default();
    if !notes.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("Annotations ({})", notes.len()),
            bold,
        )));
        for a in &notes {
            lines.push(Line::from(format!(
                "  {}: {} ({})",
                value_text(&a["kind"]),
                one_line(&value_text(&a["text"]), 200),
                value_text(&a["author"])
            )));
        }
    }
    lines
}

fn draw_detail(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(d) = app.detail.clone() else {
        f.render_widget(Paragraph::new("Nothing selected."), area);
        return;
    };
    let lines = detail_lines(&d);
    // The border takes two rows; scroll is in lines of the paragraph inside it.
    let inner = area.height.saturating_sub(2) as usize;
    let max = lines.len().saturating_sub(inner) as u16;
    app.detail_scroll = app.detail_scroll.min(max);
    let scroll = app.detail_scroll;
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0))
            .block(Block::default().borders(Borders::ALL).title(" Details ")),
        area,
    );
}

fn draw_form(f: &mut Frame, app: &App, area: Rect) {
    let Some(form) = &app.form else { return };
    let focus = form.focus();
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let mark = |on: bool| if on { "› " } else { "  " };
    let mut lines: Vec<Line> = Vec::new();
    if form.editing.is_none() {
        lines.push(Line::from(vec![
            Span::raw(mark(focus == FormRow::Type)),
            Span::styled("type: ", bold),
            Span::raw(format!("◂ {} ▸", form.ntype)),
        ]));
        lines.push(Line::from(vec![
            Span::raw(mark(focus == FormRow::Key)),
            Span::styled("key: ", bold),
            Span::raw(form.key.clone()),
            Span::styled(
                "  (suggested; must be new)",
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }
    for (i, field) in form.fields.iter().enumerate() {
        let on = focus == FormRow::Field(i);
        let shown = if field.long {
            let first = field.value.lines().next().unwrap_or_default();
            let more = if field.value.lines().count() > 1 {
                " …"
            } else {
                ""
            };
            format!("{}{more}  [enter: edit in $EDITOR]", one_line(first, 120))
        } else if on {
            format!("{}▏", field.value)
        } else {
            field.value.clone()
        };
        let changed = field.value != field.original;
        lines.push(Line::from(vec![
            Span::raw(mark(on)),
            Span::styled(
                format!("{}{}: ", field.name, if changed { "*" } else { "" }),
                bold,
            ),
            Span::raw(shown),
        ]));
    }
    let title = match &form.editing {
        Some(_) => format!(" Edit {} ", form.label),
        None => format!(" Add {} ", form.ntype),
    };
    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title)),
        area,
    );
}

fn draw_confirm(f: &mut Frame, app: &App, area: Rect) {
    let Some(c) = &app.confirm else { return };
    let lines = match c {
        Confirm::Retire { label, links, .. } => vec![
            Line::from(format!("Retire {label}?")),
            Line::from(format!(
                "It has {links} link(s); they stay, and it is never deleted."
            )),
            Line::from("y: retire · any other key: keep"),
        ],
        Confirm::ClearLog { count } => vec![
            Line::from(format!("Clear all {count} change-log entries?")),
            Line::from("Your agent will not see them. The changes themselves stay."),
            Line::from("y: clear them all · any other key: keep"),
        ],
    };
    let w = area.width.min(64);
    let popup = Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + area.height / 3,
        width: w,
        height: 5.min(area.height),
    };
    f.render_widget(Clear, popup);
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL).title(" Confirm ")),
        popup,
    );
}

fn draw_log(f: &mut Frame, app: &mut App, area: Rect) {
    if app.log.is_empty() {
        f.render_widget(
            Paragraph::new("The change log is empty: agents have handled every change.")
                .block(Block::default().borders(Borders::ALL).title(" Change log ")),
            area,
        );
        return;
    }
    let max = app.log.len().saturating_sub(1);
    app.log_offset = app.log_offset.min(max);
    app.log_cursor = app.log_cursor.min(max);
    let mut state = TableState::new()
        .with_offset(app.log_offset)
        .with_selected(Some(app.log_cursor));
    let rows: Vec<Row> = app
        .log
        .iter()
        .map(|e| {
            Row::new(vec![
                Cell::from(e.seq.to_string()),
                Cell::from(e.at.get(..16).unwrap_or(&e.at).replace('T', " ")),
                Cell::from(e.origin.clone()),
                Cell::from(e.actor.clone()),
                Cell::from(e.summary.clone()),
            ])
        })
        .collect();
    f.render_stateful_widget(
        Table::new(
            rows,
            [
                Constraint::Length(5),
                Constraint::Length(17),
                Constraint::Length(7),
                Constraint::Length(24),
                Constraint::Min(20),
            ],
        )
        .header(
            Row::new(["Seq", "When", "Origin", "Actor", "Summary"])
                .style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" Change log: {} pending ", app.log.len())),
        )
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        area,
        &mut state,
    );
    app.log_offset = state.offset();
}
