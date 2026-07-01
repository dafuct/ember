use ember_lib::people::PeopleClient;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test(flavor = "multi_thread")]
async fn directory_search_parses_people() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/people:searchDirectoryPeople"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "people": [{
                "names": [{ "displayName": "Anna Melnyk" }],
                "emailAddresses": [{ "value": "anna@company.com" }],
                "photos": [{ "url": "https://img/anna.jpg" }]
            }]
        })))
        .mount(&server)
        .await;

    let client = PeopleClient::with_base_url("tok".into(), server.uri());
    let hits = client.search_directory("ann").await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].name, "Anna Melnyk");
    assert_eq!(hits[0].email, "anna@company.com");
    assert_eq!(hits[0].photo_url.as_deref(), Some("https://img/anna.jpg"));
}

#[tokio::test(flavor = "multi_thread")]
async fn search_merges_directory_and_contacts_and_dedupes() {
    let server = MockServer::start().await;
    // Directory returns anna.
    Mock::given(method("GET"))
        .and(path("/v1/people:searchDirectoryPeople"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "people": [{ "names": [{ "displayName": "Anna" }], "emailAddresses": [{ "value": "anna@company.com" }] }]
        })))
        .mount(&server)
        .await;
    // Contacts returns anna (dup) + bob.
    Mock::given(method("GET"))
        .and(path("/v1/people:searchContacts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [
                { "person": { "names": [{ "displayName": "Anna Dup" }], "emailAddresses": [{ "value": "ANNA@company.com" }] } },
                { "person": { "names": [{ "displayName": "Bob" }], "emailAddresses": [{ "value": "bob@x.com" }] } }
            ]
        })))
        .mount(&server)
        .await;
    // otherContacts returns nothing useful.
    Mock::given(method("GET"))
        .and(path("/v1/otherContacts:search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
        .mount(&server)
        .await;

    let client = PeopleClient::with_base_url("tok".into(), server.uri());
    let hits = client.search("a").await;
    assert_eq!(hits.len(), 2, "anna should be deduped case-insensitively");
    assert_eq!(hits[0].email, "anna@company.com");
    assert!(hits.iter().any(|h| h.email == "bob@x.com"));
}

#[tokio::test(flavor = "multi_thread")]
async fn search_swallows_directory_403() {
    let server = MockServer::start().await;
    // Directory denied (personal @gmail).
    Mock::given(method("GET"))
        .and(path("/v1/people:searchDirectoryPeople"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({ "error": { "message": "denied" } })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/people:searchContacts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "person": { "names": [{ "displayName": "Bob" }], "emailAddresses": [{ "value": "bob@x.com" }] } }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/otherContacts:search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
        .mount(&server)
        .await;

    let client = PeopleClient::with_base_url("tok".into(), server.uri());
    let hits = client.search("b").await;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].email, "bob@x.com");
}
