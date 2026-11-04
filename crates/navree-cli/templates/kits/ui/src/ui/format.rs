//! Numbers and days the way the people who use this app read them: rupees
//! with lakh and crore grouping (`₹1,00,000`), phone numbers with spaces
//! (`+91 98765 43210`) and days as `5 Nov 2026`.

use chrono::NaiveDate;

/// `n` with Indian grouping: the last three digits, then pairs (`12,34,567`).
pub fn grouped(n: u64) -> String {
    let digits = n.to_string();
    if digits.len() <= 3 {
        return digits;
    }
    let (head, tail) = digits.split_at(digits.len() - 3);
    let mut pairs: Vec<&str> = Vec::new();
    let mut end = head.len();
    while end > 2 {
        pairs.push(&head[end - 2..end]);
        end -= 2;
    }
    pairs.push(&head[..end]);
    pairs.reverse();
    let front = pairs.join(",");
    format!("{front},{tail}")
}

/// An amount in paise as rupees: `₹1,00,000`, and `₹24,999.50` when there are paise.
pub fn rupees(paise: i64) -> String {
    let sign = if paise < 0 { "-" } else { "" };
    let paise = paise.unsigned_abs();
    let whole = grouped(paise / 100);
    let part = paise % 100;
    if part == 0 {
        format!("{sign}₹{whole}")
    } else {
        format!("{sign}₹{whole}.{part:02}")
    }
}

/// `+919876501001` as `+91 98765 01001`. Any other number is shown as stored.
pub fn phone_text(e164: &str) -> String {
    match e164.strip_prefix("+91") {
        Some(national)
            if national.len() == 10 && national.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            format!("+91 {} {}", &national[..5], &national[5..])
        }
        _ => e164.to_owned(),
    }
}

/// What someone typed as a phone number, as it is stored: the `+` and the
/// digits. Ten digits with no country code are Indian numbers (`+91`).
pub fn e164(typed: &str) -> String {
    let typed = typed.trim();
    let digits: String = typed.chars().filter(char::is_ascii_digit).collect();
    if typed.starts_with('+') {
        format!("+{digits}")
    } else if digits.len() == 10 {
        format!("+91{digits}")
    } else {
        digits
    }
}

/// A day as `5 Nov 2026`.
pub fn day(date: NaiveDate) -> String {
    date.format("%-d %b %Y").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_group_by_lakh_and_crore() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000), "1,000");
        assert_eq!(grouped(12_345), "12,345");
        assert_eq!(grouped(100_000), "1,00,000");
        assert_eq!(grouped(1_234_567), "12,34,567");
        assert_eq!(grouped(100_000_000), "10,00,00,000");
    }

    #[test]
    fn rupees_show_paise_only_when_there_are_some() {
        assert_eq!(rupees(0), "₹0");
        assert_eq!(rupees(499_900), "₹4,999");
        assert_eq!(rupees(10_000_000), "₹1,00,000");
        assert_eq!(rupees(2_499_950), "₹24,999.50");
        assert_eq!(rupees(-500), "-₹5");
    }

    #[test]
    fn phone_numbers_read_and_write() {
        assert_eq!(phone_text("+919876501001"), "+91 98765 01001");
        assert_eq!(phone_text("+441632960961"), "+441632960961");
        assert_eq!(e164("+91 98765 43210"), "+919876543210");
        assert_eq!(e164("98765-43210"), "+919876543210");
        assert_eq!(e164(" +44 1632 960961 "), "+441632960961");
    }

    #[test]
    fn days_read_like_a_calendar() {
        let day_one = NaiveDate::from_ymd_opt(2026, 11, 5).unwrap_or_default();
        assert_eq!(day(day_one), "5 Nov 2026");
    }
}
