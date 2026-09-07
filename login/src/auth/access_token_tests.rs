use super::*;

#[test]
fn classifies_personal_access_tokens_by_prefix() {
    assert!(matches!(
        classify_rexux_access_token("at-example"),
        RexuxAccessToken::PersonalAccessToken("at-example")
    ));
    assert!(matches!(
        classify_rexux_access_token("header.payload.signature"),
        RexuxAccessToken::AgentIdentityJwt("header.payload.signature")
    ));
}
