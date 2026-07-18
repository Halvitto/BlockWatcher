use blockwatcher_core::parse_ccusage_daily;

#[test]
fn parses_only_the_requested_day_and_merges_model_rows() {
    let report = include_str!("fixtures/ccusage_daily.json");
    let activity = parse_ccusage_daily(report, "2026-07-18").unwrap();

    assert_eq!(activity.len(), 2);
    let codex = activity.iter().find(|item| item.id == "codex").unwrap();
    assert_eq!(codex.total_tokens, 125);
    assert_eq!(codex.models[0].name, "gpt-5.6-sol");

    let hermes = activity.iter().find(|item| item.id == "hermes").unwrap();
    assert_eq!(hermes.total_tokens, 40);
    assert_eq!(hermes.tokens.total(), 36);
    assert_eq!(hermes.models.len(), 1);
    assert_eq!(hermes.models[0].tokens.total(), 36);
}

#[test]
fn rejects_invalid_json_without_reusing_partial_data() {
    assert!(parse_ccusage_daily("{", "2026-07-18").is_err());
}
