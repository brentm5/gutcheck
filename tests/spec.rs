//! Checks our API types and fixtures against the checked-in OpenAPI spec.
//! See `docs/testing.md`.

use std::collections::BTreeMap;

use gutcheck::Question;
use gutcheck::api::{SystemOneRequest, SystemOneResponse};
use jsonschema::Validator;
use rstest::rstest;
use serde_json::{Value, json};

const SPEC: &str = include_str!("../spec/openapi.json");

fn validator(schema: &str) -> Validator {
    let spec: Value = serde_json::from_str(SPEC).unwrap();
    jsonschema::validator_for(&json!({
        "$ref": format!("#/components/schemas/{schema}"),
        "components": spec["components"],
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
                if_true.map(str::to_owned),
                if_false.map(str::to_owned),
            ),
        )]),
    };

    assert_valid("SystemOneRequest", &serde_json::to_value(&request).unwrap());
}

#[test]
fn response_fixture_matches_the_spec_and_our_types() {
    let body = include_str!("fixtures/noul_success.json");

    assert_valid("SystemOneResponse", &serde_json::from_str(body).unwrap());
    let response: SystemOneResponse = serde_json::from_str(body).unwrap();
    assert!(response.answers.contains_key("noul"));
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
