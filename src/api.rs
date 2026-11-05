//! Serde types for the Jev API, checked against `spec/openapi.json` in tests.
//! See `docs/language-and-distribution.md`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
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
    Noul {
        instructions: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
}

impl Question {
    /// A `noul` question. `criteria` is left out if neither side is described.
    pub fn noul(instructions: String, if_true: Option<String>, if_false: Option<String>) -> Self {
        let criteria =
            (if_true.is_some() || if_false.is_some()).then_some(NoulCriteria { if_true, if_false });
        Question::Noul {
            instructions,
            criteria,
        }
    }

    /// The question's type as the API names it. A CLI request holds one question
    /// and uses this as its ID, so API errors read `questions.noul.criteria: ...`.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Question::Noul { .. } => "noul",
        }
    }
}

/// What counts as a yes and a no for a `noul` question.
#[derive(Debug, Clone, PartialEq, Serialize)]
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

/// One answer. It serializes back to what the API sent, apart from key order:
/// `Number` keeps `1` from becoming `1.0`, and `extra` keeps any fields the API
/// adds later.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Answer {
    Noul {
        noul: Number,
        #[serde(flatten)]
        extra: Map<String, Value>,
    },
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
