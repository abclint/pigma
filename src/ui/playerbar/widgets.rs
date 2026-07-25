use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{LineGauge, Paragraph},
};
use ratatui_image::{Resize, StatefulImage};

use crate::config::PlayerbarConfig;
use crate::config::Theme;
use crate::playback::types::PlayMode;
use crate::state::PlaybackState;
use crate::ui::gradient_line_gauge::GradientLineGauge;
use crate::ui::spinner::Spinner;
use crate::utils::format_duration;

pub fn mode_icon(mode: &PlayMode) -> (&str, &str) {
    match mode {
        PlayMode::Sequential => ("\u{F049E}", "顺序"),
        PlayMode::RepeatOne => ("\u{F0458}", "单曲"),
        PlayMode::RepeatAll => ("\u{F0577}", "列表"),
        PlayMode::Shuffle => ("\u{F049F}", "随机"),
        PlayMode::Heartbeat { .. } => ("\u{F0430}", "心动"),
    }
}

pub fn draw_song_info(f: &mut Frame, player: &PlaybackState, colors: &Theme, area: Rect) {
    if let Some(song) = &player.current_song {
        let info_lines = vec![
            Line::from(vec![
                Span::styled(" \u{266a} ", Style::default().fg(colors.accent)),
                Span::styled(
                    &song.name,
                    Style::default()
                        .fg(colors.text)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(format!("   {} ◈  {}", song.singer, song.album))
                .style(Style::default().fg(colors.muted)),
        ];
        f.render_widget(Paragraph::new(info_lines), area);
    } else {
        let idle = Line::from(Span::styled("未在播放", Style::default().fg(colors.muted)));
        f.render_widget(Paragraph::new(idle), area);
    }
}

pub fn draw_controls(f: &mut Frame, player: &PlaybackState, colors: &Theme, area: Rect) {
    let play_icon = if player.paused || !player.playing {
        "\u{25b6}"
    } else {
        "\u{23f8}"
    };
    let controls = Line::from(vec![
        Span::raw("       "),
        Span::styled("\u{23ee}", Style::default().fg(colors.muted)),
        Span::raw("   "),
        Span::styled(
            play_icon,
            Style::default()
                .fg(colors.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("   "),
        Span::styled("\u{23ed}", Style::default().fg(colors.muted)),
        Span::raw("       "),
    ])
    .alignment(Alignment::Center);
    f.render_widget(Paragraph::new(controls), area);
}

pub fn draw_mode_icon(f: &mut Frame, player: &PlaybackState, colors: &Theme, area: Rect) {
    let (icon, _) = mode_icon(&player.mode);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            icon,
            Style::default().fg(colors.accent),
        )))
        .alignment(Alignment::Right),
        area,
    );
}

pub fn draw_spinner(f: &mut Frame, tick: u64, colors: &Theme, area: Rect) {
    f.render_widget(
        Spinner::new(tick)
            .active_color(Style::default().fg(colors.accent))
            .inactive_color(Style::default().fg(colors.surface)),
        area,
    );
}

pub fn draw_current_time(f: &mut Frame, player: &PlaybackState, colors: &Theme, area: Rect) {
    if let Some(song) = &player.current_song {
        let cur_ms = (player.progress * song.duration as f64) as u64;
        f.render_widget(
            Paragraph::new(format_duration(cur_ms)).style(Style::default().fg(colors.text)),
            area,
        );
    }
}

pub fn draw_total_time(f: &mut Frame, player: &PlaybackState, colors: &Theme, area: Rect) {
    if let Some(song) = &player.current_song {
        f.render_widget(
            Paragraph::new(format_duration(song.duration)).style(Style::default().fg(colors.text)),
            area,
        );
    }
}

pub fn draw_gauge_bar(
    f: &mut Frame,
    player: &PlaybackState,
    colors: &Theme,
    pb: &PlayerbarConfig,
    area: Rect,
) {
    if player.current_song.is_none() {
        return;
    }

    let unfilled_color = if player.cached {
        pb.unfilled_color_cached.as_str()
    } else {
        pb.unfilled_color.as_str()
    };

    if pb.gradient_enabled {
        let gauge = GradientLineGauge::new(&pb.gradient_preset)
            .ratio(player.progress.clamp(0.0, 1.0))
            .label(Line::from(""))
            .filled_symbol(&pb.filled_symbol)
            .unfilled_symbol(&pb.unfilled_symbol)
            .unfilled_style(Style::default().fg(colors.field_color(unfilled_color)));
        f.render_widget(gauge, area);
    } else {
        let gauge = LineGauge::default()
            .filled_symbol(&pb.filled_symbol)
            .unfilled_symbol(&pb.unfilled_symbol)
            .filled_style(Style::default().fg(colors.field_color(&pb.filled_color)))
            .unfilled_style(Style::default().fg(colors.field_color(unfilled_color)))
            .ratio(player.progress.clamp(0.0, 1.0));
        f.render_widget(gauge, area);
    }
}

pub fn draw_gauge_with_label(
    f: &mut Frame,
    player: &PlaybackState,
    colors: &Theme,
    pb: &PlayerbarConfig,
    area: Rect,
) {
    let Some(song) = &player.current_song else {
        return;
    };

    let cur_ms = (player.progress * song.duration as f64) as u64;
    let time_str = format!(
        "{} / {}",
        format_duration(cur_ms),
        format_duration(song.duration)
    );

    let unfilled_color = if player.cached {
        pb.unfilled_color_cached.as_str()
    } else {
        pb.unfilled_color.as_str()
    };

    if pb.gradient_enabled {
        let gauge = GradientLineGauge::new(&pb.gradient_preset)
            .ratio(player.progress.clamp(0.0, 1.0))
            .label(Line::from(Span::styled(
                time_str,
                Style::default().fg(colors.text),
            )))
            .filled_symbol(&pb.filled_symbol)
            .unfilled_symbol(&pb.unfilled_symbol)
            .unfilled_style(Style::default().fg(colors.field_color(unfilled_color)));
        f.render_widget(gauge, area);
    } else {
        let gauge = LineGauge::default()
            .filled_symbol(&pb.filled_symbol)
            .unfilled_symbol(&pb.unfilled_symbol)
            .filled_style(Style::default().fg(colors.field_color(&pb.filled_color)))
            .unfilled_style(Style::default().fg(colors.field_color(unfilled_color)))
            .ratio(player.progress.clamp(0.0, 1.0))
            .label(Span::styled(time_str, Style::default().fg(colors.text)));
        f.render_widget(gauge, area);
    }
}

pub fn draw_song_detail(f: &mut Frame, player: &PlaybackState, colors: &Theme, area: Rect) {
    if let Some(song) = &player.current_song {
        let detail = Line::from(format!("{} ◈ {}", song.singer, song.album))
            .style(Style::default().fg(colors.muted));
        f.render_widget(Paragraph::new(detail), area);
    }
}
// todo: 重写
#[allow(dead_code)]
pub fn draw_volume(f: &mut Frame, player: &PlaybackState, colors: &Theme, area: Rect) {
    let vol = player.volume;
    let vol_percent = (vol * 100.0) as u16;

    let icon = if vol == 0.0 {
        ""
    } else if vol < 0.5 {
        ""
    } else {
        ""
    };

    let line = Line::from(vec![
        Span::styled(icon, Style::default().fg(colors.text)),
        Span::raw(" "),
        Span::styled(
            format!("{}%", vol_percent),
            Style::default().fg(colors.muted),
        ),
    ]);

    f.render_widget(Paragraph::new(line).alignment(Alignment::Right), area);
}

pub fn draw_cover(f: &mut Frame, player: &PlaybackState, colors: &Theme, area: Rect) {
    if player.current_song.is_some() {
        // Try to render real cover image if available
        if let Ok(mut borrow) = player.cover.0.lock()
            && let Some(protocol) = borrow.as_mut() {
                let image = StatefulImage::new().resize(Resize::Fit(None));
                f.render_stateful_widget(image, area, protocol);
                return;
            }

        // Fallback to placeholder (no border)
        for y in 0..area.height {
            for x in 0..area.width {
                if let Some(cell) = f.buffer_mut().cell_mut((area.x + x, area.y + y)) {
                    cell.set_char('░');
                    cell.set_style(Style::default().fg(colors.surface));
                }
            }
        }

        let icon = "\u{266a}";
        let icon_x = area.x + area.width / 2;
        let icon_y = area.y + area.height / 2;
        if let Some(cell) = f.buffer_mut().cell_mut((icon_x, icon_y)) {
            cell.set_char(icon.chars().next().unwrap_or('♪'));
            cell.set_style(Style::default().fg(colors.accent));
        }
    }
}
