use super::*;

const LIVE: &str = r#"{
  "model": "typesafe/jev-1.13-20260917",
  "answers": {
    "kind": {
      "type": "choice",
      "choice": "antibot",
      "probabilities": {"logic": 0.0, "antibot": 1.0, "network": 0.0, "site_change": 0.0},
      "confidence": 1.0
    },
    "severity": {
      "type": "score",
      "score": 1.96,
      "legend": {"0": "Cosmetic, retry will fix", "1": "Degraded, some attempts fail", "2": "Hard stop, checkout cannot proceed"},
      "probabilities": {"0": 0.0, "1": 0.04, "2": 0.96},
      "confidence": 0.94
    },
    "should_rotate": {"type": "noul", "noul": 0.66}
  },
  "usage": {"input_tokens": 479, "output_tokens": 81, "cost": 2.0118e-05}
}"#;

pub(crate) fn endpoint(name: &str, wire: Wire) -> Endpoint {
    Endpoint {
        name: name.to_string(),
        model: "jev-latest".to_string(),
        base_url: Some("https://api.typesafe.ai/v1/systemone".to_string()),
        api_key: Some("k".to_string()),
        source: None,
        reasoning: crate::provider::Reasoning::default(),
        oauth: None,
        body: None,
        context_window: None,
        idle_timeout_secs: None,
        effort: None,
        price: None,
        env: BTreeMap::new(),
        headers: BTreeMap::new(),
        wire,
        fallback: Vec::new(),
        compact_tokens: None,
        images: None,
        keep_reasoning: false,
        opencode: false,
        prompt_cache_key: false,
    }
}

#[test]
fn a_real_response_still_parses_into_typed_answers() {
    let decision: Decision = serde_json::from_str(LIVE).unwrap();
    assert_eq!(decision.model, "typesafe/jev-1.13-20260917");
    assert_eq!(decision.usage.input_tokens, 479);
    let Some(Answer::Choice {
        choice,
        probabilities,
        confidence,
    }) = decision.answers.get("kind")
    else {
        panic!("kind did not parse as a choice: {:?}", decision.answers);
    };
    assert_eq!(choice, "antibot");
    assert_eq!(probabilities["antibot"], 1.0);
    assert_eq!(*confidence, 1.0);
    let Some(Answer::Score { score, legend, .. }) = decision.answers.get("severity") else {
        panic!("severity did not parse as a score");
    };
    assert_eq!(*score, 1.96);
    assert_eq!(legend["2"], "Hard stop, checkout cannot proceed");
    let Some(Answer::Noul { noul }) = decision.answers.get("should_rotate") else {
        panic!("should_rotate did not parse as a noul");
    };
    assert_eq!(*noul, 0.66);
}

#[test]
fn options_render_in_probability_order_not_alphabetical() {
    let decision = Decision {
        model: "m".to_string(),
        answers: BTreeMap::from([(
            "kind".to_string(),
            Answer::Choice {
                choice: "zeta".to_string(),
                probabilities: BTreeMap::from([
                    ("alpha".to_string(), 0.1),
                    ("zeta".to_string(), 0.9),
                ]),
                confidence: 0.8,
            },
        )]),
        usage: Usage::default(),
    };
    let lines: Vec<_> = render(&decision).lines().map(str::to_string).collect();
    assert!(
        lines[1].contains("zeta"),
        "the winning option must lead, got {lines:?}"
    );
    assert!(lines[2].contains("alpha"), "got {lines:?}");
}

#[test]
fn a_long_level_name_keeps_every_bar_in_one_column() {
    let decision: Decision = serde_json::from_str(LIVE).unwrap();
    let rendered = render(&decision);
    let bars: Vec<_> = rendered
        .lines()
        .filter(|line| line.starts_with("  "))
        .map(|line| {
            line.chars()
                .position(|glyph| glyph == '█' || glyph == '░')
                .expect("a bar on every option row")
        })
        .collect();
    assert!(bars.len() >= 7, "expected every option row, got {bars:?}");
    assert!(
        bars.windows(2).all(|pair| pair[0] == pair[1]),
        "bars start in different columns: {bars:?}"
    );
}

#[test]
fn a_full_bar_is_full_and_an_empty_one_is_empty() {
    let decision: Decision = serde_json::from_str(LIVE).unwrap();
    let rendered = render(&decision);
    let antibot = rendered
        .lines()
        .find(|line| line.contains("antibot") && line.starts_with("  "))
        .unwrap();
    assert_eq!(antibot.matches('█').count(), WIDTH);
    assert_eq!(antibot.matches('░').count(), 0);
    let network = rendered
        .lines()
        .find(|line| line.contains("network"))
        .unwrap();
    assert_eq!(network.matches('█').count(), 0);
    assert_eq!(network.matches('░').count(), WIDTH);
}

#[test]
fn a_chat_endpoint_is_refused_before_any_request_goes_out() {
    let questions = BTreeMap::from([("q".to_string(), Question::noul("is it broken"))]);
    let error = target(&endpoint("openrouter", Wire::Completions), &questions)
        .expect_err("a chat endpoint must not reach the decisions API");
    assert!(
        error.to_string().contains("not a classify endpoint"),
        "got {error}"
    );
}

#[test]
fn asking_nothing_is_refused() {
    let error = target(&endpoint("jev", Wire::Decisions), &BTreeMap::new())
        .expect_err("an empty question set must not be sent");
    assert!(error.to_string().contains("at least one question"));
}

