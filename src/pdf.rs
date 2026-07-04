use std::collections::HashMap;
use std::io::BufWriter;

use printpdf::*;

use crate::error::{AppError, AppResult};
use crate::models::{Lineup, LineupSpot, Player, Team};

// US Letter in millimeters.
const PAGE_W: f64 = 215.9;
const PAGE_H: f64 = 279.4;
const MARGIN: f64 = 6.35; // 0.25 in
const GAP: f64 = 5.0;

const IN: f64 = 25.4;

fn mm(v: f64) -> Mm {
    Mm(v as f32)
}

// --- palette (mirrors the on-screen preview) ---
fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(Rgb::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, None))
}
fn ink() -> Color {
    rgb(20, 33, 58)
}
fn navy() -> Color {
    rgb(11, 37, 69)
}
fn red() -> Color {
    rgb(179, 32, 46)
}
fn muted() -> Color {
    rgb(90, 102, 117)
}
fn green() -> Color {
    rgb(31, 122, 77)
}
fn grid() -> Color {
    rgb(201, 207, 216)
}
fn green_tint() -> Color {
    rgb(236, 246, 240)
}

#[derive(Clone, Copy, PartialEq)]
enum Variant {
    Large,
    Umpire,
}

impl Variant {
    fn size_mm(self) -> (f64, f64) {
        match self {
            Variant::Large => (3.75 * IN, 8.0 * IN),
            Variant::Umpire => (3.5 * IN, 7.0 * IN),
        }
    }

    fn base_font(self) -> f64 {
        match self {
            Variant::Large => 10.0,
            Variant::Umpire => 7.5,
        }
    }
}

pub struct PdfRequest {
    pub coach: u32,
    pub scorekeeper: u32,
    pub self_copy: u32,
    pub umpire: u32,
}

struct Card {
    variant: Variant,
    recipient: String,
}

struct Placement {
    card_index: usize,
    page: usize,
    x: f64,
    y: f64,
}

struct Fonts {
    regular: IndirectFontRef,
    bold: IndirectFontRef,
}

