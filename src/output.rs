//! Formatting for plain numeric CLI output, shared by the text question types.

/// Maximum decimal places in plain output. JSON and threshold comparisons do not use it.
pub(crate) const PLAIN_DECIMAL_PLACES: usize = 3;

pub(crate) fn number_text(value: f64) -> String {
    let text = format!("{value:.PLAIN_DECIMAL_PLACES$}");
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        &text
    };
    if text == "-0" {
        "0".to_owned()
    } else {
        text.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(0.0, "0")]
    #[case(1.0, "1")]
    #[case(0.91, "0.91")]
    #[case(1.8, "1.8")]
    #[case(0.912345678, "0.912")]
    #[case(1.2346, "1.235")]
    #[case(0.0004, "0")]
    #[case(1e-8, "0")]
    #[case(-0.0, "0")]
    fn plain_numbers_are_rounded_decimals(#[case] value: f64, #[case] expected: &str) {
        assert_eq!(number_text(value), expected);
    }
}
