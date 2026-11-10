//! Who `send_text` means by `to`: a contact's name ("Sam", "sam r") or a phone number. Never
//! guesses between two different people: an ambiguous name comes back as a list to pick from.

use crate::map::address::normalize;
use crate::messages::Contact;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recipient {
    One {
        name: String,
        address: String,
        /// Not certain enough to send without the card. Either the name was only found because it
        /// contains what was typed ("ann" → "Joanne"), not because it is the name, starts with it,
        /// or is a number; or the name has more than one number, which is as often two people
        /// saved alike as one person with two phones, and the pick between them is a guess.
        loose: bool,
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
        return Recipient::One {
            name,
            address: number,
            loose: false,
        };
    }
    let tiers: [&dyn Fn(&str) -> bool; 3] = [&|n| n == q, &|n| word_prefix(n, &q), &|n| n.contains(&q)];
    for (tier, matches) in tiers.into_iter().enumerate() {
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
        let address = normalize(&best.address);
        let several_numbers = hits.iter().any(|c| normalize(&c.address) != address);
        return Recipient::One {
            name: best.name.trim().to_string(),
            address,
            loose: tier == 2 || several_numbers,
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
                address: "+15550100001".into(),
                loose: true,
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
                address: "+15550100002".into(),
                loose: true,
            }
        );
    }

    #[test]
    fn a_name_with_several_numbers_is_never_sent_to_without_the_card() {
        // The bug: two different people saved as "Chris Lee" look like one person with two
        // numbers, and `tug text "Chris Lee" …` with "without asking" on went to whichever was
        // texted last, with no card to show which.
        let book = vec![c("Chris Lee", "+15550100011"), c("Chris Lee", "+15550100012")];
        assert!(matches!(
            resolve("Chris Lee", &book, &[]),
            Recipient::One { loose: true, .. }
        ));
        // One number written two ways is one number.
        let same = vec![c("Chris Lee", "+15550100011"), c("Chris Lee", "+1 (555) 010-0011")];
        assert!(matches!(
            resolve("Chris Lee", &same, &[]),
            Recipient::One { loose: false, .. }
        ));
        // A name with a single number goes as before.
        assert!(matches!(
            resolve("Alex Kim", &self::book(), &[]),
            Recipient::One { loose: false, .. }
        ));
    }

    #[test]
    fn numbers_work_with_or_without_a_contact() {
        assert_eq!(
            resolve("(555) 010-0099", &book(), &[]),
            Recipient::One {
                name: "+15550100099".into(),
                address: "+15550100099".into(),
                loose: false,
            }
        );
        assert_eq!(
            resolve("+1 555 010 0004", &book(), &[]),
            Recipient::One {
                name: "Alex Kim".into(),
                address: "+15550100004".into(),
                loose: false,
            }
        );
    }

    #[test]
    fn a_name_that_only_contains_the_query_is_loose() {
        assert!(matches!(
            resolve("mantha", &book(), &[]),
            Recipient::One { ref name, loose: true, .. } if name == "Samantha Lee"
        ));
        // Starting a word of the name isn't loose ("tay" → "Taylor Smith").
        assert!(matches!(
            resolve("lee", &book(), &[]),
            Recipient::One { ref name, loose: false, .. } if name == "Samantha Lee"
        ));
    }

    #[test]
    fn nobody_matches() {
        assert_eq!(resolve("Jordan", &book(), &[]), Recipient::NoMatch);
        assert_eq!(resolve("  ", &book(), &[]), Recipient::NoMatch);
        assert_eq!(resolve("12345", &book(), &[]), Recipient::NoMatch);
    }
}
