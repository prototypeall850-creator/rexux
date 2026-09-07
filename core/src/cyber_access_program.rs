use rexux_api::AccessPrograms;
use rexux_login::RexuxAuth;
use rexux_protocol::turn_input::CyberAccessProgram;

pub(crate) fn for_auth(
    auth: Option<&RexuxAuth>,
    program: Option<CyberAccessProgram>,
) -> Option<AccessPrograms> {
    program
        .filter(|_| auth.is_some_and(RexuxAuth::is_chatgpt_auth))
        .map(AccessPrograms::from)
}
