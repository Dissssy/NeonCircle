// google gemini api access, this is the equivalent python code

// """
// Install the Google AI Python SDK

// $ pip install google-generativeai

// See the getting started guide for more information:
// https://ai.google.dev/gemini-api/docs/get-started/python
// """

// import os

// import google.generativeai as genai

// genai.configure(api_key=os.environ["GEMINI_API_KEY"])

// # Create the model
// # See https://ai.google.dev/api/python/google/generativeai/GenerativeModel
// generation_config = {
//   "temperature": 1,
//   "top_p": 0.95,
//   "top_k": 64,
//   "max_output_tokens": 8192,
//   "stop_sequences": [
//     "\n",
//   ],
//   "response_mime_type": "text/plain",
// }

// model = genai.GenerativeModel(
//   model_name="gemini-1.5-pro",
//   generation_config=generation_config,
//   # safety_settings = Adjust safety settings
//   # See https://ai.google.dev/gemini-api/docs/safety-settings
//   system_instruction="Always give a one sentence answer.",
// )

// chat_session = model.start_chat(
//   history=[
//     {
//       "role": "user",
//       "parts": [
//         "What is the rust programming language",
//       ],
//     },
//     {
//       "role": "model",
//       "parts": [
//         "Rust is a multi-paradigm programming language designed for performance and safety, especially safe concurrency. \n",
//       ],
//     },
//     {
//       "role": "user",
//       "parts": [
//         "do you like rust?",
//       ],
//     },
//     {
//       "role": "model",
//       "parts": [
//         "As an AI, I don't have personal preferences, but Rust is a well-respected programming language for its features. \n",
//       ],
//     },
//     {
//       "role": "user",
//       "parts": [
//         "what about Go?",
//       ],
//     },
//     {
//       "role": "model",
//       "parts": [
//         "Go is favored for its simplicity, making it suitable for building efficient and scalable applications.\n",
//       ],
//     },
//   ]
// )

// response = chat_session.send_message("INSERT_INPUT_HERE")

// print(response.text)

// i am going to build an api around easily accessing the gemini api, this will include keeping track of and (with the result of the request) passing back the history if the user of this api wants to utilize that.
// otherwise everything else will be basically identical, i just need to figure out how to do this in rust lol

// here's an example curl request to the gemini api

// curl https://generativelanguage.googleapis.com/v1/models/gemini-1.5-flash:generateContent?key=$API_KEY \
//     -H 'Content-Type: application/json' \
//     -X POST \
//     -d '{ "contents":[
//       { "parts":[{"text": "Write a story about a magic backpack"}]}
//     ]
// }'

use common::{anyhow::Result, log};

// example response

// {
//   "candidates": [
//     {
//       "content": {
//         "parts": [
//           {
//             "text": "The sky itself doesn't have a color in the same way that an object does. The blue we see is actually caused by a phenomenon called **Rayleigh scattering**. "
//           }
//         ],
//         "role": "model"
//       },
//       "finishReason": "STOP",
//       "index": 0,
//       "safetyRatings": [
//         {
//           "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT",
//           "probability": "NEGLIGIBLE"
//         },
//         {
//           "category": "HARM_CATEGORY_HATE_SPEECH",
//           "probability": "NEGLIGIBLE"
//         },
//         {
//           "category": "HARM_CATEGORY_HARASSMENT",
//           "probability": "NEGLIGIBLE"
//         },
//         {
//           "category": "HARM_CATEGORY_DANGEROUS_CONTENT",
//           "probability": "NEGLIGIBLE"
//         }
//       ]
//     }
//   ],
//   "usageMetadata": {
//     "promptTokenCount": 8,
//     "candidatesTokenCount": 34,
//     "totalTokenCount": 42
//   }
// }

#[derive(Debug, serde::Deserialize)]
pub struct Response {
    candidates: Vec<ResponseCandidate>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: UsageMetadata,
}

#[derive(Debug, serde::Deserialize)]
pub struct ResponseCandidate {
    content: Content,
    #[serde(rename = "finishReason", default)]
    finish_reason: FinishReason,
    index: i64,
    #[serde(rename = "safetyRatings", default)]
    safety_ratings: Vec<SafetyRating>,
}

#[derive(Debug, serde::Deserialize, Default)]
pub enum FinishReason {
    /// Default value. This value is unused.
    #[serde(rename = "FINISH_REASON_UNSPECIFIED")]
    #[default]
    Unspecified,
    /// Natural stop point of the model or provided stop sequence.
    #[serde(rename = "STOP")]
    Stop,
    /// The maximum number of tokens as specified in the request was reached.
    #[serde(rename = "MAX_TOKENS")]
    MaxTokens,
    /// The candidate content was flagged for safety reasons.
    #[serde(rename = "SAFETY")]
    Safety,
    /// The candidate content was flagged for recitation reasons.
    #[serde(rename = "RECITATION")]
    Recitation,
    /// Unknown reason.
    #[serde(rename = "OTHER")]
    Other,
}