#[test]
fn each_decisions_provider_keeps_its_own_url() {
    let questions = BTreeMap::from([("q".to_string(), Question::noul("is it broken"))]);
    let openrouter =
        crate::endpoint::resolve(Some("openrouter"), None, crate::endpoint::Need::Model).unwrap();
    assert_eq!(
        openrouter.base_url.as_deref(),
        Some("https://openrouter.ai/api/v1")
    );
    assert_eq!(openrouter.wire, Wire::Completions);

    let mut other = endpoint("other", Wire::Decisions);
    other.base_url = Some("https://example.test/decisions".to_string());
    target(&other, &questions).unwrap();
}

#[test]
fn the_request_keeps_the_shape_the_api_documents() {
    let questions = BTreeMap::from([
        (
            "kind".to_string(),
            Question::choice("classify", [("antibot", "blocked")]),
        ),
        (
            "severity".to_string(),
            Question::score("how bad", ["mild", "fatal"]),
        ),
    ]);
    let body = crate::provider::decisions::body(
        "jev-latest",
        &serde_json::Value::from("a 403"),
        &questions,
    );
    assert_eq!(body["model"], "jev-latest");
    assert_eq!(body["state"], "a 403");
    assert_eq!(body["questions"]["kind"]["type"], "choice");
    assert!(
        body["questions"]["kind"]["criteria"].is_object(),
        "choice criteria are option to description"
    );
    assert_eq!(body["questions"]["severity"]["type"], "score");
    assert!(
        body["questions"]["severity"]["criteria"].is_array(),
        "score criteria are ordered levels"
    );
}

#[test]
fn an_api_error_carries_the_wait_the_server_asked_for() {
    let failure =
        crate::provider::response_failure(429, r#"{"error":{"message":"slow down"}}"#, Some(30));
    let Error::Provider(failure) = failure else {
        panic!("a rate limit must classify as a provider failure");
    };
    assert_eq!(failure.kind, ProviderFailureKind::RateLimited);
    assert_eq!(
        failure.retry_after,
        Some(30),
        "the Retry-After header is the wait, and the caller cannot honour it if we drop it"
    );
    assert!(
        failure.message.contains("slow down"),
        "got {}",
        failure.message
    );
}

#[test]
fn a_decisions_failure_classifies_like_every_other_provider_failure() {
    for (status, expected) in [
        (401, ProviderFailureKind::Authentication),
        (429, ProviderFailureKind::RateLimited),
        (529, ProviderFailureKind::Server),
    ] {
        let Error::Provider(failure) = crate::provider::response_failure(status, "{}", None) else {
            panic!("status {status} did not classify");
        };
        assert_eq!(failure.kind, expected, "status {status}");
    }
}

#[test]
fn the_decisions_model_is_absent_from_the_chat_catalog() {
    let chat = crate::endpoint::catalog(Wire::Completions);
    assert!(
        !chat.iter().any(|model| model.contains("jev")),
        "jev must never be offered as a chat model: {chat:?}"
    );
}

#[test]
fn any_endpoint_can_declare_itself_a_decisions_endpoint() {
    for spelling in ["decisions", "systemone", "system-one", "DECISIONS"] {
        assert_eq!(
            crate::endpoint::Wire::parse(spelling),
            Wire::Decisions,
            "config must be able to declare `{spelling}`"
        );
    }
}

#[test]
fn a_provider_this_code_has_never_heard_of_still_works() {
    let questions = BTreeMap::from([("q".to_string(), Question::noul("is it broken"))]);
    let mut unknown = endpoint("some-future-vendor", Wire::Decisions);
    unknown.base_url = Some("https://example.invalid/v9/judge".to_string());
    unknown.model = "not-jev-at-all".to_string();
    target(&unknown, &questions).unwrap();
    let body =
        crate::provider::decisions::body(&unknown.model, &serde_json::Value::from("s"), &questions);
    assert_eq!(body["model"], "not-jev-at-all");
}

#[test]
fn a_roomier_model_is_given_the_room_it_declares() {
    let questions = BTreeMap::from([("q".to_string(), Question::noul("is it broken"))]);
    let mut small = endpoint("small", Wire::Decisions);
    small.context_window = Some(32_000);
    let mut large = endpoint("jev2", Wire::Decisions);
    large.context_window = Some(200_000);
    assert!(
        budget(&large, &questions) > budget(&small, &questions) * 5,
        "a model that declares a larger context must not be windowed to the small default"
    );
    let unknown = endpoint("unnamed", Wire::Decisions);
    assert_eq!(
        budget(&unknown, &questions),
        CONTEXT_CHARS - serde_json::to_string(&questions).unwrap().len(),
        "a provider that declares nothing falls back, it does not get zero"
    );
}

#[test]
fn the_questions_are_charged_against_the_context() {
    let one = BTreeMap::from([("q".to_string(), Question::noul("short"))]);
    let many: BTreeMap<String, Question> = (0..40)
        .map(|index| {
            (
                format!("q{index}"),
                Question::noul("a much longer question about the state".repeat(4)),
            )
        })
        .collect();
    let seat = endpoint("any", Wire::Decisions);
    assert!(
        budget(&seat, &many) < budget(&seat, &one),
        "asking more must leave less room for the state"
    );
}

#[test]
fn an_answer_from_a_provider_with_extra_fields_still_parses() {
    let body = r#"{"model":"other/judge-2","answers":{"kind":{"type":"choice","choice":"a",
      "probabilities":{"a":0.9,"b":0.1},"confidence":0.9,"rationale_id":"x","latency_ms":12}},
      "usage":{"input_tokens":1,"output_tokens":0},"trace":"abc"}"#;
    let decision: Decision = serde_json::from_str(body).unwrap();
    assert_eq!(decision.model, "other/judge-2");
    assert!(
        decision.answers.contains_key("kind"),
        "a provider adding fields must not break the read"
    );
}
