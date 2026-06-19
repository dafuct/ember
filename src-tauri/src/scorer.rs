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

// 🦀 Heuristic: does the sender address look like an automated/no-reply mailbox?
//    `to_ascii_lowercase` copies once so matching is case-insensitive. `.contains`
//    is a substring check. "notification" also matches "notifications@".
fn is_automated_sender(from: &str) -> bool {
    let f = from.to_ascii_lowercase();
    const MARKERS: [&str; 6] = [
        "no-reply", "noreply", "no_reply", "donotreply", "do-not-reply", "mailer-daemon",
    ];
    MARKERS.iter().any(|m| f.contains(m)) || f.contains("notification")
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
