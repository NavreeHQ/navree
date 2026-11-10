//! Duration literals in route attributes: `"500ms"`, `"5s"`, `"1m"`, `"1h"`.

/// Parses `value` into milliseconds; the error is the compile error message.
pub(crate) fn parse_millis(value: &str) -> Result<u64, String> {
    let malformed = || {
        format!(
            "invalid duration `{value}`: expected a whole number followed by `ms`, `s`, `m` or \
             `h`, e.g. \"500ms\", \"5s\", \"1m\" or \"1h\""
        )
    };
    let digits = value
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(digits);
    let millis_per_unit: u64 = match unit {
        "ms" => 1,
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        _ => return Err(malformed()),
    };
    if number.is_empty() {
        return Err(malformed());
    }
    let too_large = || format!("the duration `{value}` is too large");
    let millis = number
        .parse::<u64>()
        .map_err(|_| too_large())?
        .checked_mul(millis_per_unit)
        .ok_or_else(too_large)?;
    if millis == 0 {
        return Err(format!("the duration `{value}` must be greater than zero"));
    }
    Ok(millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_each_unit() {
        assert_eq!(parse_millis("500ms"), Ok(500));
        assert_eq!(parse_millis("5s"), Ok(5_000));
        assert_eq!(parse_millis("1m"), Ok(60_000));
        assert_eq!(parse_millis("2h"), Ok(7_200_000));
        assert_eq!(parse_millis("90s"), Ok(90_000));
    }

    #[test]
    fn rejects_malformed_values() {
        for bad in [
            "", "5", "s", "5 s", "5sec", "1.5s", "-1s", "5S", "1d", " 5s",
        ] {
            let error = parse_millis(bad).expect_err(bad);
            assert!(
                error.contains("expected a whole number followed by `ms`, `s`, `m` or `h`"),
                "{bad}: {error}"
            );
            assert!(error.contains(&format!("`{bad}`")), "{bad}: {error}");
        }
    }

    #[test]
    fn rejects_zero_and_overflow() {
        assert_eq!(
            parse_millis("0s"),
            Err("the duration `0s` must be greater than zero".to_owned())
        );
        let error = parse_millis("99999999999999999999h").expect_err("overflow");
        assert!(error.contains("too large"), "{error}");
    }
}
