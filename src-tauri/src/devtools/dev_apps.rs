//! Which phone apps count as "developer apps" for `recent_dev_notifications`. The one place to
//! add one.
//!
//! An app matches by its iOS bundle id, or by the name tug knows it by (the App Store name in
//! the `apps` table) when the bundle id isn't listed. Bundle ids are only listed where they're
//! known for certain; the others match by name until their id has been seen in tug's Feed
//! (Copy diagnostics doesn't show ids, but `apps` in tug.db does).

pub struct DevApp {
    /// What tug calls it in results.
    pub name: &'static str,
    pub bundle_ids: &'static [&'static str],
    /// App names that match (case-insensitive, whole name or the name followed by more words:
    /// "Jira" matches "Jira Cloud by Atlassian").
    pub names: &'static [&'static str],
}

pub const DEV_APPS: &[DevApp] = &[
    // GitHub, including Actions/CI results and review requests.
    DevApp {
        name: "GitHub",
        bundle_ids: &["com.github.stormbreaker.prod"],
        names: &["GitHub"],
    },
    // iOS doesn't say which Slack notifications are DMs, so all of Slack's count.
    DevApp {
        name: "Slack",
        bundle_ids: &["com.tinyspeck.chatlyio"],
        names: &["Slack"],
    },
    DevApp {
        name: "Linear",
        bundle_ids: &[],
        names: &["Linear"],
    },
    DevApp {
        name: "Jira",
        bundle_ids: &[],
        names: &["Jira"],
    },
    DevApp {
        name: "Sentry",
        bundle_ids: &[],
        names: &["Sentry"],
    },
    DevApp {
        name: "PagerDuty",
        bundle_ids: &[],
        names: &["PagerDuty"],
    },
    DevApp {
        name: "Vercel",
        bundle_ids: &[],
        names: &["Vercel"],
    },
    DevApp {
        name: "Netlify",
        bundle_ids: &[],
        names: &["Netlify"],
    },
];

fn name_matches(app_name: &str, wanted: &str) -> bool {
    let app = app_name.trim();
    let Some(head) = app.get(..wanted.len()) else {
        return false;
    };
    head.eq_ignore_ascii_case(wanted)
        && app[wanted.len()..]
            .chars()
            .next()
            .is_none_or(|c| c == ' ' || c == ':' || c == '-' || c == '–')
}

/// The developer app this notification came from, if it's one.
pub fn dev_app(app_id: &str, app_name: Option<&str>) -> Option<&'static str> {
    DEV_APPS
        .iter()
        .find(|a| {
            a.bundle_ids.iter().any(|id| id.eq_ignore_ascii_case(app_id))
                || app_name.is_some_and(|n| a.names.iter().any(|w| name_matches(n, w)))
        })
        .map(|a| a.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_by_bundle_id() {
        assert_eq!(dev_app("com.github.stormbreaker.prod", None), Some("GitHub"));
        assert_eq!(dev_app("com.tinyspeck.chatlyio", Some("Slack")), Some("Slack"));
    }

    #[test]
    fn matches_by_app_name() {
        assert_eq!(
            dev_app("com.example.jira", Some("Jira Cloud by Atlassian")),
            Some("Jira")
        );
        assert_eq!(dev_app("x", Some("pagerduty")), Some("PagerDuty"));
        assert_eq!(dev_app("x", Some("Linear")), Some("Linear"));
        assert_eq!(dev_app("x", Some("Vercel: Deploy")), Some("Vercel"));
    }

    #[test]
    fn leaves_everything_else_out() {
        assert_eq!(dev_app("com.apple.MobileSMS", Some("Messages")), None);
        assert_eq!(dev_app("com.burbn.instagram", Some("Instagram")), None);
        // A name that merely starts with the same letters isn't the app.
        assert_eq!(dev_app("x", Some("Linearity Curve")), None);
        assert_eq!(dev_app("x", Some("GitHubber")), None);
        assert_eq!(dev_app("x", None), None);
    }

    #[test]
    fn the_list_is_well_formed() {
        for a in DEV_APPS {
            assert!(!a.names.is_empty() || !a.bundle_ids.is_empty(), "{}", a.name);
        }
    }
}
