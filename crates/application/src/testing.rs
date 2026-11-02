use core::future::Future;

pub use ferristrace_domain::testing::Notebook;
use ferristrace_domain::{
    AccountId, Admission, ApiKey, ApiKeys, ApiSecret, AuditEntry, AuditFilter, AuditPage,
    AuditTrail, AuthError, AuthProvider, Authorizer, Clock, Counted, DecompressedBytes, Email,
    Fingerprint, HashError, IngestKey, IssueFilter, IssueReader, IssuedKey, KeyId, KeyName,
    KeyScopes, Mailer, Occurrence, PageSize, Password, PasswordCheck, PasswordHash, PasswordHasher,
    PendingFlow, ProjectId, QuotaPolicy, ReadGrant, ResetError, ResetToken, ResolvedIdentity,
    Scope, ServiceSummary, SignInProof, SinkError, Span, SpanCursor, SpanPage, SpanPages,
    SpanReader, SpanSink, StoreError, TenantDirectory, TenantId, TraceFilter, TraceId,
    TraceSummary, UnixSeconds, UnknownKey, UsageMeter, UsageReader, Window,
};

use crate::registry::{Settle, UnitOfWork};

pub trait Untransacted: Send + Sync + 'static {}

impl<T: Untransacted> UnitOfWork for T {
    type Tx = ();

    fn transaction<R, E, F, Fut>(
        &self,
        _unavailable: impl Fn() -> E + Send,
        _settle: Settle,
        work: F,
    ) -> impl Future<Output = Result<R, E>> + Send
    where
        R: Send,
        E: Send,
        F: FnOnce(Self::Tx) -> Fut + Send,
        Fut: Future<Output = Result<R, E>> + Send,
    {
        work(())
    }
}

#[derive(Debug)]
pub struct Inert(PasswordHash);

impl Default for Inert {
    fn default() -> Self {
        Self(PasswordHash::from_encoded("decoy"))
    }
}

impl Clock for Inert {
    fn now(&self) -> UnixSeconds {
        UnixSeconds::new(0)
    }
}

impl PasswordHasher for Inert {
    async fn hash(&self, _password: &Password) -> Result<PasswordHash, HashError> {
        Ok(self.0.clone())
    }

    async fn check(
        &self,
        _password: &Password,
        _against: &PasswordHash,
    ) -> Result<PasswordCheck, HashError> {
        Ok(PasswordCheck::Rejected)
    }

    fn decoy(&self) -> &PasswordHash {
        &self.0
    }
}

impl Mailer for Inert {
    async fn send_password_reset(
        &self,
        _to: &Email,
        _token: &ResetToken,
    ) -> Result<(), ResetError> {
        Ok(())
    }
}

impl SpanReader for Inert {
    async fn services(
        &self,
        _tenant: TenantId,
        _window: Window,
        _project: Option<ProjectId>,
    ) -> Result<Vec<ServiceSummary>, StoreError> {
        Ok(Vec::new())
    }

    async fn traces(
        &self,
        _tenant: TenantId,
        _filter: &TraceFilter,
    ) -> Result<Vec<TraceSummary>, StoreError> {
        Ok(Vec::new())
    }

    async fn trace(
        &self,
        _tenant: TenantId,
        _id: TraceId,
        _project: Option<ProjectId>,
    ) -> Result<Vec<Span>, StoreError> {
        Ok(Vec::new())
    }
}

impl SpanPages for Inert {
    async fn page(
        &self,
        _org: TenantId,
        _after: Option<&SpanCursor>,
        _limit: u32,
    ) -> Result<SpanPage, StoreError> {
        Ok(SpanPage::default())
    }
}

impl IssueReader for Inert {
    async fn issues(
        &self,
        _tenant: TenantId,
        _filter: &IssueFilter,
    ) -> Result<Vec<Counted>, StoreError> {
        Ok(Vec::new())
    }

    async fn occurrences(
        &self,
        _tenant: TenantId,
        _project: ProjectId,
        _print: Fingerprint,
        _window: Window,
        _limit: u32,
    ) -> Result<Vec<Occurrence>, StoreError> {
        Ok(Vec::new())
    }
}

impl UsageMeter for Inert {
    async fn record_ingest(&self, _tenant: TenantId, _decompressed: DecompressedBytes) {}
}

impl UsageReader for Inert {
    async fn used(&self, _tenant: TenantId) -> DecompressedBytes {
        DecompressedBytes::new(0)
    }
}

impl Authorizer for Inert {
    fn begin(&self) -> Result<(String, PendingFlow), AuthError> {
        Err(AuthError::SourceDisabled)
    }
}

impl AuthProvider for Inert {
    async fn sign_in(&self, _proof: SignInProof) -> Result<ResolvedIdentity, AuthError> {
        Err(AuthError::SourceDisabled)
    }
}

impl SpanSink for Inert {
    async fn accept(&self, _scope: Scope, _spans: Vec<Span>) -> Result<(), SinkError> {
        Ok(())
    }
}

impl QuotaPolicy for Inert {
    async fn admit(&self, _tenant: TenantId, _incoming: DecompressedBytes) -> Admission {
        Admission::Within
    }
}

impl TenantDirectory for Inert {
    async fn resolve(&self, _key: &IngestKey) -> Result<Scope, UnknownKey> {
        Err(UnknownKey)
    }
}

impl ApiKeys for Inert {
    async fn list(&self, _org: TenantId) -> Result<Vec<ApiKey>, StoreError> {
        Ok(Vec::new())
    }

    async fn issue(
        &self,
        _org: TenantId,
        _project: Option<ProjectId>,
        _name: &KeyName,
        _scopes: &KeyScopes,
        _by: AccountId,
    ) -> Result<IssuedKey, StoreError> {
        Err(StoreError::Unreachable)
    }

    async fn revoke(&self, _org: TenantId, _id: KeyId) -> Result<bool, StoreError> {
        Ok(false)
    }

    async fn grant_for(&self, _secret: &ApiSecret) -> Result<Option<ReadGrant>, StoreError> {
        Ok(None)
    }
}

impl AuditTrail for Inert {
    async fn record(&self, _entry: &AuditEntry) -> Result<(), StoreError> {
        Ok(())
    }

    async fn list(
        &self,
        _org: TenantId,
        _filter: &AuditFilter,
        _page: PageSize,
    ) -> Result<AuditPage, StoreError> {
        Ok(AuditPage::default())
    }
}
