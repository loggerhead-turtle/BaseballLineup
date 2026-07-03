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
            // 3.75 x 5.5 in — roomy, still fits two across a Letter page.
            Variant::Large => (3.75 * IN, 5.5 * IN),
            // 3.5 x 5.0 in — fits standard umpire lineup-card holders.
            Variant::Umpire => (3.5 * IN, 5.0 * IN),
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
        regular: doc
            .add_builtin_font(BuiltinFont::Helvetica)
            .map_err(|e| AppError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
        bold: doc
            .add_builtin_font(BuiltinFont::HelveticaBold)
            .map_err(|e| AppError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
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
            &player_map,
        );
    }

    let mut buf: Vec<u8> = Vec::new();
    {
        let mut writer = BufWriter::new(&mut buf);
        doc.save(&mut writer)
            .map_err(|e| AppError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
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

fn text(
    layer: &PdfLayerReference,
    font: &IndirectFontRef,
    s: &str,
    size: f64,
    x: f64,
    baseline_y: f64,
) {
    layer.use_text(s, size as f32, mm(x), mm(baseline_y), font);
}

/// Rough width of a Helvetica string in mm (average glyph ~0.5em).
fn approx_width(s: &str, size_pt: f64) -> f64 {
    // 1 pt = 0.352778 mm; assume mean advance of 0.52 em.
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
    player_map: &HashMap<i64, &Player>,
) {
    let pad = 2.2;
    let base = variant.base_font();
    let top = y + h;

    // Card border (this doubles as the cut line).
    rect(layer, x, y, w, h, 0.7);

    // ---- Header ----
    let header_h = if variant == Variant::Large { 20.0 } else { 16.0 };
    let header_bottom = top - header_h;

    // Logo box on the left.
    let logo_size = header_h - 2.0 * pad;
    let logo_x = x + pad;
    let logo_y = header_bottom + pad;
    rect(layer, logo_x, logo_y, logo_size, logo_size, 0.4);
    let mut logo_drawn = false;
    if let Some(path) = &team.logo_path {
        let fs_path = format!(".{path}");
        if place_logo(layer, &fs_path, logo_x + 0.6, logo_y + 0.6, logo_size - 1.2, logo_size - 1.2)
            .is_ok()
        {
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
        text(
            layer,
            &fonts.bold,
            &initial,
            base + 4.0,
            logo_x + logo_size / 2.0 - 2.0,
            logo_y + logo_size / 2.0 - 2.0,
        );
    }

    // Team name + game info to the right of the logo.
    let info_x = logo_x + logo_size + pad;
    let info_w = x + w - pad - info_x;
    let name = truncate_to(&team.name, base + 2.0, info_w);
    text(layer, &fonts.bold, &name, base + 2.0, info_x, top - pad - (base + 2.0) * 0.35);

    let mut line_y = top - pad - (base + 2.0) * 0.35 - (base * 0.5) - 1.5;
    let vs = if lineup.opponent.is_empty() {
        String::new()
    } else {
        format!("vs {}", lineup.opponent)
    };
    if !vs.is_empty() {
        let vs = truncate_to(&vs, base, info_w);
        text(layer, &fonts.regular, &vs, base, info_x, line_y);
        line_y -= base * 0.5 + 1.2;
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

    // Recipient tag, aligned to the team-name baseline in the top-right corner.
    // Bold uppercase runs wider than the average estimate, so pad the width.
    let tag_size = base - 2.0;
    let tag_w = approx_width(recipient, tag_size) * 1.32;
    let tag_x = (x + w - pad - tag_w).max(info_x);
    text(
        layer,
        &fonts.bold,
        recipient,
        tag_size,
        tag_x,
        top - pad - (base + 2.0) * 0.35,
    );

    hline(layer, x, x + w, header_bottom, 0.5);

    // ---- Footer (coaches + signature) ----
    let footer_h = if variant == Variant::Large { 20.0 } else { 16.0 };
    let footer_top = y + footer_h;
    hline(layer, x, x + w, footer_top, 0.5);

    let small = base - 2.0;
    let mut fy = footer_top - 3.0 - small * 0.35;
    if !team.head_coach.is_empty() {
        let s = truncate_to(&format!("Head Coach: {}", team.head_coach), small, w - 2.0 * pad);
        text(layer, &fonts.regular, &s, small, x + pad, fy);
        fy -= small * 0.5 + 1.8;
    }
    if !team.assistant_coaches.is_empty() {
        let s = truncate_to(
            &format!("Assistants: {}", team.assistant_coaches),
            small,
            w - 2.0 * pad,
        );
        text(layer, &fonts.regular, &s, small, x + pad, fy);
    }
    // Signature line sits below the coach lines, clear of the text above.
    let sig_y = y + 5.0;
    hline(layer, x + pad, x + w * 0.62, sig_y, 0.3);
    text(layer, &fonts.regular, "Manager signature", small - 1.5, x + pad, y + 1.6);

    // ---- Batting order table ----
    let table_top = header_bottom - 1.0;
    let table_bottom = footer_top + 1.0;
    let table_h = table_top - table_bottom;

    // Columns: order | no | player | pos
    let col_order = x + pad;
    let order_w = if variant == Variant::Large { 7.0 } else { 6.0 };
    let no_w = if variant == Variant::Large { 8.0 } else { 7.0 };
    let pos_w = if variant == Variant::Large { 12.0 } else { 10.0 };
    let col_no = col_order + order_w;
    let col_player = col_no + no_w;
    let col_pos_right = x + w - pad;
    let col_pos = col_pos_right - pos_w;

    // Header row for the table.
    let hdr_size = small - 0.5;
    let rows = spots.len().max(1);
    let row_h = table_h / rows as f64;

    // Column separator lines.
    vline(layer, col_no - 1.0, table_bottom, table_top, 0.3);
    vline(layer, col_player - 1.0, table_bottom, table_top, 0.3);
    vline(layer, col_pos - 1.0, table_bottom, table_top, 0.3);

    for (i, spot) in spots.iter().enumerate() {
        let row_top = table_top - i as f64 * row_h;
        let row_bottom = row_top - row_h;
        if i > 0 {
            hline(layer, x, x + w, row_top, 0.2);
        }
        let baseline = row_bottom + (row_h - hdr_size * 0.35) / 2.0;

        let order_label = match spot.slot_kind.as_str() {
            "DH" => "DH".to_string(),
            "EH" => "EH".to_string(),
            _ => spot.batting_order.to_string(),
        };
        text(layer, &fonts.bold, &order_label, hdr_size, col_order, baseline);

        let (num, pname) = match spot.player_id.and_then(|id| player_map.get(&id)) {
            Some(p) => (p.number.clone(), p.name.clone()),
            None => (String::new(), String::new()),
        };
        text(layer, &fonts.regular, &num, hdr_size, col_no, baseline);
        let pname = truncate_to(&pname, hdr_size, col_pos - col_player - 1.5);
        text(layer, &fonts.regular, &pname, hdr_size, col_player, baseline);
        text(layer, &fonts.bold, &spot.position, hdr_size, col_pos, baseline);
    }
}

/// Best-effort logo embedding. Returns Err if the image cannot be decoded/placed,
/// in which case the caller falls back to drawing the team initial.
fn place_logo(
    layer: &PdfLayerReference,
    fs_path: &str,
    x: f64,
    y: f64,
    box_w: f64,
    box_h: f64,
) -> AppResult<()> {
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
    let img = Image::from_dynamic_image(&dynimg);
    img.add_to_layer(
        layer.clone(),
        ImageTransform {
            translate_x: Some(mm(x)),
            translate_y: Some(mm(y)),
            dpi: Some(dpi as f32),
            ..Default::default()
        },
    );
    Ok(())
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
        let players = vec![Player {
            id: 1,
            team_id: 1,
            number: "7".into(),
            name: "Rodriguez".into(),
            default_position: "SS".into(),
            sort_order: 1,
        }];
        let spots = vec![
            LineupSpot {
                id: 1,
                lineup_id: 1,
                batting_order: 1,
                slot_kind: "BAT".into(),
                player_id: Some(1),
                position: "SS".into(),
            },
            LineupSpot {
                id: 2,
                lineup_id: 1,
                batting_order: 10,
                slot_kind: "DH".into(),
                player_id: None,
                position: "DH".into(),
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
        let bytes = render_sheet(&team, &lineup, &spots, &players, &req).expect("render ok");
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
        // Every placement must sit within the page bounds.
        for p in &placements {
            let (w, h) = cards[p.card_index].variant.size_mm();
            assert!(p.x >= 0.0 && p.x + w <= PAGE_W + 0.1);
            assert!(p.y >= 0.0 && p.y + h <= PAGE_H + 0.1);
        }
    }
}
