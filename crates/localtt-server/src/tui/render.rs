//! Pure rendering over [`TuiState`] (`docs/rfcs/server.md` 3).
//!
//! The renderer reads the state and draws one frame; it never performs I/O.
//! Tests use ratatui's `TestBackend`.

use localtt_core::types::{
    EntityKind,
    FeedPayload,
    HandoutContent,
    ResourceKind,
    RestKind,
    StatusDuration,
};
use localtt_engine::authz::{EntityView, EntityViewBody, StatblockView};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::tui::state::{CreateForm, Mode, TuiState};

/// How many feed lines the bottom pane shows.
const FEED_LINES: usize = 9;

/// Draws one frame.
#[inline]
pub fn draw(frame: &mut Frame<'_>, state: &TuiState) {
    let area = frame.area();
    let chunks = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(0),
        Constraint::Length(12),
        Constraint::Length(5),
    ])
    .split(area);
    draw_header(frame, chunks[0], state);
    let body =
        Layout::horizontal([Constraint::Percentage(34), Constraint::Min(0)]).split(chunks[1]);
    draw_entities(frame, body[0], state);
    draw_detail(frame, body[1], state);
    draw_feed(frame, chunks[2], state);
    draw_footer(frame, chunks[3], state);
    match &state.mode {
        Mode::Help => draw_help(frame, area),
        Mode::Create(form) => draw_create(frame, area, form, &state.status),
        Mode::Normal | Mode::Prompt(_) => {}
    }
}

