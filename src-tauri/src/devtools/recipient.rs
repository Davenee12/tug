//! Who `send_text` means by `to`: a contact's name ("Sam", "sam r") or a phone number. Never
//! guesses between two different people: an ambiguous name comes back as a list to pick from.

use crate::map::address::normalize;
use crate::messages::Contact;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recipient {
    One {
        name: String,
        address: String,
    },
    /// Several different people match: their names, so the caller can be more specific.
    Several(Vec<String>),
    NoMatch,
}

/// A number someone typed: mostly digits, at least 7 of them (short codes need the contact).
fn as_number(q: &str) -> Option<String> {
    let digits = q.chars().filter(char::is_ascii_digit).count();
    let only_number_chars = q
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '+' | ' ' | '-' | '(' | ')' | '.'));
    (only_number_chars && digits >= 7).then(|| normalize(q))
}

fn fold(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Every word of the query starts a word of the name, in order ("sam r" → "Sam Rivera").
fn word_prefix(name: &str, q: &str) -> bool {
    let words: Vec<&str> = name.split_whitespace().collect();
    let mut i = 0;
    for part in q.split_whitespace() {
        match words[i..].iter().position(|w| w.starts_with(part)) {
            Some(p) => i += p + 1,
            None => return false,
        }
    }
    true
}

/// `contacts` may list one person several times (one row per number). `recent` is addresses
/// with texts, newest first: between one person's numbers, the one last texted wins.
pub fn resolve(query: &str, contacts: &[Contact], recent: &[String]) -> Recipient {
    let q = fold(query);
    if q.is_empty() {
        return Recipient::NoMatch;
    }
    if let Some(number) = as_number(&q) {
        let name = contacts
            .iter()
            .find(|c| normalize(&c.address) == number)
            .map_or_else(|| number.clone(), |c| c.name.clone());
        return Recipient::One { name, address: number };
    }
    let tiers: [&dyn Fn(&str) -> bool; 3] = [&|n| n == q, &|n| word_prefix(n, &q), &|n| n.contains(&q)];
    for matches in tiers {
        let hits: Vec<&Contact> = contacts.iter().filter(|c| matches(&fold(&c.name))).collect();
        if hits.is_empty() {
            continue;
        }
        let mut names: Vec<String> = hits.iter().map(|c| c.name.trim().to_string()).collect();
        names.sort_by_key(|n| n.to_lowercase());
        names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        if names.len() > 1 {
            names.truncate(5);
            return Recipient::Several(names);
        }
        let rank = |c: &&Contact| {
            let a = normalize(&c.address);
            recent.iter().position(|r| *r == a).unwrap_or(usize::MAX)
        };
        let best = hits.iter().min_by_key(|c| rank(c)).expect("non-empty");
        return Recipient::One {
            name: best.name.trim().to_string(),
            address: normalize(&best.address),
        };
    }
    Recipient::NoMatch
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(name: &str, address: &str) -> Contact {
        Contact {
            address: address.into(),
            name: name.into(),
        }
    }

    fn book() -> Vec<Contact> {
        vec![
            c("Sam Rivera", "+15550100001"),
            c("Sam Rivera", "+15550100002"),
            c("Samantha Lee", "+15550100003"),
            c("Alex Kim", "+15550100004"),
            c("Dr. Patel", "+15550100005"),
        ]
    }

    #[test]
    fn exact_name_wins_over_partial() {
        let r = resolve("sam rivera", &book(), &[]);
        assert_eq!(
            r,
            Recipient::One {
                name: "Sam Rivera".into(),
                address: "+15550100001".into()
            }
        );
    }

    #[test]
    fn ambiguous_names_are_listed_not_guessed() {
        assert_eq!(
            resolve("sam", &book(), &[]),
            Recipient::Several(vec!["Sam Rivera".into(), "Samantha Lee".into()])
        );
    }

    #[test]
    fn word_starts_narrow_it_down() {
        let r = resolve("Sam R", &book(), &[]);
        assert!(matches!(r, Recipient::One { ref name, .. } if name == "Sam Rivera"));
        let r = resolve("alex", &book(), &[]);
        assert!(matches!(r, Recipient::One { ref name, .. } if name == "Alex Kim"));
        let r = resolve("patel", &book(), &[]);
        assert!(matches!(r, Recipient::One { ref name, .. } if name == "Dr. Patel"));
    }

    #[test]
    fn the_number_last_texted_wins_for_one_person() {
        let recent = vec!["+15550100009".to_string(), "+15550100002".to_string()];
        assert_eq!(
            resolve("Sam Rivera", &book(), &recent),
            Recipient::One {
                name: "Sam Rivera".into(),
                address: "+15550100002".into()
            }
        );
    }

    #[test]
    fn numbers_work_with_or_without_a_contact() {
        assert_eq!(
            resolve("(555) 010-0099", &book(), &[]),
            Recipient::One {
                name: "+15550100099".into(),
                address: "+15550100099".into()
            }
        );
        assert_eq!(
            resolve("+1 555 010 0004", &book(), &[]),
            Recipient::One {
                name: "Alex Kim".into(),
                address: "+15550100004".into()
            }
        );
    }

    #[test]
    fn nobody_matches() {
        assert_eq!(resolve("Jordan", &book(), &[]), Recipient::NoMatch);
        assert_eq!(resolve("  ", &book(), &[]), Recipient::NoMatch);
        assert_eq!(resolve("12345", &book(), &[]), Recipient::NoMatch);
    }
}
