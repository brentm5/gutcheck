//! Checks our API types and fixtures against the checked-in OpenAPI spec.
//! See `docs/testing.md`.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use gutcheck::api::{
    Answer, ChoiceCriteria, NoulCriteria, Question, SystemOneRequest, SystemOneResponse,
};
use jsonschema::Validator;
use rstest::rstest;
use serde_json::{Value, json};

/// The spec's schemas, parsed once for all the tests.
static COMPONENTS: LazyLock<Value> = LazyLock::new(|| {
    let mut spec: Value = serde_json::from_str(include_str!("../spec/openapi.json")).unwrap();
    spec["components"].take()
});

fn validator(schema: &str) -> Validator {
    jsonschema::validator_for(&json!({
        "$ref": format!("#/components/schemas/{schema}"),
        "components": *COMPONENTS,
    }))
    .unwrap()
}

fn assert_valid(schema: &str, instance: &Value) {
    let validator = validator(schema);
    let errors: Vec<String> = validator
        .iter_errors(instance)
        .map(|error| error.to_string())
        .collect();
    assert!(errors.is_empty(), "{schema}: {errors:?}");
}

#[rstest]
#[case::no_criteria(None, None)]
#[case::only_true(Some("Yes"), None)]
#[case::only_false(None, Some("No"))]
#[case::both(Some("Yes"), Some("No"))]
fn noul_requests_match_the_spec(#[case] if_true: Option<&str>, #[case] if_false: Option<&str>) {
    let request = SystemOneRequest {
        model: "jev-latest".to_owned(),
        state: json!("text"),
        questions: BTreeMap::from([(
            "noul".to_owned(),
            Question::noul(
                "Is it?".to_owned(),
                NoulCriteria {
                    if_true: if_true.map(str::to_owned),
                    if_false: if_false.map(str::to_owned),
                },
            ),
        )]),
    };

    assert_valid("SystemOneRequest", &serde_json::to_value(&request).unwrap());
}

#[rstest]
#[case::bare_keys(&[("billing", None), ("platform", None)])]
#[case::described_keys(&[("billing", Some("Payments")), ("platform", Some("Infra"))])]
#[case::mixed(&[("billing", None), ("platform", Some("Infra=outages"))])]
fn choice_requests_match_the_spec(#[case] options: &[(&str, Option<&str>)]) {
    let criteria = options
        .iter()
        .map(|(key, description)| ((*key).to_owned(), description.map(str::to_owned)))
        .collect();
    let request = SystemOneRequest {
        model: "jev-latest".to_owned(),
        state: json!("text"),
        questions: BTreeMap::from([(
            "choice".to_owned(),
            Question::choice("Which team?".to_owned(), ChoiceCriteria(criteria)),
        )]),
    };

    assert_valid("SystemOneRequest", &serde_json::to_value(&request).unwrap());
}

#[rstest]
#[case::one_level(&["Calm"])]
#[case::several_levels(&["Calm", "Annoyed", "Angry"])]
fn score_requests_match_the_spec(#[case] levels: &[&str]) {
    let request = SystemOneRequest {
        model: "jev-latest".to_owned(),
        state: json!("text"),
        questions: BTreeMap::from([(
            "score".to_owned(),
            Question::score(
                "How frustrated?".to_owned(),
                levels.iter().map(|level| (*level).to_owned()).collect(),
            ),
        )]),
    };

    assert_valid("SystemOneRequest", &serde_json::to_value(&request).unwrap());
}

#[rstest]
#[case::noul(include_str!("fixtures/noul_success.json"), "noul")]
#[case::score(include_str!("fixtures/score_success.json"), "score")]
#[case::choice(include_str!("fixtures/choice_success.json"), "choice")]
fn response_fixtures_match_the_spec_and_our_types(#[case] body: &str, #[case] id: &str) {
    assert_valid("SystemOneResponse", &serde_json::from_str(body).unwrap());
    let response: SystemOneResponse = serde_json::from_str(body).unwrap();
    let answer = &response.answers[id];
    let expected_type = match answer {
        Answer::Choice { .. } => "choice",
        Answer::Noul { .. } => "noul",
        Answer::Score { .. } => "score",
    };
    assert_eq!(expected_type, id);
}

#[rstest]
#[case::bad_request("HTTPBadRequestError", include_str!("fixtures/bad_request_400.json"))]
#[case::validation("HTTPValidationError", include_str!("fixtures/validation_error_422.json"))]
fn error_fixtures_match_the_spec(#[case] schema: &str, #[case] body: &str) {
    assert_valid(schema, &serde_json::from_str(body).unwrap());
}

/// Makes sure the schema really is applied, so the tests above can't pass vacuously.
#[test]
fn the_validator_rejects_a_bad_request() {
    assert!(!validator("SystemOneRequest").is_valid(&json!({"model": "jev-latest"})));
}
