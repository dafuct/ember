//! Smart-inbox scorer — classifies one message into People / Notifications / Newsletters.
//! Pure: no network, no DB. The single source of truth for Ember's stream classification.

// 🦀 An `enum` is a type that is exactly one of a fixed set of variants. `derive`
//    auto-implements traits: `Copy` makes it cheap to pass by value (no move),
//    `PartialEq`/`Eq` enable `==` and `assert_eq!`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    People,
    Notifications,
    Newsletters,
}

impl Category {
    // 🦀 `self` by value is fine because `Category` is `Copy`. Returns a
    //    `&'static str` — a string slice baked into the binary, valid forever.
    //    These are the keys persisted in the DB and sent to the UI.
    pub fn as_str(self) -> &'static str {
        match self {
            Category::People => "people",
            Category::Notifications => "notifications",
            Category::Newsletters => "newsletters",
        }
    }
}

// 🦀 The inputs the classifier reads. It borrows rather than owns: `&'a [String]`
//    is a slice (a view into someone else's Vec) and `&'a str` a string slice.
//    The lifetime `'a` says "these borrows must outlive this struct" — no copying.
pub struct MessageFeatures<'a> {
    pub label_ids: &'a [String],
    pub from_addr: &'a str,
    pub has_list_unsubscribe: bool,
    pub has_list_id: bool,
}

// 🦀 Does the sender look like an automated/no-reply mailbox? We match on the
//    address LOCAL-PART (the text before '@'), not the whole header, so a real
//    person at e.g. "person@notification-labs.io" is NOT misread as automated.
fn is_automated_sender(from: &str) -> bool {
    let lower = from.to_ascii_lowercase();
    // 🦀 `split('@').next()` takes everything before the first '@'. A From header
    //    can be "Name <local@domain>", so we then keep only the part after the
    //    last '<' or space — that leaves just the mailbox name. `unwrap_or` gives
    //    a sane fallback if the address is malformed (no '@').
    let before_at = lower.split('@').next().unwrap_or(lower.as_str());
    let local = before_at
        .rsplit(['<', ' '])
        .next()
        .unwrap_or(before_at);
    const MARKERS: [&str; 7] = [
        "no-reply", "noreply", "no_reply", "donotreply", "do-not-reply",
        "mailer-daemon", "notification",
    ];
    // 🦀 `.any(|m| ...)` returns true if ANY marker is a substring of the local-part.
    MARKERS.iter().any(|m| local.contains(m))
}

/// Classify a message into exactly one stream. Ordered precedence: the first rule
/// that matches wins, so Newsletters outranks Notifications outranks People (default).
pub fn classify(f: &MessageFeatures) -> Category {
    // 🦀 A closure capturing `f` by reference; `has("X")` asks "is label X present?".
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

    // 🦀 Test helper: builds a `MessageFeatures` borrowing the passed-in slices/strs.
    //    The lifetime `'a` ties the returned struct's borrows to the caller's data.
    fn feat<'a>(labels: &'a [String], from: &'a str, lu: bool, li: bool) -> MessageFeatures<'a> {
        MessageFeatures { label_ids: labels, from_addr: from, has_list_unsubscribe: lu, has_list_id: li }
    }

    #[test]
    fn classifies_streams_by_precedence() {
        // (labels, from, has_list_unsubscribe, has_list_id, expected)
        let cases: Vec<(Vec<&str>, &str, bool, bool, Category)> = vec![
            (vec!["CATEGORY_PROMOTIONS"], "deals@store.com", false, false, Category::Newsletters),
            (vec!["CATEGORY_FORUMS"], "list@group.com", false, false, Category::Newsletters),
            (vec![], "news@brand.com", true, false, Category::Newsletters), // List-Unsubscribe
            (vec!["CATEGORY_UPDATES"], "updates@app.com", false, false, Category::Notifications),
            (vec!["CATEGORY_SOCIAL"], "social@app.com", false, false, Category::Notifications),
            (vec![], "no-reply@service.com", false, false, Category::Notifications), // automated
            (vec![], "notifications@github.com", false, false, Category::Notifications),
            // domain contains "notification" but the local-part doesn't → People (regression)
            (vec![], "person@notification-labs.io", false, false, Category::People),
            (vec![], "team@startup.com", false, true, Category::Notifications), // List-Id
            (vec!["CATEGORY_PERSONAL"], "maya@studio.co", false, false, Category::People),
            (vec![], "maya@studio.co", false, false, Category::People), // no labels → People
            // precedence: Newsletters rule beats Notifications rule
            (vec!["CATEGORY_PROMOTIONS", "CATEGORY_UPDATES"], "x@y.com", false, false, Category::Newsletters),
            // precedence: List-Unsubscribe (Newsletters) beats List-Id (Notifications)
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
