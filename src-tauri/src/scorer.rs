
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    People,
    Notifications,
    Newsletters,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::People => "people",
            Category::Notifications => "notifications",
            Category::Newsletters => "newsletters",
        }
    }
}

pub struct MessageFeatures<'a> {
    pub label_ids: &'a [String],
    pub from_addr: &'a str,
    pub has_list_unsubscribe: bool,
    pub has_list_id: bool,
}

fn is_automated_sender(from: &str) -> bool {
    let lower = from.to_ascii_lowercase();
    let before_at = lower.split('@').next().unwrap_or(lower.as_str());
    let local = before_at
        .rsplit(['<', ' '])
        .next()
        .unwrap_or(before_at);
    const MARKERS: [&str; 7] = [
        "no-reply", "noreply", "no_reply", "donotreply", "do-not-reply",
        "mailer-daemon", "notification",
    ];
    MARKERS.iter().any(|m| local.contains(m))
}

pub fn classify(f: &MessageFeatures) -> Category {
    let has = |label: &str| f.label_ids.iter().any(|l| l == label);

    if has("CATEGORY_PROMOTIONS") || has("CATEGORY_FORUMS") || f.has_list_unsubscribe {
        return Category::Newsletters;
    }
    if has("CATEGORY_UPDATES")
        || has("CATEGORY_SOCIAL")
        || is_automated_sender(f.from_addr)
        || f.has_list_id
    {
        return Category::Notifications;
    }
    Category::People
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feat<'a>(labels: &'a [String], from: &'a str, lu: bool, li: bool) -> MessageFeatures<'a> {
        MessageFeatures { label_ids: labels, from_addr: from, has_list_unsubscribe: lu, has_list_id: li }
    }

    #[test]
    fn classifies_streams_by_precedence() {
        let cases: Vec<(Vec<&str>, &str, bool, bool, Category)> = vec![
            (vec!["CATEGORY_PROMOTIONS"], "deals@store.com", false, false, Category::Newsletters),
            (vec!["CATEGORY_FORUMS"], "list@group.com", false, false, Category::Newsletters),
            (vec![], "news@brand.com", true, false, Category::Newsletters),
            (vec!["CATEGORY_UPDATES"], "updates@app.com", false, false, Category::Notifications),
            (vec!["CATEGORY_SOCIAL"], "social@app.com", false, false, Category::Notifications),
            (vec![], "no-reply@service.com", false, false, Category::Notifications),
            (vec![], "notifications@github.com", false, false, Category::Notifications),
            (vec![], "person@notification-labs.io", false, false, Category::People),
            (vec![], "team@startup.com", false, true, Category::Notifications),
            (vec!["CATEGORY_PERSONAL"], "maya@studio.co", false, false, Category::People),
            (vec![], "maya@studio.co", false, false, Category::People),
            (vec!["CATEGORY_PROMOTIONS", "CATEGORY_UPDATES"], "x@y.com", false, false, Category::Newsletters),
            (vec![], "x@y.com", true, true, Category::Newsletters),
        ];
        for (labels, from, lu, li, expected) in cases {
            let owned: Vec<String> = labels.iter().map(|s| s.to_string()).collect();
            let got = classify(&feat(&owned, from, lu, li));
            assert_eq!(got, expected, "from={from} labels={labels:?} lu={lu} li={li}");
        }
    }

    #[test]
    fn category_as_str_gives_storage_keys() {
        assert_eq!(Category::People.as_str(), "people");
        assert_eq!(Category::Notifications.as_str(), "notifications");
        assert_eq!(Category::Newsletters.as_str(), "newsletters");
    }
}
