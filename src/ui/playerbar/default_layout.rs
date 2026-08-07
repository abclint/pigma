use ratatui::Frame;
use ratatui::layout::Rect;
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
        use ratatui::style::Style;
        use ratatui::widgets::Paragraph;
        let text = format!(" \u{26a0}  {}", err);
        f.render_widget(
            Paragraph::new(text).style(Style::default().fg(colors.error)),
            inner,
        );
        return;
    }

    let layout = build_layout::build_default(inner);

    widgets::draw_song_info(f, player, colors, layout.song_info);
    widgets::draw_controls(f, player, colors, layout.controls, true);
    widgets::draw_gauge_with_label(f, player, colors, config, layout.gauge);

    if config.visible.mode_icon {
        widgets::draw_mode_icon(f, player, colors, layout.mode_icon);
    }

    if config.visible.volume && layout.volume.width > 0 {
        widgets::draw_volume(f, player, colors, layout.volume);
    }

    if player.seeking && config.visible.spinner {
        widgets::draw_spinner(f, _tick, colors, layout.spinner);
    }
}
