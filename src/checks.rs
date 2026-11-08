//! Shallow checks that run before a request is sent. The API is the final
//! authority, so these never reject what it would accept. See `docs/api-client.md`.

use serde_json::Value;

use crate::api::{Question, SystemOneRequest};
use crate::error::Error;

/// The API's limit on the encoded request body: 32 MiB.
pub const MAX_BODY_BYTES: usize = 32 * 1024 * 1024;

pub(crate) const BODY_TOO_LARGE: &str = "request body is over the API's 32 MiB limit";

pub(crate) fn check_request(request: &SystemOneRequest) -> Result<(), Error> {
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
    for (id, question) in &request.questions {
        check_question(question)
            .map_err(|problem| failed(&format!("questions.{id}: {problem}")))?;
    }
    Ok(())
}

/// The API's limit on `choice` options.
pub const MAX_CHOICE_OPTIONS: usize = 255;

/// The API's limit on `score` levels.
pub const MAX_SCORE_LEVELS: usize = 10;

fn check_question(question: &Question) -> Result<(), String> {
    match question {
        Question::Choice { criteria, .. } if criteria.is_empty() => {
            Err("choice needs at least 1 option".to_owned())
        }
        Question::Choice { criteria, .. } if criteria.len() > MAX_CHOICE_OPTIONS => Err(format!(
            "choice allows at most {MAX_CHOICE_OPTIONS} options"
        )),
        Question::Score { criteria, .. } if criteria.is_empty() => {
            Err("score needs at least 1 level".to_owned())
        }
        Question::Score { criteria, .. } if criteria.len() > MAX_SCORE_LEVELS => {
            Err(format!("score allows at most {MAX_SCORE_LEVELS} levels"))
        }
        // `noul` criteria can only hold `true` and `false` by construction.
        Question::Choice { .. } | Question::Noul { .. } | Question::Score { .. } => Ok(()),
    }
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
    use crate::api::ChoiceCriteria;

    fn request(model: &str, state: Value, ids: &[&str]) -> SystemOneRequest {
        let question = Question::noul("Is it?".to_owned(), Default::default());
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

    fn choice_request(options: usize) -> SystemOneRequest {
        let criteria = (0..options).map(|i| (format!("o{i}"), None)).collect();
        SystemOneRequest {
            model: "m".to_owned(),
            state: json!("text"),
            questions: BTreeMap::from([(
                "team".to_owned(),
                Question::choice("Which?".to_owned(), ChoiceCriteria(criteria)),
            )]),
        }
    }

    #[rstest]
    #[case::one(1)]
    #[case::most(MAX_CHOICE_OPTIONS)]
    fn accepts_choice_option_counts_in_range(#[case] options: usize) {
        check_request(&choice_request(options)).unwrap();
    }

    #[rstest]
    #[case::none(0, "questions.team: choice needs at least 1 option")]
    #[case::too_many(
        MAX_CHOICE_OPTIONS + 1,
        "questions.team: choice allows at most 255 options"
    )]
    fn rejects_choice_option_counts_out_of_range(#[case] options: usize, #[case] message: &str) {
        let error = check_request(&choice_request(options)).unwrap_err();
        assert_eq!(error.to_string(), message);
    }

    fn score_request(levels: usize) -> SystemOneRequest {
        let criteria = (0..levels).map(|i| format!("level {i}")).collect();
        SystemOneRequest {
            model: "m".to_owned(),
            state: json!("text"),
            questions: BTreeMap::from([(
                "score".to_owned(),
                Question::score("How much?".to_owned(), criteria),
            )]),
        }
    }

    #[rstest]
    #[case::one(1)]
    #[case::most(MAX_SCORE_LEVELS)]
    fn accepts_score_level_counts_in_range(#[case] levels: usize) {
        check_request(&score_request(levels)).unwrap();
    }

    #[rstest]
    #[case::none(0, "questions.score: score needs at least 1 level")]
    #[case::too_many(MAX_SCORE_LEVELS + 1, "questions.score: score allows at most 10 levels")]
    fn rejects_score_level_counts_out_of_range(#[case] levels: usize, #[case] message: &str) {
        let error = check_request(&score_request(levels)).unwrap_err();
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