pub fn render_sheet(
    team: &Team,
    lineup: &Lineup,
    spots: &[LineupSpot],
    players: &[Player],
    req: &PdfRequest,
    uploads_dir: &str,
) -> AppResult<Vec<u8>> {
    let mut cards: Vec<Card> = Vec::new();
    let mut push = |variant: Variant, label: &str, n: u32| {
        for _ in 0..n {
            cards.push(Card {
                variant,
                recipient: label.to_string(),
            });
        }
    };
    push(Variant::Large, "HOME COACH", req.coach);
    push(Variant::Large, "SCOREKEEPER", req.scorekeeper);
    push(Variant::Large, "MANAGER COPY", req.self_copy);
    push(Variant::Umpire, "UMPIRE", req.umpire);

    let placements = pack(&cards);
    let page_count = placements.iter().map(|p| p.page + 1).max().unwrap_or(1);

    let (doc, first_page, first_layer) =
        PdfDocument::new("Lineup Cards", mm(PAGE_W), mm(PAGE_H), "Layer 1");
    let mut layers = vec![doc.get_page(first_page).get_layer(first_layer)];
    for i in 1..page_count {
        let (p, l) = doc.add_page(mm(PAGE_W), mm(PAGE_H), format!("Layer {}", i + 1));
        layers.push(doc.get_page(p).get_layer(l));
    }

    let fonts = Fonts {
        regular: doc.add_builtin_font(BuiltinFont::Helvetica).map_err(|e| {
            AppError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?,
        bold: doc.add_builtin_font(BuiltinFont::HelveticaBold).map_err(|e| {
            AppError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?,
    };

    let player_map: HashMap<i64, &Player> = players.iter().map(|p| (p.id, p)).collect();

    for pl in &placements {
        let card = &cards[pl.card_index];
        let (w, h) = card.variant.size_mm();
        let layer = &layers[pl.page];
        draw_card(
            layer,
            &fonts,
            pl.x,
            pl.y,
            w,
            h,
            card.variant,
            &card.recipient,
            team,
            lineup,
            spots,
            players,
            &player_map,
            uploads_dir,
        );
    }

    let mut buf: Vec<u8> = Vec::new();
    {
        let mut writer = BufWriter::new(&mut buf);
        doc.save(&mut writer).map_err(|e| {
            AppError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;
    }
    Ok(buf)
}

fn pack(cards: &[Card]) -> Vec<Placement> {
    let mut out = Vec::new();
    let usable_right = PAGE_W - MARGIN;
    let mut page = 0usize;
    let mut cursor_x = MARGIN;
    let mut cursor_top = MARGIN;
    let mut shelf_h = 0.0f64;

    for (i, card) in cards.iter().enumerate() {
        let (w, h) = card.variant.size_mm();
        if cursor_x + w > usable_right + 0.01 {
            cursor_x = MARGIN;
            cursor_top += shelf_h + GAP;
            shelf_h = 0.0;
        }
        if cursor_top + h > PAGE_H - MARGIN + 0.01 {
            page += 1;
            cursor_x = MARGIN;
            cursor_top = MARGIN;
            shelf_h = 0.0;
        }
        let y_bottom = PAGE_H - cursor_top - h;
        out.push(Placement {
            card_index: i,
            page,
            x: cursor_x,
            y: y_bottom,
        });
        cursor_x += w + GAP;
        shelf_h = shelf_h.max(h);
    }
    out
}

// --- primitive drawing helpers (all take an explicit color) ---

fn stroke(layer: &PdfLayerReference, thickness: f64, color: Color) {
    layer.set_outline_thickness(thickness as f32);
    layer.set_outline_color(color);
}

fn rect(layer: &PdfLayerReference, x: f64, y: f64, w: f64, h: f64, thickness: f64, color: Color) {
    stroke(layer, thickness, color);
    let points = vec![
        (Point::new(mm(x), mm(y)), false),
        (Point::new(mm(x + w), mm(y)), false),
        (Point::new(mm(x + w), mm(y + h)), false),
        (Point::new(mm(x), mm(y + h)), false),
    ];
    layer.add_line(Line {
        points,
        is_closed: true,
    });
}

fn fill_rect(layer: &PdfLayerReference, x: f64, y: f64, w: f64, h: f64, color: Color) {
    layer.set_fill_color(color);
    let ring = vec![
        (Point::new(mm(x), mm(y)), false),
        (Point::new(mm(x + w), mm(y)), false),
        (Point::new(mm(x + w), mm(y + h)), false),
        (Point::new(mm(x), mm(y + h)), false),
    ];
    layer.add_polygon(Polygon {
        rings: vec![ring],
        mode: PolygonMode::Fill,
        ..Default::default()
    });
}

fn hline(layer: &PdfLayerReference, x1: f64, x2: f64, y: f64, thickness: f64, color: Color) {
    stroke(layer, thickness, color);
    let points = vec![
        (Point::new(mm(x1), mm(y)), false),
        (Point::new(mm(x2), mm(y)), false),
    ];
    layer.add_line(Line {
        points,
        is_closed: false,
    });
}

fn vline(layer: &PdfLayerReference, x: f64, y1: f64, y2: f64, thickness: f64, color: Color) {
    stroke(layer, thickness, color);
    let points = vec![
        (Point::new(mm(x), mm(y1)), false),
        (Point::new(mm(x), mm(y2)), false),
    ];
    layer.add_line(Line {
        points,
        is_closed: false,
    });
}

fn text(
    layer: &PdfLayerReference,
    font: &IndirectFontRef,
    s: &str,
    size: f64,
    x: f64,
    baseline_y: f64,
    color: Color,
) {
    layer.set_fill_color(color);
    layer.use_text(s, size as f32, mm(x), mm(baseline_y), font);
}

fn approx_width(s: &str, size_pt: f64) -> f64 {
    s.chars().count() as f64 * size_pt * 0.352778 * 0.52
}

fn truncate_to(s: &str, size_pt: f64, max_mm: f64) -> String {
    if approx_width(s, size_pt) <= max_mm {
        return s.to_string();
    }
    let mut out = String::new();
    for ch in s.chars() {
        if approx_width(&format!("{out}{ch}\u{2026}"), size_pt) > max_mm {
            break;
        }
        out.push(ch);
    }
    if out.is_empty() {
        out.push(s.chars().next().unwrap_or(' '));
    }
    format!("{out}\u{2026}")
}

fn center_x(label: &str, size_pt: f64, col_x: f64, col_w: f64) -> f64 {
    let tw = approx_width(label, size_pt);
    (col_x + (col_w - tw) / 2.0).max(col_x + 0.4)
}

#[allow(clippy::too_many_arguments)]
fn draw_card(
    layer: &PdfLayerReference,
    fonts: &Fonts,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    variant: Variant,
    recipient: &str,
    team: &Team,
    lineup: &Lineup,
    spots: &[LineupSpot],
    players: &[Player],
    player_map: &HashMap<i64, &Player>,
    uploads_dir: &str,
) {
    let _ = lineup.dh_mode.as_str();
    let pad = 2.2;
    let base = variant.base_font();
    let small = base - 2.0;
    let top = y + h;
    let il = x + pad;
    let ir = x + w - pad;
    let iw = ir - il;

    // Column geometry — shared so the batting grid and the available grid line up.
    let w_order = iw * 0.085;
    let w_num = iw * 0.075;
    let w_pos = iw * 0.095;
    let w_subpos = iw * 0.085;
    let w_inn = iw * 0.065;
    let w_names = iw - (w_order + w_num + w_pos + w_subpos + w_inn);
    let w_start = w_names * 0.53;
    let w_sub = w_names - w_start;
    let cx_order = il;
    let cx_num = cx_order + w_order;
    let cx_start = cx_num + w_num;
    let cx_pos = cx_start + w_start;
    let cx_sub = cx_pos + w_pos;
    let cx_subpos = cx_sub + w_sub;
    let cx_inn = cx_subpos + w_subpos;

    rect(layer, x, y, w, h, 0.8, ink());

    // ---- Header ----
    let header_h = if variant == Variant::Large { 22.0 } else { 18.0 };
    let header_bottom = top - header_h;

    let logo_h = header_h - 2.0 * pad;
    let logo_x = x + pad;
    let logo_y = header_bottom + pad;
    let mut logo_w_used = 0.0;
    let mut logo_drawn = false;
    if let Some(url) = &team.logo_path {
        let file = url.rsplit('/').next().unwrap_or(url);
        let fs_path = format!("{uploads_dir}/{file}");
        if let Ok(rw) = place_logo(layer, &fs_path, logo_x, logo_y, logo_h * 1.7, logo_h) {
            logo_w_used = rw;
            logo_drawn = true;
        }
    }
    if !logo_drawn {
        let initial = team
            .name
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default();
        let s = base + 6.0;
        text(layer, &fonts.bold, &initial, s, logo_x, logo_y + (logo_h - s * 0.35) / 2.0, navy());
        logo_w_used = approx_width(&initial, s) + 1.0;
    }

    // Recipient tag (bordered, navy) — reserve room so the team name doesn't run into it.
    let tag_size = base - 2.0;
    let tag_w = approx_width(recipient, tag_size) * 1.35;
    let tag_bx = x + w - pad - tag_w - 3.0;
    let tag_baseline = top - pad - (base + 2.0) * 0.35;
    rect(layer, tag_bx, tag_baseline - 1.4, tag_w + 3.0, tag_size * 0.35 + 3.2, 0.5, navy());
    text(layer, &fonts.bold, recipient, tag_size, tag_bx + 1.6, tag_baseline, navy());

    let info_x = logo_x + logo_w_used + pad + 1.0;
    let info_w = tag_bx - pad - info_x;
    let name = truncate_to(&team.name, base + 2.0, info_w);
    text(layer, &fonts.bold, &name, base + 2.0, info_x, tag_baseline, navy());

    let mut line_y = top - pad - (base + 2.0) * 0.35 - (base * 0.5) - 1.6;
    if !lineup.opponent.is_empty() {
        let vs = truncate_to(&format!("vs {}", lineup.opponent), base, info_w + tag_w);
        text(layer, &fonts.regular, &vs, base, info_x, line_y, ink());
        line_y -= base * 0.5 + 1.4;
    }
    let mut meta_bits: Vec<String> = Vec::new();
    if !lineup.game_date.is_empty() {
        meta_bits.push(lineup.game_date.clone());
    }
    if !lineup.home_away.is_empty() {
        meta_bits.push(lineup.home_away.to_uppercase());
    }
    if !lineup.location.is_empty() {
        meta_bits.push(lineup.location.clone());
    }
    if !meta_bits.is_empty() {
        let meta = truncate_to(&meta_bits.join("  •  "), base - 1.5, info_w + tag_w);
        text(layer, &fonts.regular, &meta, base - 1.5, info_x, line_y, muted());
    }

    hline(layer, x, x + w, header_bottom, 0.7, ink());

    // ---- Footer: coaches ----
    let footer_h = if variant == Variant::Large { 13.0 } else { 11.0 };
    let footer_top = y + footer_h;
    hline(layer, x, x + w, footer_top, 0.6, ink());
    let mut fy = footer_top - 3.0 - small * 0.35;
    if !team.head_coach.is_empty() {
        let s = truncate_to(&format!("Head Coach: {}", team.head_coach), small, w - 2.0 * pad);
        text(layer, &fonts.regular, &s, small, x + pad, fy, ink());
        fy -= small * 0.5 + 1.8;
    }
    if !team.assistant_coaches.is_empty() {
        let s = truncate_to(&format!("Assistants: {}", team.assistant_coaches), small, w - 2.0 * pad);
        text(layer, &fonts.regular, &s, small, x + pad, fy, ink());
    }

    // ---- Available players (bench) ----
    let assigned: Vec<i64> = spots.iter().filter_map(|s| s.player_id).collect();
    let subs: Vec<&Player> = players.iter().filter(|p| !assigned.contains(&p.id)).collect();

    let av_size = small - 0.5;
    let av_line = av_size * 0.5 + 3.0;
    let av_head = av_size + 3.0;
    let left_count = (subs.len() + 1) / 2;
    let av_rows = left_count.max(3);
    let avail_h = av_head + av_rows as f64 * av_line + 1.5;
    let avail_top = footer_top + avail_h;
    hline(layer, x, x + w, avail_top, 0.6, ink());

    let num_w = cx_num - il;
    let center = cx_sub;
    let av_rows_top = avail_top - av_head;
    let av_hb = av_rows_top + (av_head - av_size * 0.35) / 2.0;
    for &(numx, namex, nameend) in &[(il, cx_num, center), (center, center + num_w, ir)] {
        text(layer, &fonts.bold, "#", av_size, numx + 0.8, av_hb, muted());
        let title = truncate_to("PLAYER AVAILABLE", av_size - 0.5, nameend - namex - 1.5);
        text(layer, &fonts.bold, &title, av_size - 0.5, namex + 1.2, av_hb, muted());
    }
    vline(layer, cx_num, footer_top, avail_top, 0.25, grid());
    vline(layer, center, footer_top, avail_top, 0.25, grid());
    vline(layer, center + num_w, footer_top, avail_top, 0.25, grid());
    hline(layer, x, x + w, av_rows_top, 0.4, ink());

    for r in 0..av_rows {
        let row_top = av_rows_top - r as f64 * av_line;
        if r > 0 {
            hline(layer, x, x + w, row_top, 0.25, grid());
        }
        let baseline = row_top - av_line + (av_line - av_size * 0.35) / 2.0;
        if r < left_count {
            if let Some(p) = subs.get(r) {
                text(layer, &fonts.bold, &p.number, av_size, il + 1.0, baseline, red());
                let nm = truncate_to(&p.name, av_size, center - cx_num - 2.0);
                text(layer, &fonts.regular, &nm, av_size, cx_num + 1.2, baseline, ink());
            }
        }
        if let Some(p) = subs.get(left_count + r) {
            text(layer, &fonts.bold, &p.number, av_size, center + 1.0, baseline, red());
            let nm = truncate_to(&p.name, av_size, ir - (center + num_w) - 2.0);
            text(layer, &fonts.regular, &nm, av_size, center + num_w + 1.2, baseline, ink());
        }
    }

    // ---- Main lineup table ----
    let table_top = header_bottom;
    let table_bottom = avail_top;
    let col_hdr_h = small + 2.5;
    let data_top = table_top - col_hdr_h;
    let n_data = spots.len() + 1;
    let row_h = (data_top - table_bottom) / n_data as f64;

    // Tint DEF rows first (under the grid + text).
    for (i, spot) in spots.iter().enumerate() {
        if spot.slot_kind == "DEF" {
            let row_top = data_top - i as f64 * row_h;
            fill_rect(layer, x + 0.4, row_top - row_h, w - 0.8, row_h, green_tint());
        }
    }

    // Vertical separators.
    for &vx in &[cx_num, cx_start, cx_pos, cx_sub, cx_subpos, cx_inn] {
        vline(layer, vx, table_bottom, table_top, 0.25, grid());
    }

    // Column header row.
    let hs = small - 1.0;
    let hdr_base = table_top - col_hdr_h + (col_hdr_h - hs * 0.35) / 2.0;
    text(layer, &fonts.bold, "#", hs, center_x("#", hs, cx_num, w_num), hdr_base, muted());
    text(layer, &fonts.bold, "STARTER", hs, cx_start + 1.0, hdr_base, muted());
    text(layer, &fonts.bold, "POS", hs, center_x("POS", hs, cx_pos, w_pos), hdr_base, muted());
    text(layer, &fonts.bold, "SUBSTITUTE", hs, cx_sub + 1.0, hdr_base, muted());
    text(layer, &fonts.bold, "POS", hs, center_x("POS", hs, cx_subpos, w_subpos), hdr_base, muted());
    text(layer, &fonts.bold, "INN", hs, center_x("INN", hs, cx_inn, w_inn), hdr_base, muted());
    hline(layer, x, x + w, table_top - col_hdr_h, 0.5, ink());

    let cell_size = small - 0.5;
    for i in 0..n_data {
        let row_top = data_top - i as f64 * row_h;
        let row_bottom = row_top - row_h;
        if i > 0 {
            hline(layer, x, x + w, row_top, 0.25, grid());
        }
        // Split the substitute area so two substitutes can be recorded per spot.
        hline(layer, cx_sub, x + w, row_bottom + row_h / 2.0, 0.18, grid());
        if i >= spots.len() {
            continue;
        }
        let spot = &spots[i];
        let baseline = row_bottom + (row_h - cell_size * 0.35) / 2.0;
        let is_def = spot.slot_kind == "DEF";

        if is_def {
            text(
                layer,
                &fonts.bold,
                "DEF",
                small - 1.0,
                center_x("DEF", small - 1.0, cx_order, w_order),
                baseline,
                green(),
            );
        } else {
            let order_label = match spot.slot_kind.as_str() {
                "EH" => "EH".to_string(),
                _ => spot.batting_order.to_string(),
            };
            let ord_size = base + 2.0;
            let ord_base = row_bottom + (row_h - ord_size * 0.35) / 2.0;
            text(
                layer,
                &fonts.bold,
                &order_label,
                ord_size,
                center_x(&order_label, ord_size, cx_order, w_order),
                ord_base,
                red(),
            );
        }

        let (num, pname) = match spot.player_id.and_then(|id| player_map.get(&id)) {
            Some(p) => (p.number.clone(), p.name.clone()),
            None => (String::new(), String::new()),
        };
        text(layer, &fonts.bold, &num, cell_size, cx_num + 1.0, baseline, ink());
        let pname = truncate_to(&pname, cell_size, w_start - 2.0);
        text(layer, &fonts.regular, &pname, cell_size, cx_start + 1.0, baseline, ink());
        let pos_label = truncate_to(&spot.position, cell_size, w_pos - 1.0);
        let pos_color = if is_def { green() } else { ink() };
        text(layer, &fonts.bold, &pos_label, cell_size, cx_pos + 1.0, baseline, pos_color);
    }
}

/// Best-effort logo embedding. Returns the rendered width in mm on success.
fn place_logo(
    layer: &PdfLayerReference,
    fs_path: &str,
    x: f64,
    y: f64,
    box_w: f64,
    box_h: f64,
) -> AppResult<f64> {
    use printpdf::image_crate::GenericImageView;
    let dynimg = printpdf::image_crate::open(fs_path)
        .map_err(|e| AppError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let (w_px, h_px) = {
        let (wp, hp) = dynimg.dimensions();
        (wp as f64, hp as f64)
    };
    if w_px <= 0.0 || h_px <= 0.0 {
        return Err(AppError::new(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "empty image",
        ));
    }
    let dpi = (w_px * IN / box_w).max(h_px * IN / box_h);
    let rendered_w = w_px * IN / dpi;
    let rendered_h = h_px * IN / dpi;
    let ty = y + (box_h - rendered_h) / 2.0;
    let img = Image::from_dynamic_image(&dynimg);
    img.add_to_layer(
        layer.clone(),
        ImageTransform {
            translate_x: Some(mm(x)),
            translate_y: Some(mm(ty)),
            dpi: Some(dpi as f32),
            ..Default::default()
        },
    );
    Ok(rendered_w)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> (Team, Lineup, Vec<LineupSpot>, Vec<Player>) {
        let team = Team {
            id: 1,
            user_id: 1,
            name: "River Cats".into(),
            head_coach: "J. Smith".into(),
            assistant_coaches: "A. Lee".into(),
            logo_path: None,
            created_at: String::new(),
        };
        let lineup = Lineup {
            id: 1,
            team_id: 1,
            name: "Opener".into(),
            opponent: "Bulls".into(),
            game_date: "2026-07-10".into(),
            location: "City Park".into(),
            home_away: "home".into(),
            use_dh: 0,
            use_eh: 0,
            dh_mode: "traditional".into(),
            created_at: String::new(),
        };
        let players = vec![
            Player { id: 1, team_id: 1, number: "7".into(), name: "Rodriguez".into(), default_position: "DH".into(), sort_order: 1 },
            Player { id: 2, team_id: 1, number: "30".into(), name: "Miller".into(), default_position: "P".into(), sort_order: 2 },
            Player { id: 3, team_id: 1, number: "5".into(), name: "Available".into(), default_position: "".into(), sort_order: 3 },
        ];
        let spots = vec![
            LineupSpot { id: 1, lineup_id: 1, batting_order: 1, slot_kind: "BAT".into(), player_id: Some(1), position: "DH".into(), is_dh: 0 },
            LineupSpot { id: 2, lineup_id: 1, batting_order: 10, slot_kind: "DEF".into(), player_id: Some(2), position: "P".into(), is_dh: 0 },
        ];
        (team, lineup, spots, players)
    }

    #[test]
    fn renders_valid_pdf_bytes() {
        let (team, lineup, spots, players) = sample();
        let req = PdfRequest { coach: 1, scorekeeper: 1, self_copy: 1, umpire: 2 };
        let bytes =
            render_sheet(&team, &lineup, &spots, &players, &req, "uploads").expect("render ok");
        assert!(bytes.len() > 1000);
        assert_eq!(&bytes[0..5], b"%PDF-");
    }

    #[test]
    fn packs_all_requested_cards() {
        let cards: Vec<Card> = (0..7)
            .map(|i| Card {
                variant: if i < 3 { Variant::Large } else { Variant::Umpire },
                recipient: "X".into(),
            })
            .collect();
        let placements = pack(&cards);
        assert_eq!(placements.len(), cards.len());
        for p in &placements {
            let (w, h) = cards[p.card_index].variant.size_mm();
            assert!(p.x >= 0.0 && p.x + w <= PAGE_W + 0.1);
            assert!(p.y >= 0.0 && p.y + h <= PAGE_H + 0.1);
        }
    }
}
