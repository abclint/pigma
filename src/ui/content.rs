use std::borrow::Cow;

use ncm_api::{SingerInfo, SongInfo, SongList, TopList};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, TableState},
};

use super::BlockStyle;
use super::table;
use crate::config::ColumnDef;
use crate::config::ColumnsConfig;
use crate::state::{ContentState, TableMode};

/// Look up a field value for a table row by its column field name.
/// Returns `None` for unknown fields (rendered as "—").
fn song_field<'a>(song: &'a SongInfo, field: &str) -> Option<Cow<'a, str>> {
    match field {
        "name" => Some(Cow::Borrowed(&song.name)),
        "singer" => Some(Cow::Borrowed(&song.singer)),
        "album" => Some(Cow::Borrowed(&song.album)),
        "duration" => Some(Cow::Owned(crate::utils::format_duration(song.duration))),
        "id" => Some(Cow::Owned(song.id.to_string())),
        _ => None,
    }
}

fn songlist_field<'a>(list: &'a SongList, field: &str) -> Option<Cow<'a, str>> {
    match field {
        "name" => Some(Cow::Borrowed(&list.name)),
        "author" => Some(Cow::Borrowed(&list.author)),
        "id" => Some(Cow::Owned(list.id.to_string())),
        _ => None,
    }
}

fn toplist_field<'a>(list: &'a TopList, field: &str) -> Option<Cow<'a, str>> {
    match field {
        "name" => Some(Cow::Borrowed(&list.name)),
        "description" => Some(Cow::Borrowed(&list.description)),
        "id" => Some(Cow::Owned(list.id.to_string())),
        _ => None,
    }
}

fn singer_field<'a>(singer: &'a SingerInfo, field: &str) -> Option<Cow<'a, str>> {
    match field {
        "name" => Some(Cow::Borrowed(&singer.name)),
        "id" => Some(Cow::Owned(singer.id.to_string())),
        _ => None,
    }
}

/// Build table rows directly from a slice of items, avoiding the intermediate
/// `HashMap` allocation that the old `compute_rows` path performed per row.
fn build_rows<'a, I>(
    items: &'a [I],
    columns: &[ColumnDef],
    lookup: impl Fn(&'a I, &str) -> Option<Cow<'a, str>>,
) -> Vec<Vec<String>> {
    let mut warned = std::collections::HashSet::new();
    items
        .iter()
        .map(|item| {
            columns
                .iter()
                .map(|col| {
                    lookup(item, &col.field)
                        .map(Cow::into_owned)
                        .unwrap_or_else(|| {
                            if !warned.contains(&col.field) {
                                log::warn!("Missing field: \"{}\" — showing \"—\"", col.field);
                                warned.insert(col.field.clone());
                            }
                            "—".to_string()
                        })
                })
                .collect()
        })
        .collect()
}

fn compute_rows(content: &ContentState, columns: &[ColumnDef]) -> Vec<Vec<String>> {
    match content {
        ContentState::Songs(songs) => build_rows(songs, columns, song_field),
        ContentState::SongLists(lists) => build_rows(lists, columns, songlist_field),
        ContentState::TopLists(lists) => build_rows(lists, columns, toplist_field),
        ContentState::HotSearch(keywords) => build_rows(&keywords.0, columns, |kw, field| {
            if field == "keyword" {
                Some(Cow::Borrowed(kw.as_str()))
            } else {
                None
            }
        }),
        ContentState::Singers(singers) => build_rows(singers, columns, singer_field),
        _ => vec![],
    }
}

#[allow(clippy::too_many_arguments)]
pub fn render_content(
    f: &mut Frame,
    content: &ContentState,
    columns: &ColumnsConfig,
    api: Option<&str>,
    cache: &std::cell::RefCell<Option<Vec<Vec<String>>>>,
    bs: &BlockStyle<'_>,
    table_state: &mut TableState,
    table_mode: TableMode,
    area: Rect,
) {
    let colors = bs.colors;
    match content {
        ContentState::Empty => {
            let text = Line::from(Span::styled("", Style::default().fg(colors.muted)));
            f.render_widget(Paragraph::new(text), area);
        }
        ContentState::Loading => {
            let text = Line::from(Span::styled("加载中...", Style::default().fg(colors.muted)));
            f.render_widget(Paragraph::new(text), area);
        }
        ContentState::Error(e) => {
            let text = Line::from(Span::styled(
                format!("错误: {e}"),
                Style::default().fg(colors.error),
            ));
            f.render_widget(Paragraph::new(text), area);
        }
        _ => {
            let cols = columns.for_content(content.content_type(), api);
            if cache.borrow().is_none() {
                let rows = compute_rows(content, cols);
                *cache.borrow_mut() = Some(rows);
            }
            let rows = cache.borrow();
            table::render_table(
                f,
                cols,
                rows.as_deref().unwrap_or(&[]),
                table_state,
                table_mode,
                colors,
                area,
            );
        }
    }
}
