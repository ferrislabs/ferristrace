use ferristrace_domain::{
    Agent, Answer, AuthError, AuthSources, ChallengeToken, ChangeError, ChangePassword, Confirmed,
    Email, Enrolment, FactorError, FactorKind, FederatedError, FederatedSignIn, FlowError,
    LiveSession, NoProvider, Opened, Password, RecoveryCode, ResetError, ResetPassword, ResetToken,
    RollError, SecondFactors, SecondStep, Session, SessionError, SessionId, SessionToken, Sessions,
    SignIn, SignInError, SignInProof, Standing, TotpCode, TotpSecret,
};

use crate::borrowed::Borrowed;
use crate::registry::IdentityRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: IdentityRepos,
{
    #[transactional(
        local_passwords, accounts, sessions, factors,
        unavailable = SignInError::Session(SessionError::StoreUnreachable)
    )]
    pub async fn sign_in(
        &self,
        proof: SignInProof,
        from: Option<&Agent>,
    ) -> Result<Opened, SignInError> {
        SignIn::new(
            AuthSources::new(Some(local_passwords), None::<NoProvider>),
            accounts,
            sessions,
            factors,
            self.provision,
        )
        .begin(proof, from)
        .await
    }

    #[transactional(
        local_passwords, accounts, sessions, factors,
        unavailable = SignInError::Session(SessionError::StoreUnreachable)
    )]
    pub async fn resolve_session(&self, token: &SessionToken) -> Result<Session, SignInError> {
        SignIn::new(
            AuthSources::new(Some(local_passwords), None::<NoProvider>),
            accounts,
            sessions,
            factors,
            self.provision,
        )
        .resolve(token)
        .await
    }

    #[transactional(
        local_passwords, accounts, sessions, factors,
        unavailable = SignInError::Session(SessionError::StoreUnreachable)
    )]
    pub async fn sign_out(&self, token: &SessionToken) -> Result<(), SignInError> {
        SignIn::new(
            AuthSources::new(Some(local_passwords), None::<NoProvider>),
            accounts,
            sessions,
            factors,
            self.provision,
        )
        .end(token)
        .await
    }

    #[transactional(sessions, factors, unavailable = FactorError::StoreUnreachable)]
    pub async fn factor_standing(&self, token: &SessionToken) -> Result<Standing, FactorError> {
        SecondFactors::new(factors, sessions, &self.clock)
            .standing(token)
            .await
    }

    #[transactional(sessions, factors, unavailable = FactorError::StoreUnreachable)]
    pub async fn enrol_factor(
        &self,
        token: &SessionToken,
        secret: &TotpSecret,
    ) -> Result<Enrolment, FactorError> {
        SecondFactors::new(factors, sessions, &self.clock)
            .enrol(token, secret)
            .await
    }

    #[transactional(sessions, factors, unavailable = FactorError::StoreUnreachable)]
    pub async fn confirm_factor(
        &self,
        token: &SessionToken,
        code: TotpCode,
        minted: Vec<RecoveryCode>,
    ) -> Result<Confirmed, FactorError> {
        SecondFactors::new(factors, sessions, &self.clock)
            .confirm(token, code, minted)
            .await
    }

    #[transactional(sessions, factors, unavailable = FactorError::StoreUnreachable)]
    pub async fn forget_factor(
        &self,
        token: &SessionToken,
        kind: FactorKind,
    ) -> Result<(), FactorError> {
        SecondFactors::new(factors, sessions, &self.clock)
            .forget(token, kind)
            .await
    }

    #[transactional(sessions, factors, unavailable = FactorError::StoreUnreachable)]
    pub async fn regenerate_recovery_codes(
        &self,
        token: &SessionToken,
        minted: Vec<RecoveryCode>,
    ) -> Result<Vec<RecoveryCode>, FactorError> {
        SecondFactors::new(factors, sessions, &self.clock)
            .regenerate(token, minted)
            .await
    }

    #[transactional(
        sessions,
        factors,
        unavailable = FactorError::StoreUnreachable,
        settle = always
    )]
    pub async fn verify_second_step(
        &self,
        challenge: &ChallengeToken,
        answer: &Answer,
        from: Option<&Agent>,
    ) -> Result<SessionToken, FactorError> {
        SecondStep::new(factors, sessions, &self.clock)
            .verify(challenge, answer, from)
            .await
    }

    #[transactional(resets, unavailable = ResetError::StoreUnreachable)]
    pub async fn request_reset(
        &self,
        email: &Email,
        token: ResetToken,
    ) -> Result<bool, ResetError> {
        ResetPassword::new(resets, Borrowed(&self.hasher), Borrowed(&self.mailer))
            .request(email, token)
            .await
    }

    #[transactional(resets, unavailable = ResetError::StoreUnreachable)]
    pub async fn confirm_reset(
        &self,
        token: &ResetToken,
        chosen: &Password,
    ) -> Result<u64, ResetError> {
        ResetPassword::new(resets, Borrowed(&self.hasher), Borrowed(&self.mailer))
            .confirm(token, chosen)
            .await
    }

    #[transactional(sessions, local_passwords, resets, unavailable = ChangeError::Unavailable)]
    pub async fn change_password(
        &self,
        token: &SessionToken,
        current: &Password,
        chosen: &Password,
    ) -> Result<u64, ChangeError> {
        ChangePassword::new(sessions, local_passwords, resets, Borrowed(&self.hasher))
            .change(token, current, chosen)
            .await
    }

    #[transactional(sessions, unavailable = RollError::Unavailable)]
    pub async fn live_sessions(&self, token: &SessionToken) -> Result<Vec<LiveSession>, RollError> {
        Sessions::new(&sessions, Borrowed(&sessions))
            .live(token)
            .await
    }

    #[transactional(sessions, unavailable = RollError::Unavailable)]
    pub async fn end_session(&self, token: &SessionToken, id: SessionId) -> Result<(), RollError> {
        Sessions::new(&sessions, Borrowed(&sessions))
            .end_one(token, id)
            .await
    }

    #[transactional(sessions, unavailable = RollError::Unavailable)]
    pub async fn end_other_sessions(&self, token: &SessionToken) -> Result<u64, RollError> {
        Sessions::new(&sessions, Borrowed(&sessions))
            .end_others(token)
            .await
    }

    #[transactional(flows, accounts, sessions, unavailable = FederatedError::Flow(FlowError::StoreUnreachable))]
    pub async fn start_federated_sign_in(&self) -> Result<String, FederatedError> {
        let Some(provider) = &self.oidc else {
            return Err(FederatedError::Auth(AuthError::SourceDisabled));
        };
        FederatedSignIn::new(provider, flows, accounts, sessions, self.provision)
            .start()
            .await
    }

    #[transactional(
        flows,
        accounts,
        sessions,
        unavailable = FederatedError::Flow(FlowError::StoreUnreachable),
        settle = always
    )]
    pub async fn finish_federated_sign_in(
        &self,
        code: String,
        state: &str,
        from: Option<&Agent>,
    ) -> Result<SessionToken, FederatedError> {
        let Some(provider) = &self.oidc else {
            return Err(FederatedError::Auth(AuthError::SourceDisabled));
        };
        FederatedSignIn::new(provider, flows, accounts, sessions, self.provision)
            .finish(code, state, from)
            .await
    }
}
