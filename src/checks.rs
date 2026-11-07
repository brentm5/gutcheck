//! Shallow checks that run before a request is sent. The API is the final
//! authority, so these never reject what it would accept. See `docs/api-client.md`.

use serde_json::Value;

use crate::api::SystemOneRequest;
use crate::error::Error;

/// The API's limit on the encoded request body: 32 MiB.
pub const MAX_BODY_BYTES: usize = 32 * 1024 * 1024;

pub(crate) const BODY_TOO_LARGE: &str = "request body is over the API's 32 MiB limit";

pub(crate) fn check_request(request: &SystemOneRequest) -> Result<(), Error> {
    if request.model.is_empty() {
        return Err(failed("model: must not be empty"));
    }
    match &request.state {
        Value::String(_) | Value::Array(_) | Value::Object(_) => Ok(()),
        _ => Err(failed("state: must be a string, object or array")),
    }?;
    if request.questions.is_empty() {
        return Err(failed("questions: needs at least 1 question"));
    }
    if request.questions.keys().any(String::is_empty) {
        return Err(failed("questions: question IDs must not be empty"));
    }
    Ok(())
}

pub fn check_body_size(bytes: usize) -> Result<(), Error> {
    if bytes > MAX_BODY_BYTES {
        return Err(failed(BODY_TOO_LARGE));
    }
    Ok(())
}

fn failed(message: &str) -> Error {
    Error::Usage(message.to_owned())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use rstest::rstest;
    use serde_json::json;

    use super::*;
    use crate::api::Question;

    fn request(model: &str, state: Value, ids: &[&str]) -> SystemOneRequest {
        let question = Question::noul("Is it?".to_owned(), None, None);
        SystemOneRequest {
            model: model.to_owned(),
            state,
            questions: ids
                .iter()
                .map(|id| ((*id).to_owned(), question.clone()))
                .collect::<BTreeMap<_, _>>(),
        }
    }

    #[rstest]
    #[case::string(json!("text"))]
    #[case::object(json!({"subject": "text"}))]
    #[case::array(json!(["text"]))]
    #[case::empty_string(json!(""))]
    #[case::empty_object(json!({}))]
    #[case::empty_array(json!([]))]
    fn accepts_valid_requests(#[case] state: Value) {
        check_request(&request("jev-latest", state, &["id"])).unwrap();
    }

    #[rstest]
    #[case::empty_model("", json!("text"), &["id"], "model: must not be empty")]
    #[case::number("m", json!(1), &["id"], "state: must be a string, object or array")]
    #[case::null("m", Value::Null, &["id"], "state: must be a string, object or array")]
    #[case::no_questions("m", json!("text"), &[], "questions: needs at least 1 question")]
    #[case::empty_id("m", json!("text"), &[""], "questions: question IDs must not be empty")]
    fn rejects_invalid_requests(
        #[case] model: &str,
        #[case] state: Value,
        #[case] ids: &[&str],
        #[case] message: &str,
    ) {
        let error = check_request(&request(model, state, ids)).unwrap_err();
        assert_eq!(error.to_string(), message);
    }

    #[test]
    fn body_size_limit_is_inclusive() {
        check_body_size(MAX_BODY_BYTES).unwrap();
        assert_eq!(
            check_body_size(MAX_BODY_BYTES + 1).unwrap_err().to_string(),
            "request body is over the API's 32 MiB limit"
        );
    }
}