fn draw_header(frame: &mut Frame<'_>, area: Rect, state: &TuiState) {
    let title = state
        .campaign
        .as_ref()
        .map_or("no campaign", |campaign| campaign.name.as_str());
    let actor = match state.campaign.as_ref() {
        Some(campaign) => campaign
            .roster
            .iter()
            .find(|entry| entry.id == state.actor)
            .map_or_else(
                || format!("{} (not on roster)", state.actor),
                |entry| format!("{} ({:?})", entry.display_name, entry.role),
            ),
        None => state.actor.to_string(),
    };
    let lines = vec![
        Line::from(vec![
            Span::styled("localtt", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::raw(title),
        ]),
        Line::from(format!("{}  |  actor: {}", state.clock_text(), actor)),
    ];
    let block = Block::bordered().title(" campaign ");
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_entities(frame: &mut Frame<'_>, area: Rect, state: &TuiState) {
    let items = state
        .entities
        .iter()
        .map(|entity| {
            ListItem::new(Line::from(format!(
                "{}  [{}] rev {}",
                entity.name.as_deref().unwrap_or("(unnamed)"),
                kind_label(entity.kind),
                entity.rev
            )))
        })
        .collect::<Vec<_>>();
    let list = List::new(items)
        .block(Block::bordered().title(" entities "))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("> ");
    let mut list_state = ListState::default();
    if !state.entities.is_empty() {
        list_state.select(Some(state.selected));
    }
    frame.render_stateful_widget(list, area, &mut list_state);
}

fn draw_detail(frame: &mut Frame<'_>, area: Rect, state: &TuiState) {
    let title = state
        .selected()
        .and_then(|entity| entity.name.as_deref())
        .map_or(" detail ".to_owned(), |name| format!(" {name} "));
    let lines = state
        .selected()
        .map_or_else(|| vec![Line::from("no entity selected")], detail_lines);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(title))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn detail_lines(entity: &EntityView) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(format!(
            "{}  [{}] rev {}",
            entity.name.as_deref().unwrap_or("(unnamed)"),
            kind_label(entity.kind),
            entity.rev
        )),
        Line::from(format!(
            "owner {}  |  {}",
            entity.owner,
            if entity.writable {
                "writable"
            } else {
                "read-only"
            }
        )),
    ];
    if !entity.hidden.is_empty() {
        lines.push(Line::from(format!(
            "hidden: {}",
            entity
                .hidden
                .iter()
                .map(|field| format!("{field:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    match &entity.body {
        Some(EntityViewBody::Statblock(statblock)) => {
            statblock_lines(statblock, &mut lines);
        }
        Some(EntityViewBody::Note(note)) => {
            lines.push(Line::from(format!("note: {}", note.text)));
        }
        Some(EntityViewBody::Handout(handout)) => match &handout.content {
            HandoutContent::Text { text } => lines.push(Line::from(format!("handout: {text}"))),
            HandoutContent::Item { item } => {
                lines.push(Line::from(format!("handout item: {}", item.name)));
            }
        },
        None => lines.push(Line::from("no visible body")),
    }
    lines
}

fn statblock_lines(statblock: &StatblockView, lines: &mut Vec<Line<'static>>) {
    if let Some(hp) = statblock.hp {
        lines.push(Line::from(format!(
            "HP {}/{}  temp {}",
            hp.current, hp.max, hp.temp
        )));
    }
    if let Some(abilities) = statblock.abilities {
        lines.push(Line::from(format!(
            "STR {} ({:+})  DEX {} ({:+})  CON {} ({:+})",
            abilities.strength,
            modifier(abilities.strength),
            abilities.dexterity,
            modifier(abilities.dexterity),
            abilities.constitution,
            modifier(abilities.constitution)
        )));
        lines.push(Line::from(format!(
            "INT {} ({:+})  WIS {} ({:+})  CHA {} ({:+})",
            abilities.intelligence,
            modifier(abilities.intelligence),
            abilities.wisdom,
            modifier(abilities.wisdom),
            abilities.charisma,
            modifier(abilities.charisma)
        )));
    }
    if let Some(speed) = statblock.speed {
        lines.push(Line::from(format!(
            "speed walk {}  fly {}  swim {}  climb {}",
            speed.walk_ft,
            speed.fly_ft.unwrap_or(0),
            speed.swim_ft.unwrap_or(0),
            speed.climb_ft.unwrap_or(0)
        )));
    }
    if let Some(saves) = statblock.death_saves {
        lines.push(Line::from(format!(
            "death saves {}/3 successes, {}/3 failures",
            saves.successes, saves.failures
        )));
    }
    if statblock.exhaustion > 0 || statblock.inspiration {
        lines.push(Line::from(format!(
            "exhaustion {}  inspiration {}",
            statblock.exhaustion, statblock.inspiration
        )));
    }
    if let Some(resources) = &statblock.resources {
        lines.push(Line::from("Resources"));
        for (index, resource) in resources.iter().enumerate() {
            lines.push(Line::from(format!(
                "  {}. {} {} [{}]",
                index + 1,
                resource.name,
                resource.current,
                resource_kind(resource.kind)
            )));
        }
    }
    if let Some(blocks) = &statblock.spellcasting {
        lines.push(Line::from("Spells"));
        let mut index = 0;
        for block in blocks {
            for spell in &block.spells {
                index += 1;
                lines.push(Line::from(format!(
                    "  {index}. {}{}",
                    spell.name,
                    if spell.prepared { "" } else { " (unprepared)" }
                )));
            }
        }
    }
    if let Some(statuses) = &statblock.statuses {
        lines.push(Line::from("Statuses"));
        for (index, status) in statuses.iter().enumerate() {
            lines.push(Line::from(format!(
                "  {}. {} ({})",
                index + 1,
                status.name,
                duration_label(&status.duration)
            )));
        }
    }
    if let Some(notes) = &statblock.notes {
        lines.push(Line::from(format!("notes: {notes}")));
    }
}

fn draw_feed(frame: &mut Frame<'_>, area: Rect, state: &TuiState) {
    let start = state.feed.len().saturating_sub(FEED_LINES);
    let lines = state.feed[start..]
        .iter()
        .map(|entry| {
            Line::from(format!(
                "[{}] {}",
                entry.clock_s.time_of_day(),
                feed_text(&entry.payload)
            ))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(" feed "))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_footer(frame: &mut Frame<'_>, area: Rect, state: &TuiState) {
    let prompt = match &state.mode {
        Mode::Prompt(prompt) => format!("{} > {}", prompt.kind.label(), prompt.buffer),
        Mode::Normal => String::new(),
        Mode::Create(_) => "new character: Tab/Up/Down field  Enter create  Esc cancel".to_owned(),
        Mode::Help => "help: any key closes".to_owned(),
    };
    let lines = vec![
        Line::from(state.status.clone()),
        Line::from(prompt),
        Line::from(
            "n new  j/k select  dmg d  heal h  temp t  saves s  roll r  status a/x/e  res p/o  rest R/L  cast c  conc C/E  hitdie H  clock A/S  save w  help ?  quit q",
        ),
    ];
    frame.render_widget(Paragraph::new(lines).block(Block::bordered()), area);
}

fn draw_create(frame: &mut Frame<'_>, area: Rect, form: &CreateForm, status: &str) {
    let popup = centered(area, 72, 64);
    frame.render_widget(Clear, popup);
    let mut lines = vec![Line::from("New player character"), Line::from("")];
    for index in 0..form.fields.len() {
        let focused = index == form.focus;
        let marker = if focused { ">" } else { " " };
        let style = if focused {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        lines.push(Line::from(vec![
            Span::raw(format!("{marker} {:<9}", CreateForm::label(index))),
            Span::styled(form.fields[index].clone(), style),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(
        "abilities: STR,DEX,CON,INT,WIS,CHA (1-30), or standard",
    ));
    lines.push(Line::from("Tab/Up/Down field  Enter create  Esc cancel"));
    lines.push(Line::from(status));
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(" new character "))
            .wrap(Wrap { trim: true }),
        popup,
    );
}

fn draw_help(frame: &mut Frame<'_>, area: Rect) {
    let popup = centered(area, 78, 80);
    frame.render_widget(Clear, popup);
    let lines = vec![
        Line::from("Keys"),
        Line::from(""),
        Line::from("n             new character"),
        Line::from("j / Down      next entity"),
        Line::from("k / Up        previous entity"),
        Line::from("g / G         first / last entity"),
        Line::from("d / h / t     damage / heal / temp HP"),
        Line::from("s             set death saves"),
        Line::from("r             roll 1d20+3 or check <stat>"),
        Line::from("a / x / e     add / remove / edit status"),
        Line::from("p / o         spend / restore resource"),
        Line::from("R / L         short / long rest"),
        Line::from("c             cast spell"),
        Line::from("C / E         begin / end concentration"),
        Line::from("H             spend hit die"),
        Line::from("A / S         advance / set clock"),
        Line::from("w             save"),
        Line::from("q / Ctrl-C    quit"),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(" help "))
            .wrap(Wrap { trim: true }),
        popup,
    );
}

fn centered(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1])[1]
}

fn kind_label(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Pc => "pc",
        EntityKind::Companion => "companion",
        EntityKind::Summon => "summon",
        EntityKind::Familiar => "familiar",
        EntityKind::Npc => "npc",
        EntityKind::Monster => "monster",
        EntityKind::Note => "note",
        EntityKind::Handout => "handout",
    }
}

fn resource_kind(kind: ResourceKind) -> &'static str {
    match kind {
        ResourceKind::SpellSlot { .. } => "slot",
        ResourceKind::PactSlot => "pact",
        ResourceKind::HitDie { .. } => "hit die",
        ResourceKind::Feature => "feature",
        ResourceKind::Item => "item",
        ResourceKind::Other => "other",
    }
}

fn duration_label(duration: &StatusDuration) -> String {
    match duration {
        StatusDuration::Timed { expires_at } => format!("until {expires_at}s"),
        StatusDuration::UntilRest { rest } => match rest {
            RestKind::Short => "until a short rest".to_owned(),
            RestKind::Long => "until a long rest".to_owned(),
        },
        StatusDuration::Indefinite => "indefinite".to_owned(),
        StatusDuration::Manual { text } => text.clone(),
    }
}

fn feed_text(payload: &FeedPayload) -> String {
    match payload {
        FeedPayload::Roll { roll } => format!("roll total {}", roll.result.total),
        FeedPayload::StatusExpired { name, .. } => format!("status expired: {name}"),
        FeedPayload::ConcentrationEnded { name } => format!("concentration ended: {name}"),
        FeedPayload::Hint { text } => text.clone(),
    }
}

fn modifier(score: i32) -> i32 {
    (score - 10).div_euclid(2)
}

#[cfg(test)]
mod tests {
    use localtt_core::builders::CampaignBuilder;
    use ratatui::{Terminal, backend::TestBackend};

    use super::draw;
    use crate::{
        app::FeedPage,
        tui::state::{CreateForm, Mode, TuiState},
    };

    fn state() -> TuiState {
        let mut builder = CampaignBuilder::new("Render Test");
        let dm = builder.dm("DM");
        let campaign = localtt_engine::authz::view_of_campaign(&builder.build(), dm)
            .expect("the DM is a member");
        TuiState::new(
            dm,
            campaign,
            Vec::new(),
            FeedPage {
                entries: Vec::new(),
                trimmed_before: None,
            },
        )
    }

    fn render_text(state: &TuiState) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("the test backend starts");
        terminal
            .draw(|frame| draw(frame, state))
            .expect("the frame draws");
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn the_header_and_empty_detail_render() {
        let text = render_text(&state());
        assert!(text.contains("localtt"));
        assert!(text.contains("Render Test"));
        assert!(text.contains("no entity selected"));
    }

    #[test]
    fn the_create_form_renders_over_the_screen() {
        let mut state = state();
        state.mode = Mode::Create(CreateForm::default());
        let text = render_text(&state);
        assert!(text.contains("new character"));
        assert!(text.contains("abilities"));
        assert!(text.contains("Enter create"));
    }
}
