//! Renderings derived purely from a `Plan`: Mermaid dependency graphs and a
//! standalone HTML before/after movement diagram.

use crate::Plan;

fn esc_label(s: &str) -> String {
    s.replace('"', "'").replace('\n', " ")
}
fn esc_xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Mermaid dependency graph with nodes declared in original source order.
pub fn mermaid_before(plan: &Plan) -> String {
    let mut out = String::from("graph TD\n");
    for (c, n) in plan.nodes.iter().enumerate() {
        out += &format!("  N{c}[\"{}\"]\n", esc_label(&n.display));
    }
    edges(plan, &mut out);
    out
}

/// Mermaid dependency graph in reordered layout; mutual groups become subgraphs.
pub fn mermaid_after(plan: &Plan) -> String {
    let mut out = String::from("graph TD\n");
    let mut i = 0;
    let mut gnum = 0;
    while i < plan.after.len() {
        let c = plan.after[i];
        let g = plan.group_of[c];
        if g >= 0 {
            gnum += 1;
            out += &format!("  subgraph mutual_{gnum}[\"mutual\"]\n");
            while i < plan.after.len() && plan.group_of[plan.after[i]] == g {
                let m = plan.after[i];
                out += &format!("    N{m}[\"{}\"]\n", esc_label(&plan.nodes[m].display));
                i += 1;
            }
            out += "  end\n";
        } else {
            out += &format!("  N{c}[\"{}\"]\n", esc_label(&plan.nodes[c].display));
            i += 1;
        }
    }
    edges(plan, &mut out);
    out
}

fn edges(plan: &Plan, out: &mut String) {
    for (c, ds) in plan.deps.iter().enumerate() {
        for &d in ds {
            *out += &format!("  N{c} --> N{d}\n");
        }
    }
}

/// Self-contained HTML showing BEFORE and AFTER columns with arrows linking
/// each item's old row to its new row; moved items are highlighted.
pub fn html_diff(plan: &Plan, title: &str) -> String {
    let n = plan.nodes.len();
    let row_h = 30.0_f64;
    let pad_top = 70.0_f64;
    let col_w = 320.0_f64;
    let gap = 200.0_f64;
    let left_x = 24.0_f64;
    let right_x = left_x + col_w + gap;
    let height = pad_top + (n as f64) * row_h + 30.0;
    let width = right_x + col_w + 24.0;

    let y = |row: usize| pad_top + (row as f64) * row_h + row_h / 2.0;

    // pos in after for each compact id
    let mut pos_after = vec![0usize; n];
    for (p, &c) in plan.after.iter().enumerate() {
        pos_after[c] = p;
    }

    let mut svg = String::new();
    svg += &format!(
        "<svg width=\"{width:.0}\" height=\"{height:.0}\" xmlns=\"http://www.w3.org/2000/svg\" font-family=\"ui-monospace,Menlo,Consolas,monospace\" font-size=\"13\">\n"
    );
    svg += &format!("<text x=\"{left_x:.0}\" y=\"26\" font-size=\"15\" font-weight=\"700\">{}</text>\n", esc_xml(title));
    svg += &format!("<text x=\"{left_x:.0}\" y=\"50\" font-weight=\"700\">BEFORE</text>\n");
    svg += &format!("<text x=\"{right_x:.0}\" y=\"50\" font-weight=\"700\">AFTER</text>\n");

    // arrows first (under the boxes)
    for c in 0..n {
        let before_row = c;
        let after_row = pos_after[c];
        let moved = before_row != after_row;
        let (x1, y1) = (left_x + col_w, y(before_row));
        let (x2, y2) = (right_x, y(after_row));
        let mx = (x1 + x2) / 2.0;
        let (stroke, w, op) = if moved {
            ("#d83933", 1.8, 0.95)
        } else {
            ("#9aa0a6", 1.0, 0.5)
        };
        svg += &format!(
            "<path d=\"M{x1:.1},{y1:.1} C{mx:.1},{y1:.1} {mx:.1},{y2:.1} {x2:.1},{y2:.1}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"{w}\" opacity=\"{op}\"/>\n"
        );
    }

    // boxes + labels
    let row_svg = |x: f64, row: usize, c: usize, moved: bool| {
        let yy = pad_top + (row as f64) * row_h + 4.0;
        let fill = if moved { "#fff3cd" } else { "#f4f5f7" };
        let stroke = if moved { "#d08c00" } else { "#d0d3d7" };
        let label = esc_xml(&plan.nodes[c].display);
        format!(
            "<rect x=\"{x:.0}\" y=\"{yy:.1}\" width=\"{col_w:.0}\" height=\"{:.0}\" rx=\"5\" fill=\"{fill}\" stroke=\"{stroke}\"/>\n<text x=\"{:.0}\" y=\"{:.1}\">{label}</text>\n",
            row_h - 6.0,
            x + 10.0,
            yy + 18.0,
        )
    };
    for c in 0..n {
        let moved = pos_after[c] != c;
        svg += &row_svg(left_x, c, c, moved);
    }
    for (row, &c) in plan.after.iter().enumerate() {
        let moved = row != c;
        svg += &row_svg(right_x, row, c, moved);
    }
    svg += "</svg>\n";

    format!(
        "<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>{t} — before/after</title>\n<style>body{{margin:0;background:#fff;color:#1a1a1a}}.legend{{font:13px ui-monospace,monospace;padding:10px 24px}}.legend b{{color:#d83933}}</style></head>\n<body>\n<div class=\"legend\">red arrows = moved &nbsp;·&nbsp; gray = unchanged position &nbsp;·&nbsp; {moved}/{total} items moved</div>\n{svg}\n</body></html>\n",
        t = esc_xml(title),
        moved = plan.moved,
        total = n,
    )
}
