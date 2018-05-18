use std::cmp::{min};

#[derive(PartialEq, Eq, Clone, Debug, Copy)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}


#[derive(PartialEq, Eq, Clone, Debug, Copy)]
pub enum Margin {
    Fixed(u16),
    Percent(u16),
}

impl Margin {
    pub fn parse_margin_string(margin: &str) -> Margin {
        if margin.ends_with('%') {
            Margin::Percent(min(
                100,
                margin[0..margin.len() - 1].parse::<u16>().unwrap_or(100),
            ))
        } else {
            Margin::Fixed(margin.parse::<u16>().unwrap_or(0))
        }
    }

    pub fn parse_margin(margin_option: &str) -> (Margin, Margin, Margin, Margin) {
        let margins = margin_option.split(',').collect::<Vec<&str>>();

        match margins.len() {
            1 => {
                let margin = Self::parse_margin_string(margins[0]);
                (margin, margin, margin, margin)
            }
            2 => {
                let margin_tb = Self::parse_margin_string(margins[0]);
                let margin_rl = Self::parse_margin_string(margins[1]);
                (margin_tb, margin_rl, margin_tb, margin_rl)
            }
            3 => {
                let margin_top = Self::parse_margin_string(margins[0]);
                let margin_rl = Self::parse_margin_string(margins[1]);
                let margin_bottom = Self::parse_margin_string(margins[2]);
                (margin_top, margin_rl, margin_bottom, margin_rl)
            }
            4 => {
                let margin_top = Self::parse_margin_string(margins[0]);
                let margin_right = Self::parse_margin_string(margins[1]);
                let margin_bottom = Self::parse_margin_string(margins[2]);
                let margin_left = Self::parse_margin_string(margins[3]);
                (margin_top, margin_right, margin_bottom, margin_left)
            }
            _ => (
                Margin::Fixed(0),
                Margin::Fixed(0),
                Margin::Fixed(0),
                Margin::Fixed(0),
            ),
        }
    }
}
