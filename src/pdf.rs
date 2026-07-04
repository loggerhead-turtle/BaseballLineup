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

#[derive(Clone, Copy, PartialEq)]
enum Variant {
    Large,
    Umpire,
}

impl Variant {
    fn size_mm(self) -> (f64, f64) {
        match self {
            // 3.75 in wide, tall enough for the batting order, per-row
            // substitute columns, and the full available-players list.
            Variant::Large => (3.75 * IN, 8.0 * IN),
            // 3.5 in wide fits standard umpire holders; runs tall (the umpire
            // folds it) so the whole card fits.
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
    x: f64, // bottom-left, mm
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
    // Build the ordered list of cards (group large recipients first, umpire last).
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

    // Shelf-pack the cards onto Letter pages.
    let placements = pack(&cards);
    let page_count = placements.iter().map(|p| p.page + 1).max().unwrap_or(1);

    // Create the document + pages.
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
    let mut cursor_top = MARGIN; // distance from top of page
    let mut shelf_h = 0.0f64;

    for (i, card) in cards.iter().enumerate() {
        let (w, h) = card.variant.size_mm();

        // Wrap to a new shelf if the card would overflow the right edge.
        if cursor_x + w > usable_right + 0.01 {
            cursor_x = MARGIN;
            cursor_top += shelf_h + GAP;
            shelf_h = 0.0;
        }
        // Wrap to a new page if the card would overflow the bottom.
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

// --- primitive drawing helpers ---

fn set_stroke(layer: &PdfLayerReference, thickness: f64) {
    layer.set_outline_thickness(thickness as f32);
    layer.set_outline_color(Color::Rgb(Rgb::new(0.0, 0.0, 0.0, None)));
}

fn rect(layer: &PdfLayerReference, x: f64, y: f64, w: f64, h: f64, thickness: f64) {
    set_stroke(layer, thickness);
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

fn hline(layer: &PdfLayerReference, x1: f64, x2: f64, y: f64, thickness: f64) {
    set_stroke(layer, thickness);
    let points = vec![
        (Point::new(mm(x1), mm(y)), false),
        (Point::new(mm(x2), mm(y)), false),
    ];
    layer.add_line(Line {
        points,
        is_closed: false,
    });
}

fn vline(layer: &PdfLayerReference, x: f64, y1: f64, y2: f64, thickness: f64) {
    set_stroke(layer, thickness);
    let points = vec![
        (Point::new(mm(x), mm(y1)), false),
        (Point::new(mm(x), mm(y2)), false),
    ];
    layer.add_line(Line {
        points,
        is_closed: false,
    });
}

fn text(layer: &PdfLayerReference, font: &IndirectFontRef, s: &str, size: f64, x: f64, baseline_y: f64) {
    layer.use_text(s, size as f32, mm(x), mm(baseline_y), font);
}

/// Rough width of a Helvetica string in mm (average glyph ~0.52em).
fn approx_width(s: &str, size_pt: f64) -> f64 {
    s.chars().count() as f64 * size_pt * 0.352778 * 0.52
}

fn truncate_to(s: &str, size_pt: f64, max_mm: f64) -> String {
    if approx_width(s, size_pt) <= max_mm {
        return s.to_string();
    }
    let mut out = String::new();
    for ch in s.chars() {
        let candidate_w = approx_width(&format!("{out}{ch}\u{2026}"), size_pt);
        if candidate_w > max_mm {
            break;
        }
        out.push(ch);
    }
    if out.is_empty() {
        out.push(s.chars().next().unwrap_or(' '));
    }
    format!("{out}\u{2026}")
}

/// Center-aligned x for a label within a column of width `col_w` starting at `col_x`.
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
    let pad = 2.2;
    let base = variant.base_font();
    let small = base - 2.0;
    let top = y + h;
    let il = x + pad; // inner left
    let ir = x + w - pad; // inner right
    let iw = ir - il; // inner width

    // Card border (doubles as the cut line).
    rect(layer, x, y, w, h, 0.7);

    // ---- Header: logo (no border) + team / opponent / date ----
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
        // Fit within a box up to 1.7x as wide as tall; no border around the logo.
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
        text(layer, &fonts.bold, &initial, s, logo_x, logo_y + (logo_h - s * 0.35) / 2.0);
        logo_w_used = approx_width(&initial, s) + 1.0;
    }

    // Team name + game info to the right of the logo.
    let info_x = logo_x + logo_w_used + pad + 1.0;
    let info_w = x + w - pad - info_x;
    let name = truncate_to(&team.name, base + 2.0, info_w);
    text(layer, &fonts.bold, &name, base + 2.0, info_x, top - pad - (base + 2.0) * 0.35);

    let mut line_y = top - pad - (base + 2.0) * 0.35 - (base * 0.5) - 1.6;
    if !lineup.opponent.is_empty() {
        let vs = truncate_to(&format!("vs {}", lineup.opponent), base, info_w);
        text(layer, &fonts.regular, &vs, base, info_x, line_y);
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
        let meta = truncate_to(&meta_bits.join("  •  "), base - 1.5, info_w);
        text(layer, &fonts.regular, &meta, base - 1.5, info_x, line_y);
    }

    // Recipient tag, top-right.
    let tag_size = base - 2.0;
    let tag_w = approx_width(recipient, tag_size) * 1.32;
    let tag_x = (x + w - pad - tag_w).max(info_x);
    text(layer, &fonts.bold, recipient, tag_size, tag_x, top - pad - (base + 2.0) * 0.35);

    hline(layer, x, x + w, header_bottom, 0.6);

    // ---- Footer: coaches only (no signature) ----
    let footer_h = if variant == Variant::Large { 13.0 } else { 11.0 };
    let footer_top = y + footer_h;
    hline(layer, x, x + w, footer_top, 0.6);
    let mut fy = footer_top - 3.0 - small * 0.35;
    if !team.head_coach.is_empty() {
        let s = truncate_to(&format!("Head Coach: {}", team.head_coach), small, w - 2.0 * pad);
        text(layer, &fonts.regular, &s, small, x + pad, fy);
        fy -= small * 0.5 + 1.8;
    }
    if !team.assistant_coaches.is_empty() {
        let s = truncate_to(&format!("Assistants: {}", team.assistant_coaches), small, w - 2.0 * pad);
        text(layer, &fonts.regular, &s, small, x + pad, fy);
    }

    // ---- Available players (bench), two columns above the footer ----
    let assigned: Vec<i64> = spots.iter().filter_map(|s| s.player_id).collect();
    let subs: Vec<&Player> = players.iter().filter(|p| !assigned.contains(&p.id)).collect();

    let av_size = small - 0.5;
    let av_line = av_size * 0.5 + 2.6;
    let av_head = av_size + 2.5;
    // At least three lines so there's always room to write in late arrivals.
    let left_count = (subs.len() + 1) / 2;
    let av_rows = left_count.max(3);
    let avail_h = av_head + av_rows as f64 * av_line + 2.0;
    let avail_top = footer_top + avail_h;
    hline(layer, x, x + w, avail_top, 0.6);

    let av_num_w = iw * 0.085;
    let av_half = iw / 2.0;
    let av_cols = [il, il + av_half];
    let av_hdr_base = avail_top - av_head + 1.0;
    for &cxa in av_cols.iter() {
        text(layer, &fonts.bold, "#", av_size, cxa + 0.8, av_hdr_base);
        text(layer, &fonts.bold, "PLAYER AVAILABLE", av_size - 0.5, cxa + av_num_w + 1.0, av_hdr_base);
    }
    // Separators for the available block.
    vline(layer, il + av_num_w, footer_top, avail_top, 0.3);
    vline(layer, il + av_half, footer_top, avail_top, 0.3);
    vline(layer, il + av_half + av_num_w, footer_top, avail_top, 0.3);
    let av_rows_top = avail_top - av_head;
    hline(layer, x, x + w, av_rows_top, 0.3);

    for r in 0..av_rows {
        let row_top = av_rows_top - r as f64 * av_line;
        if r > 0 {
            hline(layer, x, x + w, row_top, 0.15);
        }
        let baseline = row_top - av_line + av_line * 0.3;
        let entries = [subs.get(r), subs.get(left_count + r)];
        for (c, e) in entries.iter().enumerate() {
            if let Some(p) = e {
                let cxa = av_cols[c];
                text(layer, &fonts.bold, &p.number, av_size, cxa + 0.8, baseline);
                let nm = truncate_to(&p.name, av_size, av_half - av_num_w - 2.5);
                text(layer, &fonts.regular, &nm, av_size, cxa + av_num_w + 1.0, baseline);
            }
        }
    }

    // ---- Main lineup table ----
    let table_top = header_bottom;
    let table_bottom = avail_top;

    // Column widths as fractions of the inner width:
    // ORDER | # | STARTER | POS | SUBSTITUTE | POS | INN
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

    // Vertical separators across the full table height.
    for &vx in &[cx_num, cx_start, cx_pos, cx_sub, cx_subpos, cx_inn] {
        vline(layer, vx, table_bottom, table_top, 0.3);
    }

    // Column header row.
    let hs = small - 1.0;
    let col_hdr_h = small + 2.5;
    let hdr_base = table_top - col_hdr_h + (col_hdr_h - hs * 0.35) / 2.0;
    text(layer, &fonts.bold, "ORDER", hs, center_x("ORDER", hs, cx_order, w_order), hdr_base);
    text(layer, &fonts.bold, "#", hs, center_x("#", hs, cx_num, w_num), hdr_base);
    text(layer, &fonts.bold, "STARTER", hs, cx_start + 1.0, hdr_base);
    text(layer, &fonts.bold, "POS", hs, center_x("POS", hs, cx_pos, w_pos), hdr_base);
    text(layer, &fonts.bold, "SUBSTITUTE", hs, cx_sub + 1.0, hdr_base);
    text(layer, &fonts.bold, "POS", hs, center_x("POS", hs, cx_subpos, w_subpos), hdr_base);
    text(layer, &fonts.bold, "INN", hs, center_x("INN", hs, cx_inn, w_inn), hdr_base);
    hline(layer, x, x + w, table_top - col_hdr_h, 0.4);

    // Data rows: every batting spot, plus two blank rows for write-ins.
    let n_data = spots.len() + 2;
    let data_top = table_top - col_hdr_h;
    let row_h = (data_top - table_bottom) / n_data as f64;
    let cell_size = small - 0.5;
    for i in 0..n_data {
        let row_top = data_top - i as f64 * row_h;
        let row_bottom = row_top - row_h;
        if i > 0 {
            hline(layer, x, x + w, row_top, 0.2);
        }
        if i >= spots.len() {
            continue; // blank extra rows
        }
        let spot = &spots[i];
        let baseline = row_bottom + (row_h - cell_size * 0.35) / 2.0;

        // ORDER — large bold numeral, centered.
        let order_label = match spot.slot_kind.as_str() {
            "DH" => "DH".to_string(),
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
        );

        // Starter number + name.
        let (num, pname) = match spot.player_id.and_then(|id| player_map.get(&id)) {
            Some(p) => (p.number.clone(), p.name.clone()),
            None => (String::new(), String::new()),
        };
        text(layer, &fonts.regular, &num, cell_size, cx_num + 1.0, baseline);
        let pname = truncate_to(&pname, cell_size, w_start - 2.0);
        text(layer, &fonts.regular, &pname, cell_size, cx_start + 1.0, baseline);

        // Starter position (two-way DH renders as "SS/DH").
        let pos_label = if spot.is_dh != 0 {
            if spot.position.is_empty() {
                "DH".to_string()
            } else {
                format!("{}/DH", spot.position)
            }
        } else {
            spot.position.clone()
        };
        let pos_label = truncate_to(&pos_label, cell_size, w_pos - 1.0);
        text(layer, &fonts.bold, &pos_label, cell_size, cx_pos + 1.0, baseline);

        // SUBSTITUTE / POS / INN columns are left blank for in-game write-in.
    }
}

/// Best-effort logo embedding. Returns the rendered width in mm on success, so
/// the caller can flow text next to it. Returns Err if the image can't be
/// decoded/placed (the caller then falls back to the team initial).
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
    // Choose a DPI so the image fits within the box while preserving aspect ratio.
    let dpi = (w_px * IN / box_w).max(h_px * IN / box_h);
    let rendered_w = w_px * IN / dpi;
    let rendered_h = h_px * IN / dpi;
    // Vertically center within the box.
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
            use_dh: 1,
            use_eh: 0,
            created_at: String::new(),
        };
        let players = vec![
            Player {
                id: 1,
                team_id: 1,
                number: "7".into(),
                name: "Rodriguez".into(),
                default_position: "SS".into(),
                sort_order: 1,
            },
            Player {
                id: 2,
                team_id: 1,
                number: "22".into(),
                name: "Chen".into(),
                default_position: "CF".into(),
                sort_order: 2,
            },
        ];
        let spots = vec![
            LineupSpot {
                id: 1,
                lineup_id: 1,
                batting_order: 1,
                slot_kind: "BAT".into(),
                player_id: Some(1),
                position: "P".into(),
                is_dh: 1,
            },
            LineupSpot {
                id: 2,
                lineup_id: 1,
                batting_order: 2,
                slot_kind: "BAT".into(),
                player_id: None,
                position: "SS".into(),
                is_dh: 0,
            },
        ];
        (team, lineup, spots, players)
    }

    #[test]
    fn renders_valid_pdf_bytes() {
        let (team, lineup, spots, players) = sample();
        let req = PdfRequest {
            coach: 1,
            scorekeeper: 1,
            self_copy: 1,
            umpire: 2,
        };
        let bytes =
            render_sheet(&team, &lineup, &spots, &players, &req, "uploads").expect("render ok");
        assert!(bytes.len() > 1000, "pdf should be non-trivial");
        assert_eq!(&bytes[0..5], b"%PDF-", "should start with a PDF header");
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
        assert_eq!(placements.len(), cards.len(), "every card is placed");
        for p in &placements {
            let (w, h) = cards[p.card_index].variant.size_mm();
            assert!(p.x >= 0.0 && p.x + w <= PAGE_W + 0.1);
            assert!(p.y >= 0.0 && p.y + h <= PAGE_H + 0.1);
        }
    }
}
