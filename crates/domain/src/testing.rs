use crate::key::IngestKey;
use crate::tenant::{TenantDirectory, TenantId, UnknownKey};

pub async fn a_directory_resolves_the_key_it_holds<D: TenantDirectory>(
    directory: &D,
    held: &IngestKey,
    tenant: TenantId,
) {
    let resolved = directory.resolve(held).await;

    assert_eq!(resolved.map(|scope| scope.tenant), Ok(tenant));
}

pub async fn a_directory_refuses_a_key_it_does_not_hold<D: TenantDirectory>(
    directory: &D,
    absent: &IngestKey,
) {
    let resolved = directory.resolve(absent).await;

    assert_eq!(resolved.map(|scope| scope.tenant), Err(UnknownKey));
}

pub async fn a_directory_answers_the_same_key_the_same_way_twice<D: TenantDirectory>(
    directory: &D,
    held: &IngestKey,
) {
    let once = directory.resolve(held).await;
    let twice = directory.resolve(held).await;

    assert_eq!(once, twice);
}