// {
//   "category": enum (HarmCategory),
//   "probability": enum (HarmProbability),
//   "blocked": boolean
// }

#[derive(Debug, serde::Deserialize)]
pub struct SafetyRating {
    category: HarmCategory,
    #[serde(default)]
    probability: HarmProbability,
    #[serde(default)]
    blocked: bool,
}

#[derive(Debug, serde::Deserialize, Default)]
pub enum HarmProbability {
    /// Probability is unspecified.
    #[serde(rename = "HARM_PROBABILITY_UNSPECIFIED")]
    #[default]
    Unspecified,
    /// Content has a negligible chance of being unsafe.
    #[serde(rename = "NEGLIGIBLE")]
    Negligible,
    /// Content has a low chance of being unsafe.
    #[serde(rename = "LOW")]
    Low,
    /// Content has a medium chance of being unsafe.
    #[serde(rename = "MEDIUM")]
    Medium,
    /// Content has a high chance of being unsafe.
    #[serde(rename = "HIGH")]
    High,
}

#[derive(Debug, serde::Deserialize)]
pub struct UsageMetadata {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: i64,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: i64,
    #[serde(rename = "totalTokenCount")]
    total_token_count: i64,
}

impl Response {
    pub async fn get(query: &str) -> Result<Self> {
        // let json = serde_json::to_string(&Request::new(query))?;
        // println!("{:#?}", json);
        // return Err(anyhow!("test"));

        let response = common::WEB_CLIENT
            .post(
                format!(
                    "https://generativelanguage.\
                googleapis.com/v1/models/\
                gemini-1.5-pro:generateContent\
                ?key={}",
                    common::get_config().google_gemini_api_key
                )
                .as_str(),
            )
            .json(&Request::new(query))
            .send()
            .await?;

        let delayed_json = response.text().await?;

        // Ok(Self {
        //     body: serde_json::from_str(&delayed_json).inspect_err(|e| {
        //         log::error!("Failed to parse json: {} from:\n{}", e, delayed_json);
        //     })?,
        // })
        Ok(serde_json::from_str(&delayed_json).inspect_err(|e| {
            log::error!("Failed to parse json: {} from:\n{}", e, delayed_json);
        })?)
    }
    pub fn formatted_response(self) -> String {
        self.candidates
            .into_iter()
            .map(|candidate| {
                candidate
                    .content
                    .parts
                    .into_iter()
                    .map(|part| part.text)
                    .collect::<String>()
            })
            .collect::<Vec<String>>()
            .join("\n")
    }
}

#[derive(Debug, serde::Serialize, Clone, PartialEq, PartialOrd)]
pub struct Request {
    contents: Vec<Content>,
    #[serde(rename = "safetySettings")]
    safety_settings: Vec<SafetySetting>,
    #[serde(rename = "generationConfig")]
    generation_config: GenerationConfig,
}

impl Request {
    pub fn new(query: &str) -> Self {
        Self {
            contents: vec![Content {
                parts: vec![Part {
                    text: format!("{}\nOne sentence response only please.", query),
                }],
            }],
            safety_settings: SafetySetting::all(HarmBlockThreshold::None),
            generation_config: GenerationConfig {
                stop_sequences: vec!["\n".to_string()],
                // candidate_count: 1,
                max_output_tokens: 8192,
                // temperature: 1.0,
                // top_p: 0.95,
                // top_k: 64,
            },
        }
    }
}

#[derive(
    Debug, serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
pub struct Content {
    parts: Vec<Part>,
}

#[derive(
    Debug, serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
pub struct Part {
    text: String,
}

#[derive(Debug, serde::Serialize, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SafetySetting {
    category: HarmCategory,
    threshold: HarmBlockThreshold,
}

impl SafetySetting {
    pub fn all(threshold: HarmBlockThreshold) -> Vec<Self> {
        vec![
            // Self {
            //     category: HarmCategory::Derogatory,
            //     threshold,
            // },
            // Self {
            //     category: HarmCategory::Toxicity,
            //     threshold,
            // },
            // Self {
            //     category: HarmCategory::Violence,
            //     threshold,
            // },
            // Self {
            //     category: HarmCategory::Sexual,
            //     threshold,
            // },
            // Self {
            //     category: HarmCategory::Medical,
            //     threshold,
            // },
            // Self {
            //     category: HarmCategory::Dangerous,
            //     threshold,
            // },
            Self {
                category: HarmCategory::Harassment,
                threshold,
            },
            Self {
                category: HarmCategory::HateSpeech,
                threshold,
            },
            Self {
                category: HarmCategory::SexuallyExplicit,
                threshold,
            },
            Self {
                category: HarmCategory::DangerousContent,
                threshold,
            },
        ]
    }
}

