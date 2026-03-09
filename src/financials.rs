use crate::models::{IndicatorData, StockIndicators};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FinancialSelection {
    pub section: usize,
    pub row: usize,
}

impl FinancialSelection {
    pub const fn new(section: usize, row: usize) -> Self {
        Self { section, row }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum NavDir {
    Up,
    Down,
    Left,
    Right,
}

pub struct FinancialSection<'a> {
    pub title: &'static str,
    pub rows: Vec<(&'static str, &'a IndicatorData)>,
}

const GRID: &[(usize, usize)] = &[(0, 0), (0, 1), (1, 0), (1, 1), (2, 0)];

pub fn sections(data: &StockIndicators) -> Vec<FinancialSection<'_>> {
    vec![
        FinancialSection {
            title: "Valuation",
            rows: vec![
                ("DY", &data.valuation.dy),
                ("P/E", &data.valuation.p_e),
                ("P/B", &data.valuation.p_b),
                ("P/EBITDA", &data.valuation.p_ebitda),
                ("P/EBIT", &data.valuation.p_ebit),
                ("P/S", &data.valuation.p_s),
                ("P/Assets", &data.valuation.p_assets),
                ("P/Working Capital", &data.valuation.p_working_capital),
                ("P/Net Current Assets", &data.valuation.p_net_current_assets),
                ("EV/EBITDA", &data.valuation.ev_ebitda),
                ("EV/EBIT", &data.valuation.ev_ebit),
                ("EPS", &data.valuation.eps),
                ("BVPS", &data.valuation.bvps),
            ],
        },
        FinancialSection {
            title: "Debt",
            rows: vec![
                ("Net Debt / Equity", &data.debt.net_debt_equity),
                ("Net Debt / EBITDA", &data.debt.net_debt_ebitda),
                ("Net Debt / EBIT", &data.debt.net_debt_ebit),
                ("Equity / Assets", &data.debt.equity_to_assets),
                ("Liabilities / Assets", &data.debt.liabilities_to_assets),
                ("Current Ratio", &data.debt.current_ratio),
            ],
        },
        FinancialSection {
            title: "Efficiency",
            rows: vec![
                ("Gross Margin", &data.efficiency.gross_margin),
                ("EBITDA Margin", &data.efficiency.ebitda_margin),
                ("EBIT Margin", &data.efficiency.ebit_margin),
                ("Net Margin", &data.efficiency.net_margin),
            ],
        },
        FinancialSection {
            title: "Profitability",
            rows: vec![
                ("ROE", &data.profitability.roe),
                ("ROA", &data.profitability.roa),
                ("ROIC", &data.profitability.roic),
                ("Asset Turnover", &data.profitability.asset_turnover),
            ],
        },
        FinancialSection {
            title: "Growth",
            rows: vec![
                ("Revenue CAGR 5Y", &data.growth.revenue_cagr5),
                ("Earnings CAGR 5Y", &data.growth.earnings_cagr5),
            ],
        },
    ]
}

pub fn clamp_selection(
    selection: FinancialSelection,
    sections: &[FinancialSection<'_>],
) -> FinancialSelection {
    if sections.is_empty() {
        return FinancialSelection::new(0, 0);
    }
    let section = selection.section.min(sections.len().saturating_sub(1));
    let row_max = sections[section].rows.len().saturating_sub(1);
    let row = if sections[section].rows.is_empty() {
        0
    } else {
        selection.row.min(row_max)
    };
    FinancialSelection::new(section, row)
}

pub fn move_selection(
    selection: FinancialSelection,
    dir: NavDir,
    sections: &[FinancialSection<'_>],
) -> FinancialSelection {
    if sections.is_empty() {
        return selection;
    }

    let selection = clamp_selection(selection, sections);
    let (grid_row, grid_col) = section_position(selection.section);

    match dir {
        NavDir::Up => move_vertical(selection, grid_row, grid_col, sections, true),
        NavDir::Down => move_vertical(selection, grid_row, grid_col, sections, false),
        NavDir::Left => move_horizontal(selection, grid_row, grid_col, sections, true),
        NavDir::Right => move_horizontal(selection, grid_row, grid_col, sections, false),
    }
}

pub fn indicator_from_selection<'a>(
    sections: &'a [FinancialSection<'a>],
    selection: FinancialSelection,
) -> Option<(&'static str, &'a IndicatorData)> {
    let section = sections.get(selection.section)?;
    let (name, data) = section.rows.get(selection.row)?;
    Some((*name, *data))
}

fn section_position(section: usize) -> (usize, usize) {
    GRID.get(section).copied().unwrap_or((0, 0))
}

fn section_at(row: usize, col: usize) -> Option<usize> {
    GRID.iter().position(|(r, c)| *r == row && *c == col)
}

fn move_vertical(
    selection: FinancialSelection,
    grid_row: usize,
    grid_col: usize,
    sections: &[FinancialSection<'_>],
    up: bool,
) -> FinancialSelection {
    let row_count = sections[selection.section].rows.len();
    if up {
        if selection.row > 0 {
            return FinancialSelection::new(selection.section, selection.row - 1);
        }
        if grid_row == 0 {
            return selection;
        }
        if let Some(above) = section_at(grid_row - 1, grid_col) {
            let above_rows = sections[above].rows.len();
            let row = above_rows.saturating_sub(1);
            return FinancialSelection::new(above, row);
        }
    } else {
        if selection.row + 1 < row_count {
            return FinancialSelection::new(selection.section, selection.row + 1);
        }
        if let Some(below) = section_at(grid_row + 1, grid_col) {
            return FinancialSelection::new(below, 0);
        }
    }
    selection
}

fn move_horizontal(
    selection: FinancialSelection,
    grid_row: usize,
    grid_col: usize,
    sections: &[FinancialSection<'_>],
    left: bool,
) -> FinancialSelection {
    let target_col = if left {
        grid_col.saturating_sub(1)
    } else {
        grid_col + 1
    };
    let Some(target_section) = section_at(grid_row, target_col) else {
        return selection;
    };
    let row_count = sections[target_section].rows.len();
    let row = if row_count == 0 {
        0
    } else {
        selection.row.min(row_count - 1)
    };
    FinancialSelection::new(target_section, row)
}
