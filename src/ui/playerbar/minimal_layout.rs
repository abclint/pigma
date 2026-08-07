use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Padding;

use crate::config::PlayerbarConfig;
use crate::state::PlaybackState;

use super::super::{BlockStyle, create_block};
use super::build_layout;
use super::widgets;

pub fn draw(
    f: &mut Frame,
    player: &PlaybackState,
    _tick: u64,
    bs: &BlockStyle<'_>,
    config: &PlayerbarConfig,
    area: Rect,
) {
    let colors = bs.colors;
    let block = create_block("", bs, false).block_padding(Padding::horizontal(1));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if let Some(err) = &player.error {
        use ratatui::widgets::Paragraph;
        let text = format!(" \u{26a0}  {}", err);
        f.render_widget(
            Paragraph::new(text).style(Style::default().fg(colors.error)),
            inner,
        );
        return;
    }

    let layout = build_layout::build_minimal(inner);

    draw_song_info_inline(f, player, colors, layout.song_info);
    widgets::draw_controls(f, player, colors, layout.controls, true);
    widgets::draw_gauge_bar(f, player, colors, config, layout.gauge);
    widgets::draw_current_time(f, player, colors, layout.progress_time_left);
    widgets::draw_total_time(f, player, colors, layout.progress_time_right);

    if config.visible.mode_icon {
        widgets::draw_mode_icon(f, player, colors, layout.mode_icon);
    }
}

fn draw_song_info_inline(
    f: &mut Frame,
    player: &PlaybackState,
    colors: &crate::config::Theme,
    area: Rect,
) {
    use ratatui::text::{Line, Span};
    use ratatui::widgets::Paragraph;

    if let Some(song) = &player.current_song {
        let info = Line::from(vec![
            Span::styled(" \u{266a} ", Style::default().fg(colors.accent)),
            Span::styled(
                &song.name,
                Style::default()
                    .fg(colors.text)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(" - ", Style::default().fg(colors.muted)),
            Span::styled(&song.singer, Style::default().fg(colors.muted)),
        ]);
        f.render_widget(Paragraph::new(info), area);
    } else {
        let idle = Line::from(Span::styled("未在播放", Style::default().fg(colors.muted)));
        f.render_widget(Paragraph::new(idle), area);
    }
}
