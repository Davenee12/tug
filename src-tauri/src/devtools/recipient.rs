//! Who `send_text` means by `to`: a contact's name ("Sam", "sam r") or a phone number. Never
//! guesses between two different people: an ambiguous name comes back as a list to pick from.

use tug_bridge::protocol::ContactName;

use crate::map::address::normalize;
use crate::messages::Contact;

/// How many people `contact_names` marks as texted lately.
pub const RECENT_PEOPLE: usize = 8;

/// Each name once (one person can have several numbers, so several rows), for picking who to
/// text: the people texted most lately first (up to `RECENT_PEOPLE`, marked `recent`), then
/// everyone else A to Z. Picking a name sends it as `to`, which `resolve` then finds exactly.
pub fn contact_names(contacts: &[Contact], recent: &[String], limit: usize) -> Vec<ContactName> {
    // (folded name, shown name, best position in `recent`)
    let mut people: Vec<(String, String, usize)> = Vec::new();
    for c in contacts {
        let shown = c.name.split_whitespace().collect::<Vec<_>>().join(" ");
        if shown.is_empty() {
            continue;
        }
        let key = shown.to_lowercase();
        let a = normalize(&c.address);
        let rank = recent.iter().position(|r| *r == a).unwrap_or(usize::MAX);
        match people.iter_mut().find(|p| p.0 == key) {
            Some(p) => p.2 = p.2.min(rank),
            None => people.push((key, shown, rank)),
        }
    }
    let mut lately: Vec<_> = people.iter().filter(|p| p.2 != usize::MAX).cloned().collect();
    lately.sort_by_key(|p| p.2);
    lately.truncate(RECENT_PEOPLE);
    let mut rest: Vec<_> = people
        .into_iter()
        .filter(|p| !lately.iter().any(|l| l.0 == p.0))
        .collect();
    rest.sort_by(|a, b| a.0.cmp(&b.0));
    lately
        .into_iter()
        .map(|p| ContactName {
            name: p.1,
            recent: true,
        })
        .chain(rest.into_iter().map(|p| ContactName {
            name: p.1,
            recent: false,
        }))
        .take(limit)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recipient {
    One {
        name: String,
        address: String,
        /// Only found because the name contains what was typed ("ann" → "Joanne"), not because
        /// it is the name, starts with it, or is a number. Such a text always gets the card.
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
        // One person saved twice ("Maya Chen", "maya  chen") is one name, as contact_names lists it.
        names.sort_by_key(|n| fold(n));
        names.dedup_by(|a, b| fold(a) == fold(b));
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
            loose: tier == 2,
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
                loose: false,
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
                loose: false,
            }
        );
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

    #[test]
    fn contact_names_list_each_person_once_lately_texted_first() {
        let c = |name: &str, address: &str| Contact {
            name: name.into(),
            address: address.into(),
        };
        let contacts = vec![
            c("Zoe Park", "+15550000001"),
            c("Maya Chen", "+15550000002"),
            c("maya  chen", "+15550000003"), // The same person, another number.
            c("Arlo Diaz", "+15550000004"),
            c("   ", "+15550000005"), // No name: nothing to pick.
        ];
        let recent = vec!["+15550000003".to_string(), "+15550000001".to_string()];
        let names = contact_names(&contacts, &recent, 100);
        let shown: Vec<(&str, bool)> = names.iter().map(|n| (n.name.as_str(), n.recent)).collect();
        assert_eq!(
            shown,
            vec![("Maya Chen", true), ("Zoe Park", true), ("Arlo Diaz", false)]
        );
        assert_eq!(contact_names(&contacts, &recent, 2).len(), 2);
        // Each name, sent back as `to`, is found exactly: no card for a loose match.
        for n in &names {
            assert!(
                matches!(
                    resolve(&n.name, &contacts, &recent),
                    Recipient::One { loose: false, .. }
                ),
                "{}",
                n.name
            );
        }
    }

    #[test]
    fn only_the_most_lately_texted_are_marked_recent() {
        let contacts: Vec<Contact> = (0..20)
            .map(|i| Contact {
                name: format!("Person {i:02}"),
                address: format!("+1555000{i:04}"),
            })
            .collect();
        let recent: Vec<String> = (0..20).rev().map(|i| format!("+1555000{i:04}")).collect();
        let names = contact_names(&contacts, &recent, 100);
        assert_eq!(names.iter().filter(|n| n.recent).count(), RECENT_PEOPLE);
        assert_eq!(names[0].name, "Person 19");
        assert_eq!(names.len(), 20);
    }
}
