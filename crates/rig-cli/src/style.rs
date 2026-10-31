//! One palette for the whole UI. Colour carries meaning, so the same thing is always the same colour:
//! green is fine, yellow is working or needs care, red failed or risky, cyan names a model,
//! violet is reasoning effort, blue is neutral access.

use rig_core::permission::Autonomy;

pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";
pub const ITALIC: &str = "\x1b[3m";
pub const UNDERLINE: &str = "\x1b[4m";
pub const STRIKE: &str = "\x1b[9m";
pub const GREEN: &str = "\x1b[38;5;78m";
pub const RED: &str = "\x1b[38;5;203m";
pub const YELLOW: &str = "\x1b[38;5;220m";
pub const BLUE: &str = "\x1b[38;5;75m";
pub const CYAN: &str = "\x1b[38;5;80m";
pub const VIOLET: &str = "\x1b[38;5;141m";
pub const MAGENTA: &str = "\x1b[35m";

/// `text` in `style`, or untouched when colour is off.
pub fn paint(on: bool, style: &str, text: &str) -> String {
    if on {
        format!("{style}{text}{RESET}")
    } else {
        text.to_string()
    }
}

/// More access, warmer colour: asking and read-only are calm, high is a warning.
pub fn permission(a: Autonomy) -> &'static str {
    match a {
        Autonomy::Ask | Autonomy::ReadOnly => GREEN,
        Autonomy::Low => BLUE,
        Autonomy::Medium => YELLOW,
        Autonomy::High => RED,
    }
}

/// How full the context window is: quiet until it matters.
pub fn context(percent: u8) -> &'static str {
    match percent {
        0..=49 => "",
        50..=79 => YELLOW,
        _ => RED,
    }
}

/// A running tool's dot breathes through yellow: bright, dim, bright over one second.
pub fn pulse(frame: usize) -> &'static str {
    const FRAMES: [&str; 10] = [
        "\x1b[38;5;226m",
        "\x1b[38;5;220m",
        "\x1b[38;5;214m",
        "\x1b[38;5;178m",
        "\x1b[38;5;136m",
        "\x1b[38;5;94m",
        "\x1b[38;5;136m",
        "\x1b[38;5;178m",
        "\x1b[38;5;214m",
        "\x1b[38;5;220m",
    ];
    FRAMES[frame % FRAMES.len()]
}

/// How risky an approval reason sounds.
pub fn risk_in(reason: &str) -> &'static str {
    let r = reason.to_lowercase();
    if r.contains("high risk") || r.contains("high-risk") {
        RED
    } else if r.contains("low risk") {
        GREEN
    } else {
        YELLOW
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn more_access_is_never_a_calmer_colour() {
        assert_eq!(permission(Autonomy::Ask), permission(Autonomy::ReadOnly));
        assert_ne!(permission(Autonomy::Low), permission(Autonomy::High));
        assert_eq!(permission(Autonomy::High), RED);
    }

    #[test]
    fn the_pulse_cycles_and_context_warns_late() {
        assert_eq!(pulse(0), pulse(10));
        assert_ne!(pulse(0), pulse(5));
        assert_eq!((context(10), context(60), context(90)), ("", YELLOW, RED));
        assert_eq!(risk_in("high risk command"), RED);
        assert_eq!(
            risk_in("medium risk command exceeds autonomy level 'low'"),
            YELLOW
        );
    }
}