// HarmCategory.HARM_CATEGORY_HATE_SPEECH, HarmCategory.HARM_CATEGORY_SEXUALLY_EXPLICIT, HarmCategory.HARM_CATEGORY_DANGEROUS_CONTENT, HarmCategory.HARM_CATEGORY_HARASSMENT

#[derive(
    Debug, serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
pub enum HarmCategory {
    // /// Category is unspecified.
    // #[serde(rename = "HARM_CATEGORY_UNSPECIFIED")]
    // Unspecified,
    // /// Negative or harmful comments targeting identity and/or protected attribute.
    // #[serde(rename = "HARM_CATEGORY_DEROGATORY")]
    // Derogatory,
    // /// Content that is rude, disrespectful, or profane.
    // #[serde(rename = "HARM_CATEGORY_TOXICITY")]
    // Toxicity,
    // /// Describes scenarios depicting violence against an individual or group, or general descriptions of gore.
    // #[serde(rename = "HARM_CATEGORY_VIOLENCE")]
    // Violence,
    // /// Contains references to sexual acts or other lewd content.
    // #[serde(rename = "HARM_CATEGORY_SEXUAL")]
    // Sexual,
    // /// Promotes unchecked medical advice.
    // #[serde(rename = "HARM_CATEGORY_MEDICAL")]
    // Medical,
    // /// Dangerous content that promotes, facilitates, or encourages harmful acts.
    // #[serde(rename = "HARM_CATEGORY_DANGEROUS")]
    // Dangerous,
    /// Harasment content.
    #[serde(rename = "HARM_CATEGORY_HARASSMENT")]
    Harassment,
    /// Hate speech and content.
    #[serde(rename = "HARM_CATEGORY_HATE_SPEECH")]
    HateSpeech,
    /// Sexually explicit content.
    #[serde(rename = "HARM_CATEGORY_SEXUALLY_EXPLICIT")]
    SexuallyExplicit,
    /// Dangerous content.
    #[serde(rename = "HARM_CATEGORY_DANGEROUS_CONTENT")]
    DangerousContent,
}

#[derive(Debug, serde::Serialize, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum HarmBlockThreshold {
    /// Threshold is unspecified.
    #[serde(rename = "HARM_BLOCK_THRESHOLD_UNSPECIFIED")]
    Unspecified,
    /// Content with NEGLIGIBLE will be allowed.
    #[serde(rename = "BLOCK_LOW_AND_ABOVE")]
    LowAndAbove,
    /// Content with NEGLIGIBLE and LOW will be allowed.
    #[serde(rename = "BLOCK_MEDIUM_AND_ABOVE")]
    MediumAndAbove,
    /// Content with NEGLIGIBLE, LOW, and MEDIUM will be allowed.
    #[serde(rename = "BLOCK_ONLY_HIGH")]
    OnlyHigh,
    /// All content will be allowed.
    #[serde(rename = "BLOCK_NONE")]
    None,
}

// {
//   "stopSequences": [
//     string
//   ],
//   "candidateCount": integer,
//   "maxOutputTokens": integer,
//   "temperature": number,
//   "topP": number,
//   "topK": integer
// }

#[derive(Debug, serde::Serialize, Clone, PartialEq, PartialOrd)]
pub struct GenerationConfig {
    #[serde(rename = "stopSequences")]
    stop_sequences: Vec<String>,
    // #[serde(rename = "candidateCount")]
    // candidate_count: i64,
    #[serde(rename = "maxOutputTokens")]
    max_output_tokens: i64,
    // temperature: f64,
    // #[serde(rename = "topP")]
    // top_p: f64,
    // #[serde(rename = "topK")]
    // top_k: i64,
}

// curl request based on the new function
// curl https://generativelanguage.googleapis.com/v1/models/gemini-1.5-pro:generateContent?key=$API_KEY \
//     -H 'Content-Type: application/json' \
//     -X POST \
//     -d '{ "contents":[
//       { "parts":[{"text": "What is the color of the sky?"}]}
//     ],
//     "safetySettings": [
//       {"category": "HARM_CATEGORY_HARASSMENT", "threshold": "BLOCK_NONE"},
//       {"category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "BLOCK_NONE"},
//       {"category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "BLOCK_NONE"},
//       {"category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "BLOCK_NONE"}
//     ],
//     "generationConfig": {
//       "stopSequences": ["\n"],
//       "maxOutputTokens": 8192
//     }
// }'
