use core::future::Future;

use ferristrace_domain::{IngestKey, Scope, TenantDirectory, TenantId, UnknownKey};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

type Digest32 = [u8; 32];

#[derive(Debug, Default)]
pub struct StaticDirectory {
    entries: Vec<(Digest32, TenantId)>,
}

impl StaticDirectory {
    pub fn from_plaintext<I, K>(entries: I) -> Self
    where
        I: IntoIterator<Item = (K, TenantId)>,
        K: AsRef<str>,
    {
        Self {
            entries: entries
                .into_iter()
                .map(|(key, tenant)| (digest_of(key.as_ref()), tenant))
                .collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn lookup(&self, key: &IngestKey) -> Option<TenantId> {
        let offered = digest_of(key.exposed());
        let mut found = None;
        for (candidate, tenant) in &self.entries {
            if bool::from(candidate[..].ct_eq(&offered[..])) {
                found = Some(*tenant);
            }
        }
        found
    }
}

impl TenantDirectory for StaticDirectory {
    fn resolve(&self, key: &IngestKey) -> impl Future<Output = Result<Scope, UnknownKey>> + Send {
        let found = self.lookup(key).map(Scope::of).ok_or(UnknownKey);
        async move { found }
    }
}

fn digest_of(plaintext: &str) -> Digest32 {
    Sha256::digest(plaintext.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use ferristrace_domain::{IngestKey, Scope, TenantDirectory, TenantId, UnknownKey, testing};
    use uuid::Uuid;

    use super::StaticDirectory;

    #[tokio::test]
    async fn a_registered_key_resolves_to_its_tenant() {
        let tenant = TenantId::new(Uuid::from_u128(3));
        let directory = StaticDirectory::from_plaintext([("secret", tenant)]);

        let resolved = directory
            .resolve(&IngestKey::from_plaintext("secret"))
            .await;

        assert_eq!(resolved, Ok(Scope::of(tenant)));
    }

    #[tokio::test]
    async fn an_unregistered_key_resolves_to_nothing() {
        let directory =
            StaticDirectory::from_plaintext([("secret", TenantId::new(Uuid::from_u128(3)))]);

        let resolved = directory.resolve(&IngestKey::from_plaintext("other")).await;

        assert_eq!(resolved, Err(UnknownKey));
    }

    #[tokio::test]
    async fn an_empty_directory_resolves_nothing() {
        let directory = StaticDirectory::default();

        assert!(directory.is_empty());
        assert_eq!(
            directory
                .resolve(&IngestKey::from_plaintext("secret"))
                .await,
            Err(UnknownKey)
        );
    }

    #[tokio::test]
    async fn a_static_directory_honours_the_directory_contract() {
        let tenant = TenantId::new(Uuid::from_u128(3));
        let directory = StaticDirectory::from_plaintext([("secret", tenant)]);
        let held = IngestKey::from_plaintext("secret");
        let absent = IngestKey::from_plaintext("other");

        testing::a_directory_resolves_the_key_it_holds(&directory, &held, tenant).await;
        testing::a_directory_refuses_a_key_it_does_not_hold(&directory, &absent).await;
        testing::a_directory_answers_the_same_key_the_same_way_twice(&directory, &held).await;
    }
}
