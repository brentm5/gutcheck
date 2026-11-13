//! Serde types for the Jev API, checked against `spec/openapi.json` in tests.
//! See `docs/language-and-distribution.md`.

use std::collections::BTreeMap;

use serde::de::{MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Number, Value};

use crate::error::Error;

/// The body of `POST /v1/systemone`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SystemOneRequest {
    pub model: String,
    pub state: Value,
    pub questions: BTreeMap<String, Question>,
}

/// A request as a caller writes it: the body of `POST /v1/systemone`, where
/// `model` may be left out to use the configured one. The MCP tool reads it from
/// JSON with [`AskRequest::from_json`].
#[derive(Debug, Clone, PartialEq)]
pub struct AskRequest {
    pub model: Option<String>,
    pub state: Value,
    pub questions: BTreeMap<String, Question>,
}

impl AskRequest {
    /// Reads a request from its JSON fields. A missing `state` is left for the
    /// local checks to report; anything else that has the wrong shape is
    /// reported here, naming the field.
    pub fn from_json(mut fields: Map<String, Value>) -> Result<Self, Error> {
        let model = match fields.remove("model") {
            None | Some(Value::Null) => None,
            Some(Value::String(model)) => Some(model),
            Some(_) => return Err(Error::Usage("model: must be a string".to_owned())),
        };
        let state = fields.remove("state").unwrap_or(Value::Null);
        let questions = match fields.remove("questions") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(questions)) => questions,
            Some(_) => {
                return Err(Error::Usage(
                    "questions: must be an object keyed by question ID".to_owned(),
                ));
            }
        };
        let questions = questions
            .into_iter()
            .map(|(id, question)| {
                if !question.is_object() {
                    return Err(Error::Usage(format!("questions.{id}: must be an object")));
                }
                let question = serde_json::from_value(question)
                    .map_err(|e| Error::Usage(format!("questions.{id}: {e}")))?;
                Ok((id, question))
            })
            .collect::<Result<_, Error>>()?;
        Ok(Self {
            model,
            state,
            questions,
        })
    }
}

/// A single question, tagged on `type` in JSON. Text fields take any JSON
/// value, as the API does (a string, object or array), and a `null` one is sent
/// as left out. A question of a type `gutcheck` doesn't know, or one whose
/// fields don't fit these types, is kept as raw JSON in [`Question::Other`] and
/// sent as is, for the API to judge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    Choice {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instructions: Option<Value>,
        #[serde(default)]
        criteria: ChoiceCriteria,
        /// Fields `gutcheck` doesn't read, sent back unchanged.
        #[serde(flatten)]
        extra: Map<String, Value>,
    },
    Noul {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instructions: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
        #[serde(flatten)]
        extra: Map<String, Value>,
    },
    Score {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instructions: Option<Value>,
        /// The level descriptions, lowest first. A level's position is its score.
        #[serde(default)]
        criteria: Vec<Value>,
        #[serde(flatten)]
        extra: Map<String, Value>,
    },
    /// Any other question, including its `type`, unchanged.
    #[serde(untagged)]
    Other(Map<String, Value>),
}

impl Question {
    /// A `choice` question.
    pub fn choice(instructions: String, criteria: ChoiceCriteria) -> Self {
        Question::Choice {
            instructions: Some(instructions.into()),
            criteria,
            extra: Map::new(),
        }
    }

    /// A `noul` question. `criteria` is left out if neither side is described.
    pub fn noul(instructions: String, criteria: NoulCriteria) -> Self {
        let described = criteria.if_true.is_some() || criteria.if_false.is_some();
        Question::Noul {
            instructions: Some(instructions.into()),
            criteria: described.then_some(criteria),
            extra: Map::new(),
        }
    }

    /// A `score` question. `levels` are in rubric order: the first is `0`.
    pub fn score(instructions: String, levels: Vec<String>) -> Self {
        Question::Score {
            instructions: Some(instructions.into()),
            criteria: levels.into_iter().map(Value::from).collect(),
            extra: Map::new(),
        }
    }

    /// The question's type as the API names it, or `None` for [`Question::Other`].
    /// A CLI request holds one question and uses this as its ID, so API errors
    /// read `questions.noul.criteria: ...`.
    pub(crate) fn kind(&self) -> Option<&'static str> {
        match self {
            Question::Choice { .. } => Some("choice"),
            Question::Noul { .. } => Some("noul"),
            Question::Score { .. } => Some("score"),
            Question::Other(_) => None,
        }
    }
}

/// The options of a `choice` question, in the order given. Each key maps to its
/// description, or to `null` when the option is interpreted by its name alone.
/// Keys should be unique; the CLI rejects duplicates before building this.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChoiceCriteria(pub Vec<(String, Value)>);

impl ChoiceCriteria {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<'de> Deserialize<'de> for ChoiceCriteria {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Options;

        impl<'de> Visitor<'de> for Options {
            type Value = ChoiceCriteria;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an object of option keys")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut options = Vec::with_capacity(map.size_hint().unwrap_or(0));
                while let Some(option) = map.next_entry()? {
                    options.push(option);
                }
                Ok(ChoiceCriteria(options))
            }
        }

