//! Graph view: the notes of the vault as nodes and their links as edges,
//! laid out by a force simulation. The local graph shows the notes within
//! two links of the current one.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::*;

use super::Editor;
use super::link_index::{LinkIndex, link_index};
use crate::i18n::I18nManager;
use crate::theme::Theme;

/// Notes shown at most; the best connected are kept.
const MAX_NODES: usize = 400;
/// Links followed from the current note in the local graph.
const LOCAL_DEPTH: usize = 2;
/// Up to this many notes every label is shown.
const ALL_LABELS_UP_TO: usize = 80;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GraphNode {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
    /// Number of notes linked with this one.
    pub(crate) degree: usize,
}

/// Nodes, edges between node indices, and a position in 0..1 per node.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct GraphView {
    pub(crate) nodes: Vec<GraphNode>,
    pub(crate) edges: Vec<(usize, usize)>,
    pub(crate) positions: Vec<(f32, f32)>,
}

/// The notes and links to draw: all notes, or those within
/// [`LOCAL_DEPTH`] links of `center`.
pub(crate) fn graph_view(index: &LinkIndex, center: Option<&PathBuf>, local: bool) -> GraphView {
    let mut files = index.targets().files().cloned().collect::<Vec<_>>();
    files.sort();
    let mut neighbors = HashMap::<PathBuf, HashSet<PathBuf>>::new();
    for file in &files {
        for target in index.outgoing(file) {
            if target != file {
                neighbors
                    .entry(file.clone())
                    .or_default()
                    .insert(target.clone());
                neighbors
                    .entry(target.clone())
                    .or_default()
                    .insert(file.clone());
            }
        }
    }
    let degree = |path: &PathBuf| neighbors.get(path).map_or(0, HashSet::len);

    let mut chosen = match (local, center) {
        (true, Some(center)) => {
            let mut seen = HashSet::from([center.clone()]);
            let mut order = vec![center.clone()];
            let mut queue = VecDeque::from([(center.clone(), 0)]);
            while let Some((path, depth)) = queue.pop_front() {
                if depth == LOCAL_DEPTH {
                    continue;
                }
                let mut next = neighbors
                    .get(&path)
                    .map(|set| set.iter().cloned().collect::<Vec<_>>())
                    .unwrap_or_default();
                next.sort();
                for neighbor in next {
                    if seen.insert(neighbor.clone()) {
                        order.push(neighbor.clone());
                        queue.push_back((neighbor, depth + 1));
                    }
                }
            }
            order
        }
        _ => files,
    };
    if chosen.len() > MAX_NODES {
        // Keep the current note, then the best connected.
        chosen.sort_by(|left, right| {
            let is_center = |path: &PathBuf| center == Some(path);
            is_center(right)
                .cmp(&is_center(left))
                .then(degree(right).cmp(&degree(left)))
                .then(left.cmp(right))
        });
        chosen.truncate(MAX_NODES);
        chosen.sort();
    }

    let position = chosen
        .iter()
        .enumerate()
        .map(|(index, path)| (path.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut edges = Vec::new();
    for (index, path) in chosen.iter().enumerate() {
        if let Some(set) = neighbors.get(path) {
            for neighbor in set {
                if let Some(&other) = position.get(neighbor)
                    && index < other
                {
                    edges.push((index, other));
                }
            }
        }
    }
    edges.sort();
    let nodes = chosen
        .iter()
        .map(|path| GraphNode {
            path: path.clone(),
            name: index.targets().link_name(path),
            degree: degree(path),
        })
        .collect::<Vec<_>>();
    let positions = layout(nodes.len(), &edges);
    GraphView {
        nodes,
        edges,
        positions,
    }
}

/// Fruchterman–Reingold layout, scaled to fill 0..1 on both axes.
pub(crate) fn layout(count: usize, edges: &[(usize, usize)]) -> Vec<(f32, f32)> {
    if count == 0 {
        return Vec::new();
    }
    if count == 1 {
        return vec![(0.5, 0.5)];
    }
    // A sunflower spiral as a deterministic start.
    let mut positions = (0..count)
        .map(|index| {
            let radius = ((index as f32 + 0.5) / count as f32).sqrt() * 0.5;
            let angle = index as f32 * 2.399_963;
            (0.5 + radius * angle.cos(), 0.5 + radius * angle.sin())
        })
        .collect::<Vec<_>>();
    let k = (1.0 / count as f32).sqrt();
    let iterations = if count > 200 { 120 } else { 300 };
    let mut temperature = 0.1_f32;
    let cooling = temperature / iterations as f32;
    let mut displacement = vec![(0.0_f32, 0.0_f32); count];
    for _ in 0..iterations {
        displacement
            .iter_mut()
            .for_each(|delta| *delta = (0.0, 0.0));
        for left in 0..count {
            for right in left + 1..count {
                let dx = positions[left].0 - positions[right].0;
                let dy = positions[left].1 - positions[right].1;
                let distance = (dx * dx + dy * dy).sqrt().max(0.001);
                let force = k * k / distance;
                let (fx, fy) = (dx / distance * force, dy / distance * force);
                displacement[left].0 += fx;
                displacement[left].1 += fy;
                displacement[right].0 -= fx;
                displacement[right].1 -= fy;
            }
        }
        for &(left, right) in edges {
            let dx = positions[left].0 - positions[right].0;
            let dy = positions[left].1 - positions[right].1;
            let distance = (dx * dx + dy * dy).sqrt().max(0.001);
            let force = distance * distance / k;
            let (fx, fy) = (dx / distance * force, dy / distance * force);
            displacement[left].0 -= fx;
            displacement[left].1 -= fy;
            displacement[right].0 += fx;
            displacement[right].1 += fy;
        }
        for (position, delta) in positions.iter_mut().zip(&displacement) {
            // A pull to the middle keeps unlinked notes from drifting off.
            let delta = (
                delta.0 - (position.0 - 0.5) * k * 2.0,
                delta.1 - (position.1 - 0.5) * k * 2.0,
            );
            let length = (delta.0 * delta.0 + delta.1 * delta.1).sqrt().max(0.0001);
            let step = length.min(temperature);
            position.0 += delta.0 / length * step;
            position.1 += delta.1 / length * step;
        }
        temperature = (temperature - cooling).max(0.002);
    }

    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for &(x, y) in &positions {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    let scale = |value: f32, min: f32, max: f32| {
        if max - min < 1e-6 {
            0.5
        } else {
            (value - min) / (max - min)
        }
    };
    positions
        .into_iter()
        .map(|(x, y)| (scale(x, min_x, max_x), scale(y, min_y, max_y)))
        .collect()
}

/// The open graph overlay.
pub(crate) struct GraphState {
    local: bool,
    /// The index and centre the view was computed for.
    source: Option<(Arc<LinkIndex>, Option<PathBuf>, bool)>,
    view: Option<Arc<GraphView>>,
    hovered: Option<usize>,
    _task: Option<Task<()>>,
}

impl Editor {
    pub(super) fn on_show_graph(
        &mut self,
        _: &crate::components::ShowGraph,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.close_graph(cx) {
            return;
        }
        self.graph = Some(GraphState {
            local: self.file_path.is_some(),
            source: None,
            view: None,
            hovered: None,
            _task: None,
        });
        cx.notify();
    }

    pub(super) fn close_graph(&mut self, cx: &mut Context<Self>) -> bool {
        if self.graph.take().is_some() {
            cx.notify();
            true
        } else {
            false
        }
    }

    fn set_graph_local(&mut self, local: bool, cx: &mut Context<Self>) {
        if let Some(graph) = self.graph.as_mut()
            && graph.local != local
        {
            graph.local = local;
            graph.hovered = None;
            cx.notify();
        }
    }

    /// Lays the graph out again in the background when the index, the
    /// current note or the local/global choice changed.
    pub(super) fn sync_graph(&mut self, cx: &mut Context<Self>) {
        if self.graph.is_none() {
            return;
        }
        let Some(root) = self.link_root() else {
            return;
        };
        let Some(index) = link_index(&root, true, cx) else {
            return;
        };
        let center = self
            .file_path
            .as_ref()
            .map(|path| path.canonicalize().unwrap_or_else(|_| path.clone()));
        let Some(graph) = self.graph.as_mut() else {
            return;
        };
        let unchanged = graph
            .source
            .as_ref()
            .is_some_and(|(known, known_center, local)| {
                Arc::ptr_eq(known, &index) && *known_center == center && *local == graph.local
            });
        if unchanged {
            return;
        }
        graph.source = Some((index.clone(), center.clone(), graph.local));
        let local = graph.local;
        let build = cx
            .background_executor()
            .spawn(async move { graph_view(&index, center.as_ref(), local) });
        graph._task = Some(cx.spawn(async move |this, cx| {
            let view = build.await;
            let _ = this.update(cx, |editor, cx| {
                if let Some(graph) = editor.graph.as_mut() {
                    graph.view = Some(Arc::new(view));
                    graph.hovered = None;
                    cx.notify();
                }
            });
        }));
    }

    pub(super) fn render_graph(
        &self,
        theme: &Theme,
        top: Pixels,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let graph = self.graph.as_ref()?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let strings = cx.global::<I18nManager>().strings().clone();
        let current = self
            .file_path
            .as_ref()
            .map(|path| path.canonicalize().unwrap_or_else(|_| path.clone()));

        let toggle = |id: &'static str, label: String, active: bool, local: bool| {
            div()
                .id(id)
                .px(px(10.0))
                .py(px(2.0))
                .rounded(px(d.menu_item_radius))
                .cursor_pointer()
                .text_color(if active {
                    c.text_default
                } else {
                    c.text_placeholder
                })
                .when(active, |button| button.bg(c.dialog_secondary_button_bg))
                .hover(|button| button.bg(c.dialog_secondary_button_hover))
                .on_click(cx.listener(move |this, _, _, cx| this.set_graph_local(local, cx)))
                .child(label)
        };
        let header = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(
                div()
                    .flex_1()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c.dialog_title)
                    .child(strings.graph_title.clone()),
            )
            .when(self.file_path.is_some(), |header| {
                header.child(toggle(
                    "graph-local",
                    strings.graph_local.clone(),
                    graph.local,
                    true,
                ))
            })
            .child(toggle(
                "graph-all",
                strings.graph_all.clone(),
                !graph.local || self.file_path.is_none(),
                false,
            ))
            .child(
                div()
                    .id("graph-close")
                    .px(px(8.0))
                    .cursor_pointer()
                    .text_color(c.text_placeholder)
                    .hover(|button| button.text_color(c.text_default))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_graph(cx);
                    }))
                    .child("✕"),
            );

        let body = match graph.view.as_ref() {
            None => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(c.text_placeholder)
                .child(strings.query_loading.clone())
                .into_any_element(),
            Some(view) if view.nodes.is_empty() => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(c.text_placeholder)
                .child(strings.graph_empty.clone())
                .into_any_element(),
            Some(view) => self.render_graph_canvas(view.clone(), graph.hovered, current, theme, cx),
        };

        Some(
            div()
                .id("graph-overlay")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.close_graph(cx);
                    }),
                )
                .child(
                    div()
                        .id("graph-panel")
                        .absolute()
                        .top(top + px(12.0))
                        .bottom(px(12.0))
                        .left(px(24.0))
                        .right(px(24.0))
                        .p(px(10.0))
                        .flex()
                        .flex_col()
                        .gap(px(6.0))
                        .bg(c.dialog_surface)
                        .border(px(d.dialog_border_width))
                        .border_color(c.dialog_border)
                        .rounded(px(d.menu_panel_radius))
                        .shadow_lg()
                        .text_size(px(t.dialog_body_size))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(header)
                        .child(body),
                )
                .into_any_element(),
        )
    }

    fn render_graph_canvas(
        &self,
        view: Arc<GraphView>,
        hovered: Option<usize>,
        current: Option<PathBuf>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let t = &theme.typography;
        const PADDING: f32 = 36.0;
        let place = |bounds: Bounds<Pixels>, (x, y): (f32, f32)| {
            let width = (bounds.size.width - px(PADDING * 2.0)).max(px(1.0));
            let height = (bounds.size.height - px(PADDING * 2.0)).max(px(1.0));
            point(
                bounds.origin.x + px(PADDING) + width * x,
                bounds.origin.y + px(PADDING) + height * y,
            )
        };
        let edge_color = c.table_border;
        let highlight = c.text_link;
        let edges_view = view.clone();
        let edges = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                for (lit, color, width) in [(false, edge_color, 1.0), (true, highlight, 1.5)] {
                    let mut builder = PathBuilder::stroke(px(width));
                    let mut any = false;
                    for &(left, right) in &edges_view.edges {
                        let touches = hovered.is_some_and(|node| node == left || node == right);
                        if touches != lit {
                            continue;
                        }
                        builder.move_to(place(bounds, edges_view.positions[left]));
                        builder.line_to(place(bounds, edges_view.positions[right]));
                        any = true;
                    }
                    if any && let Ok(path) = builder.build() {
                        window.paint_path(path, color);
                    }
                }
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();

        let linked = hovered
            .map(|node| {
                view.edges
                    .iter()
                    .filter_map(|&(left, right)| {
                        (left == node)
                            .then_some(right)
                            .or((right == node).then_some(left))
                    })
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default();
        let mut by_degree = (0..view.nodes.len()).collect::<Vec<_>>();
        by_degree.sort_by(|left, right| view.nodes[*right].degree.cmp(&view.nodes[*left].degree));
        let labelled = by_degree.into_iter().take(15).collect::<HashSet<_>>();
        let all_labels = view.nodes.len() <= ALL_LABELS_UP_TO;

        let nodes = view.nodes.iter().enumerate().map(|(index, node)| {
            let size = (6.0 + (node.degree as f32).sqrt() * 3.0).min(20.0);
            let (x, y) = view.positions[index];
            let is_current = current.as_ref() == Some(&node.path);
            let lit = hovered == Some(index) || linked.contains(&index);
            let color = if is_current || lit {
                c.text_link
            } else {
                c.text_placeholder
            };
            let label = all_labels || is_current || lit || labelled.contains(&index);
            let path = node.path.clone();
            // The node and its label, centred on the position.
            div()
                .id(("graph-node", index))
                .absolute()
                .left(relative(x))
                .top(relative(y))
                .ml(px(-60.0))
                .mt(px(-size / 2.0))
                .w(px(120.0))
                .flex()
                .flex_col()
                .items_center()
                .cursor_pointer()
                .on_hover(cx.listener(move |this, hover: &bool, _, cx| {
                    if let Some(graph) = this.graph.as_mut() {
                        let next = if *hover {
                            Some(index)
                        } else if graph.hovered == Some(index) {
                            None
                        } else {
                            graph.hovered
                        };
                        if graph.hovered != next {
                            graph.hovered = next;
                            cx.notify();
                        }
                    }
                }))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.close_graph(cx);
                    this.open_path_and_reveal(path.clone(), None, window, cx);
                }))
                .child(
                    div()
                        .size(px(size))
                        .rounded_full()
                        .bg(color)
                        .when(is_current, |dot| {
                            dot.border_2().border_color(c.text_default)
                        }),
                )
                .when(label, |item| {
                    item.child(
                        div()
                            .max_w(px(120.0))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(px(t.dialog_body_size * 0.8))
                            .text_color(if is_current || lit {
                                c.text_default
                            } else {
                                c.text_placeholder
                            })
                            .child(SharedString::from(node.name.clone())),
                    )
                })
        });

        div()
            .flex_1()
            .relative()
            .overflow_hidden()
            .child(edges)
            .child(
                // Inset by the canvas padding so relative positions match.
                div()
                    .absolute()
                    .top(px(PADDING))
                    .bottom(px(PADDING))
                    .left(px(PADDING))
                    .right(px(PADDING))
                    .children(nodes),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{graph_view, layout};
    use crate::editor::link_index::LinkIndex;

    fn index() -> LinkIndex {
        let sources: Vec<(PathBuf, String)> = vec![
            ("/v/A.md".into(), "[[B]] [[C]]".into()),
            ("/v/B.md".into(), "[[A]] [[D]]".into()),
            ("/v/C.md".into(), String::new()),
            ("/v/D.md".into(), "[[E]]".into()),
            ("/v/E.md".into(), String::new()),
            ("/v/Lone.md".into(), String::new()),
        ];
        let files = sources.iter().map(|(path, _)| path.clone()).collect();
        LinkIndex::from_sources(Path::new("/v"), files, sources)
    }

    fn names(view: &super::GraphView) -> Vec<&str> {
        view.nodes.iter().map(|node| node.name.as_str()).collect()
    }

    #[test]
    fn global_graph_has_every_note_and_undirected_edges() {
        let view = graph_view(&index(), None, false);
        assert_eq!(names(&view), ["A", "B", "C", "D", "E", "Lone"]);
        // A–B is linked both ways but drawn once.
        assert_eq!(view.edges, [(0, 1), (0, 2), (1, 3), (3, 4)]);
        let degrees = view
            .nodes
            .iter()
            .map(|node| node.degree)
            .collect::<Vec<_>>();
        assert_eq!(degrees, [2, 2, 1, 2, 1, 0]);
    }

    #[test]
    fn local_graph_reaches_two_links() {
        let view = graph_view(&index(), Some(&PathBuf::from("/v/C.md")), true);
        // C → A (1) → B (2); D is three links away.
        assert_eq!(names(&view), ["C", "A", "B"]);
        assert_eq!(view.edges, [(0, 1), (1, 2)]);
    }

    #[test]
    fn layout_fills_the_unit_square_and_separates_nodes() {
        let positions = layout(6, &[(0, 1), (0, 2), (1, 3), (3, 4)]);
        assert_eq!(positions.len(), 6);
        assert!(
            positions
                .iter()
                .all(|&(x, y)| (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y))
        );
        for (index, a) in positions.iter().enumerate() {
            for b in &positions[index + 1..] {
                let distance = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
                assert!(distance > 0.05, "{a:?} {b:?}");
            }
        }
        // Linked notes end up closer than the unlinked one.
        let distance = |a: usize, b: usize| {
            ((positions[a].0 - positions[b].0).powi(2) + (positions[a].1 - positions[b].1).powi(2))
                .sqrt()
        };
        assert!(distance(0, 1) < distance(0, 5));
        assert_eq!(layout(1, &[]), [(0.5, 0.5)]);
        assert!(layout(0, &[]).is_empty());
    }
}
