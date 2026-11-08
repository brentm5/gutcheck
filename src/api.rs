//! Serde types for the Jev API, checked against `spec/openapi.json` in tests.
//! See `docs/language-and-distribution.md`.

use std::collections::BTreeMap;

use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Number, Value};

/// The body of `POST /v1/systemone`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SystemOneRequest {
    pub model: String,
    pub state: Value,
    pub questions: BTreeMap<String, Question>,
}

/// A single question, tagged on `type` in JSON.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    Choice {
        instructions: String,
        criteria: ChoiceCriteria,
    },
    Noul {
        instructions: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
}

impl Question {
    /// A `choice` question.
    pub fn choice(instructions: String, criteria: ChoiceCriteria) -> Self {
        Question::Choice {
            instructions,
            criteria,
        }
    }

    /// A `noul` question. `criteria` is left out if neither side is described.
    pub fn noul(instructions: String, criteria: NoulCriteria) -> Self {
        let described = criteria.if_true.is_some() || criteria.if_false.is_some();
        Question::Noul {
            instructions,
            criteria: described.then_some(criteria),
        }
    }

    /// The question's type as the API names it. A CLI request holds one question
    /// and uses this as its ID, so API errors read `questions.noul.criteria: ...`.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Question::Choice { .. } => "choice",
            Question::Noul { .. } => "noul",
        }
    }
}

/// The options of a `choice` question, in the order given. Each key maps to its
/// description, or to `null` when the option is interpreted by its name alone.
/// Keys should be unique; the CLI rejects duplicates before building this.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChoiceCriteria(pub Vec<(String, Option<String>)>);

impl ChoiceCriteria {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Serialize for ChoiceCriteria {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, description) in &self.0 {
            map.serialize_entry(key, description)?;
        }
        map.end()
    }
}

/// What counts as a yes and a no for a `noul` question.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct NoulCriteria {
    #[serde(rename = "true", skip_serializing_if = "Option::is_none")]
    pub if_true: Option<String>,
    #[serde(rename = "false", skip_serializing_if = "Option::is_none")]
    pub if_false: Option<String>,
}

/// The parts of a `POST /v1/systemone` response that `gutcheck` uses.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SystemOneResponse {
    pub answers: BTreeMap<String, Answer>,
}

/// One answer. It serializes back to the same fields and values the API sent,
/// though key order and number formatting can differ (`0.910` becomes `0.91`).
/// `Number` keeps `1` from becoming `1.0`, and `extra` keeps the fields
/// `gutcheck` doesn't read, such as `probabilities` and `confidence`, along with
/// any the API adds later.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Answer {
    Choice {
        choice: String,
        #[serde(flatten)]
        extra: Map<String, Value>,
    },
    Noul {
        noul: Number,
        #[serde(flatten)]
        extra: Map<String, Value>,
    },
}

impl Answer {
    /// The answer's type as the API names it, matching [`Question::kind`].
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Answer::Choice { .. } => "choice",
            Answer::Noul { .. } => "noul",
        }
    }

    /// A `choice` answer's selected key.
    pub fn choice(&self) -> Option<&str> {
        match self {
            Answer::Choice { choice, .. } => Some(choice),
            Answer::Noul { .. } => None,
        }
    }

    /// A `noul` answer's probability. A `Number` always converts, because
    /// `arbitrary_precision` is off and JSON can't hold a non-finite float.
    pub fn probability(&self) -> Option<f64> {
        match self {
            Answer::Noul { noul, .. } => Some(noul.as_f64().unwrap_or(f64::NAN)),
            Answer::Choice { .. } => None,
        }
    }
}

/// The body of a `400` response.
#[derive(Debug, Deserialize)]
pub(crate) struct BadRequestBody {
    pub detail: Value,
}

/// The body of a `422` response.
#[derive(Debug, Deserialize)]
pub(crate) struct ValidationBody {
    pub detail: Vec<ValidationEntry>,
}

/// One failed rule in a `422` response. `loc` holds field names and array indices.
#[derive(Debug, Deserialize)]
pub(crate) struct ValidationEntry {
    pub loc: Vec<Value>,
    pub msg: String,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    /// `noul` criteria hold only `true` and/or `false` keys, and are left out
    /// when neither is described. See `docs/api-client.md`.
    #[rstest]
    #[case::neither(None, None, json!({"type": "noul", "instructions": "Is it?"}))]
    #[case::only_true(Some("Yes"), None, json!({"type": "noul", "instructions": "Is it?", "criteria": {"true": "Yes"}}))]
    #[case::only_false(None, Some("No"), json!({"type": "noul", "instructions": "Is it?", "criteria": {"false": "No"}}))]
    #[case::both(Some("Yes"), Some("No"), json!({"type": "noul", "instructions": "Is it?", "criteria": {"true": "Yes", "false": "No"}}))]
    fn noul_criteria_have_only_true_and_false_keys(
        #[case] if_true: Option<&str>,
        #[case] if_false: Option<&str>,
        #[case] expected: Value,
    ) {
        let question = Question::noul(
            "Is it?".to_owned(),
            NoulCriteria {
                if_true: if_true.map(str::to_owned),
                if_false: if_false.map(str::to_owned),
            },
        );
        assert_eq!(serde_json::to_value(&question).unwrap(), expected);
    }

    /// A bare option key is sent with a `null` description, and options keep
    /// the order they were given in.
    #[test]
    fn choice_criteria_keep_order_and_send_bare_keys_as_null() {
        let question = Question::choice(
            "Which team?".to_owned(),
            ChoiceCriteria(vec![
                ("platform".to_owned(), None),
                ("billing".to_owned(), Some("Payments".to_owned())),
            ]),
        );
        assert_eq!(
            serde_json::to_string(&question).unwrap(),
            r#"{"type":"choice","instructions":"Which team?","criteria":{"platform":null,"billing":"Payments"}}"#
        );
    }

    #[test]
    fn answers_read_only_their_own_type() {
        let choice: Answer =
            serde_json::from_value(json!({"type": "choice", "choice": "platform"})).unwrap();
        assert_eq!(choice.choice(), Some("platform"));
        assert_eq!(choice.probability(), None);

        let noul: Answer = serde_json::from_value(json!({"type": "noul", "noul": 0.5})).unwrap();
        assert_eq!(noul.probability(), Some(0.5));
        assert_eq!(noul.choice(), None);
    }
}