        deserializer.deserialize_map(Options)
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

/// What counts as a yes and a no for a `noul` question. Any other key makes
/// the question [`Question::Other`], which the local checks reject.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoulCriteria {
    #[serde(rename = "true", default, skip_serializing_if = "Option::is_none")]
    pub if_true: Option<Value>,
    #[serde(rename = "false", default, skip_serializing_if = "Option::is_none")]
    pub if_false: Option<Value>,
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
    Score {
        score: Number,
        #[serde(flatten)]
        extra: Map<String, Value>,
    },
    /// An answer to a question of another type, including its `type`, unchanged.
    #[serde(untagged)]
    Other(Map<String, Value>),
}

impl Answer {
    /// The answer's type as the API names it, matching [`Question::kind`].
    pub(crate) fn kind(&self) -> Option<&'static str> {
        match self {
            Answer::Choice { .. } => Some("choice"),
            Answer::Noul { .. } => Some("noul"),
            Answer::Score { .. } => Some("score"),
            Answer::Other(_) => None,
        }
    }

    /// A `choice` answer's selected key.
    pub fn choice(&self) -> Option<&str> {
        match self {
            Answer::Choice { choice, .. } => Some(choice),
            Answer::Noul { .. } | Answer::Score { .. } | Answer::Other(_) => None,
        }
    }

    /// A `noul` answer's probability.
    pub fn probability(&self) -> Option<f64> {
        match self {
            Answer::Noul { noul, .. } => Some(number(noul)),
            Answer::Choice { .. } | Answer::Score { .. } | Answer::Other(_) => None,
        }
    }

    /// A `score` answer's probability-weighted score.
    pub fn score(&self) -> Option<f64> {
        match self {
            Answer::Score { score, .. } => Some(number(score)),
            Answer::Choice { .. } | Answer::Noul { .. } | Answer::Other(_) => None,
        }
    }
}

/// A `Number` always converts, because `arbitrary_precision` is off and JSON
/// can't hold a non-finite float.
fn number(value: &Number) -> f64 {
    value.as_f64().unwrap_or(f64::NAN)
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
                if_true: if_true.map(Value::from),
                if_false: if_false.map(Value::from),
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
                ("platform".to_owned(), Value::Null),
                ("billing".to_owned(), json!("Payments")),
            ]),
        );
        assert_eq!(
            serde_json::to_string(&question).unwrap(),
            r#"{"type":"choice","instructions":"Which team?","criteria":{"platform":null,"billing":"Payments"}}"#
        );
    }

    /// Levels are sent as an array in the order given, so position is the score.
    #[test]
    fn score_criteria_keep_order() {
        let question = Question::score(
            "How frustrated?".to_owned(),
            vec![
                "Calm".to_owned(),
                "2 Annoyed".to_owned(),
                "Angry".to_owned(),
            ],
        );
        assert_eq!(
            serde_json::to_string(&question).unwrap(),
            r#"{"type":"score","instructions":"How frustrated?","criteria":["Calm","2 Annoyed","Angry"]}"#
        );
    }

    /// Known types take any JSON value where the API does, keep fields
    /// `gutcheck` doesn't read, and stay typed so the local checks apply. `noul`
    /// criteria with another key don't fit.
    #[rstest]
    #[case::choice(json!({"type": "choice", "instructions": ["a"], "criteria": {"x": {"y": 1}, "z": null}}), Some("choice"))]
    #[case::score_without_instructions(json!({"type": "score", "criteria": [{"level": 0}]}), Some("score"))]
    #[case::noul(json!({"type": "noul", "instructions": {"q": "?"}, "criteria": {"false": ["no"]}}), Some("noul"))]
    #[case::choice_extra_fields(json!({"type": "choice", "criteria": {"x": "y"}, "assets": [1], "temperature": 0.2}), Some("choice"))]
    #[case::score_extra_fields(json!({"type": "score", "criteria": ["a"], "future": {"k": null}}), Some("score"))]
    #[case::noul_extra_fields(json!({"type": "noul", "instructions": "?", "future": true}), Some("noul"))]
    #[case::noul_other_key(json!({"type": "noul", "criteria": {"maybe": "?"}}), None)]
    #[case::unknown(json!({"type": "box", "instructions": "logo"}), None)]
    fn known_types_take_any_json_text(#[case] question: Value, #[case] kind: Option<&str>) {
        let parsed: Question = serde_json::from_value(question.clone()).unwrap();
        assert_eq!(parsed.kind(), kind);
        assert_eq!(serde_json::to_value(&parsed).unwrap(), question);
    }

    #[test]
    fn answers_read_only_their_own_type() {
        let choice: Answer =
            serde_json::from_value(json!({"type": "choice", "choice": "platform"})).unwrap();
        assert_eq!(choice.choice(), Some("platform"));
        assert_eq!(choice.probability(), None);
        assert_eq!(choice.score(), None);

        let noul: Answer = serde_json::from_value(json!({"type": "noul", "noul": 0.5})).unwrap();
        assert_eq!(noul.probability(), Some(0.5));
        assert_eq!(noul.choice(), None);
        assert_eq!(noul.score(), None);

        let score: Answer = serde_json::from_value(json!({"type": "score", "score": 1.8})).unwrap();
        assert_eq!(score.score(), Some(1.8));
        assert_eq!(score.probability(), None);
        assert_eq!(score.choice(), None);
    }
}
