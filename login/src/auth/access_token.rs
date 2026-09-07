const PERSONAL_ACCESS_TOKEN_PREFIX: &str = "at-";

pub(super) enum RexuxAccessToken<'a> {
    PersonalAccessToken(&'a str),
    AgentIdentityJwt(&'a str),
}

pub(super) fn classify_rexux_access_token(access_token: &str) -> RexuxAccessToken<'_> {
    if access_token.starts_with(PERSONAL_ACCESS_TOKEN_PREFIX) {
        RexuxAccessToken::PersonalAccessToken(access_token)
    } else {
        RexuxAccessToken::AgentIdentityJwt(access_token)
    }
}

#[cfg(test)]
#[path = "access_token_tests.rs"]
mod tests;
