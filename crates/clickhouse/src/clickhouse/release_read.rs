use std::collections::BTreeMap;

use clickhouse::Row;
use ferristrace_domain::{
    Counted, DEPLOY_FEED_LIMIT, Deploy, DeployFeed, Deployed, Fingerprint, ProjectId,
    RELEASE_LIMIT, Release, ReleaseDetail, ReleaseFilter, ReleaseReader, StoreError,
    TRAFFIC_HOUR_NANOS, TenantId, Traffic, Window,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::clickhouse::issues::ClickhouseIssues;
use crate::clickhouse::read::{held, nanos, refused, unsigned, uuid_of};

const RELEASES: &str = "SELECT \
     service_name AS service, \
     version AS version, \
     groupUniqArrayIf(environment, environment != '') AS environments, \
     toUnixTimestamp64Nano(min(first_seen)) AS first_nanos, \
     toUnixTimestamp64Nano(max(last_seen)) AS last_nanos, \
     sum(requests) AS seen_requests, \
     sum(errors) AS seen_errors \
   FROM releases \
   WHERE tenant_id = ? \
     AND (? = 0 OR project_id = ?) \
     AND (? = '' OR service_name = ?) \
   GROUP BY service_name, version \
   ORDER BY first_nanos DESC, service, version \
   LIMIT {limit}";

const DEPLOYS: &str = "SELECT \
     service_name AS service, \
     version AS version, \
     environment AS environment, \
     toUnixTimestamp64Nano(min(first_seen)) AS at_nanos \
   FROM releases \
   WHERE tenant_id = ? \
     AND (? = 0 OR project_id = ?) \
     AND (? = '' OR service_name = ?) \
   GROUP BY service_name, version, environment \
   HAVING at_nanos >= ? AND at_nanos < ? \
   ORDER BY at_nanos, service, version, environment \
   LIMIT {limit}";

const FEED: &str = "SELECT \
     toString(tenant_id) AS tenant, \
     service_name AS service, \
     version AS version, \
     environment AS environment, \
     toUnixTimestamp64Nano(min(first_seen)) AS at_nanos \
   FROM releases \
   GROUP BY tenant_id, service_name, version, environment \
   HAVING at_nanos >= ? AND at_nanos < ? \
   ORDER BY at_nanos, tenant, service, version, environment \
   LIMIT {limit}";

const FIRST_SEEN_ISSUES: &str = "SELECT \
     argMin(service_name, time) AS service, \
     toUnixTimestamp64Nano(min(time)) AS first_nanos \
   FROM span_events \
   WHERE tenant_id = ? \
     AND name = 'exception' \
     AND (? = 0 OR project_id = ?) \
     AND (? = '' OR service_name = ?) \
   GROUP BY project_id, fingerprint \
   HAVING first_nanos >= ? \
   LIMIT 100000";

const INTRODUCED: &str = "SELECT \
     toString(project_id) AS project_id, \
     lower(hex(fingerprint)) AS fingerprint, \
     any(attributes['exception.type']) AS exception_type, \
     argMax(attributes['exception.message'], time) AS message, \
     argMin(service_name, time) AS service, \
     count() AS seen, \
     toUnixTimestamp64Nano(min(time)) AS first_nanos, \
     toUnixTimestamp64Nano(max(time)) AS last_nanos \
   FROM span_events \
   WHERE tenant_id = ? \
     AND name = 'exception' \
     AND (? = 0 OR project_id = ?) \
   GROUP BY project_id, fingerprint \
   HAVING service = ? AND first_nanos >= ? AND first_nanos < ? \
   ORDER BY first_nanos, fingerprint \
   LIMIT 200";

const AROUND: &str = "SELECT \
     countIf(start_time < fromUnixTimestamp64Nano(?)) AS before_requests, \
     countIf(start_time < fromUnixTimestamp64Nano(?) AND status = 'error') AS before_errors, \
     countIf(start_time >= fromUnixTimestamp64Nano(?)) AS after_requests, \
     countIf(start_time >= fromUnixTimestamp64Nano(?) AND status = 'error') AS after_errors \
   FROM spans FINAL \
   WHERE tenant_id = ? \
     AND service_name = ? \
     AND kind IN ('server', 'consumer') \
     AND (? = 0 OR project_id = ?) \
     AND start_time >= fromUnixTimestamp64Nano(?) \
     AND start_time < fromUnixTimestamp64Nano(?)";

#[derive(Debug, Row, Deserialize)]
struct ReleaseRow {
    service: String,
    version: String,
    environments: Vec<String>,
    first_nanos: i64,
    last_nanos: i64,
    seen_requests: u64,
    seen_errors: u64,
}

#[derive(Debug, Row, Deserialize)]
struct DeployRow {
    service: String,
    version: String,
    environment: String,
    at_nanos: i64,
}

#[derive(Debug, Row, Deserialize)]
struct FedRow {
    tenant: String,
    service: String,
    version: String,
    environment: String,
    at_nanos: i64,
}

#[derive(Debug, Row, Deserialize)]
struct FirstSeenRow {
    service: String,
    first_nanos: i64,
}

#[derive(Debug, Row, Deserialize)]
struct IntroducedRow {
    project_id: String,
    fingerprint: String,
    exception_type: String,
    message: String,
    service: String,
    seen: u64,
    first_nanos: i64,
    last_nanos: i64,
}

#[derive(Debug, Row, Deserialize)]
struct AroundRow {
    before_requests: u64,
    before_errors: u64,
    after_requests: u64,
    after_errors: u64,
}

fn limited(sql: &str, limit: u32) -> String {
    sql.replace("{limit}", &limit.to_string())
}

fn read_release(row: ReleaseRow) -> Release {
    let mut environments: Vec<Box<str>> = row.environments.into_iter().map(Box::from).collect();
    environments.sort();
    Release {
        service: row.service.into(),
        version: row.version.into(),
        environments,
        first_seen_unix_nanos: unsigned(row.first_nanos),
        last_seen_unix_nanos: unsigned(row.last_nanos),
        requests: row.seen_requests,
        errors: row.seen_errors,
        new_issues: 0,
    }
}

pub(crate) fn rollout_ends(releases: &[Release]) -> BTreeMap<(&str, &str), u64> {
    let mut by_service: BTreeMap<&str, Vec<&Release>> = BTreeMap::new();
    for release in releases {
        by_service
            .entry(&release.service)
            .or_default()
            .push(release);
    }
    let mut ends = BTreeMap::new();
    for (service, mut versions) in by_service {
        versions.sort_by_key(|release| (release.first_seen_unix_nanos, release.version.clone()));
        for (index, release) in versions.iter().enumerate() {
            let end = versions[index + 1..]
                .iter()
                .map(|later| later.first_seen_unix_nanos)
                .find(|first| *first > release.first_seen_unix_nanos)
                .unwrap_or(u64::MAX);
            ends.insert((service, &*release.version), end);
        }
    }
    ends
}

fn read_counted(row: IntroducedRow) -> Result<Counted, StoreError> {
    Ok(Counted {
        project: ProjectId::new(Uuid::parse_str(&row.project_id).map_err(|error| held(&error))?),
        print: Fingerprint::try_from(row.fingerprint.as_str()).map_err(|error| held(&error))?,
        exception_type: row.exception_type.into(),
        message: row.message.into(),
        service: row.service.into(),
        count: row.seen,
        first_seen_unix_nanos: unsigned(row.first_nanos),
        last_seen_unix_nanos: unsigned(row.last_nanos),
    })
}

impl ClickhouseIssues {
    async fn every_release(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
        service: Option<&str>,
    ) -> Result<Vec<Release>, StoreError> {
        let service = service.unwrap_or_default();
        let rows: Vec<ReleaseRow> = self
            .client()
            .query(&limited(RELEASES, RELEASE_LIMIT * 40))
            .bind(org.as_uuid())
            .bind(u8::from(project.is_some()))
            .bind(uuid_of(project))
            .bind(service)
            .bind(service)
            .fetch_all()
            .await
            .map_err(|error| refused(&error))?;
        Ok(rows.into_iter().map(read_release).collect())
    }

    async fn introduced_in(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
        release: &Release,
        end: u64,
    ) -> Result<Vec<Counted>, StoreError> {
        let rows: Vec<IntroducedRow> = self
            .client()
            .query(INTRODUCED)
            .bind(org.as_uuid())
            .bind(u8::from(project.is_some()))
            .bind(uuid_of(project))
            .bind(&*release.service)
            .bind(nanos(release.first_seen_unix_nanos))
            .bind(nanos(end))
            .fetch_all()
            .await
            .map_err(|error| refused(&error))?;
        rows.into_iter().map(read_counted).collect()
    }

    async fn around(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
        release: &Release,
    ) -> Result<(Traffic, Traffic), StoreError> {
        let first = release.first_seen_unix_nanos;
        let from = first.saturating_sub(TRAFFIC_HOUR_NANOS);
        let to = first.saturating_add(TRAFFIC_HOUR_NANOS);
        let row: AroundRow = self
            .client()
            .query(AROUND)
            .bind(nanos(first))
            .bind(nanos(first))
            .bind(nanos(first))
            .bind(nanos(first))
            .bind(org.as_uuid())
            .bind(&*release.service)
            .bind(u8::from(project.is_some()))
            .bind(uuid_of(project))
            .bind(nanos(from))
            .bind(nanos(to))
            .fetch_one()
            .await
            .map_err(|error| refused(&error))?;
        Ok((
            Traffic {
                requests: row.before_requests,
                errors: row.before_errors,
            },
            Traffic {
                requests: row.after_requests,
                errors: row.after_errors,
            },
        ))
    }
}

impl ReleaseReader for ClickhouseIssues {
    async fn releases(
        &self,
        org: TenantId,
        filter: &ReleaseFilter,
    ) -> Result<Vec<Release>, StoreError> {
        let service = filter.service.as_deref();
        let mut releases = self.every_release(org, filter.project, service).await?;
        let lowest = releases
            .iter()
            .map(|release| release.first_seen_unix_nanos)
            .min();
        if let Some(lowest) = lowest {
            let service = service.unwrap_or_default();
            let firsts: Vec<FirstSeenRow> = self
                .client()
                .query(FIRST_SEEN_ISSUES)
                .bind(org.as_uuid())
                .bind(u8::from(filter.project.is_some()))
                .bind(uuid_of(filter.project))
                .bind(service)
                .bind(service)
                .bind(nanos(lowest))
                .fetch_all()
                .await
                .map_err(|error| refused(&error))?;
            let counts = counted_per_release(&releases, &firsts);
            for (release, count) in releases.iter_mut().zip(counts) {
                release.new_issues = count;
            }
        }
        let window = filter.window;
        releases.retain(|release| {
            release.first_seen_unix_nanos < window.to_unix_nanos()
                && release.last_seen_unix_nanos >= window.from_unix_nanos()
        });
        releases.truncate(RELEASE_LIMIT as usize);
        Ok(releases)
    }

    async fn release(
        &self,
        org: TenantId,
        service: &str,
        version: &str,
        project: Option<ProjectId>,
    ) -> Result<Option<ReleaseDetail>, StoreError> {
        let releases = self.every_release(org, project, Some(service)).await?;
        let ends = rollout_ends(&releases);
        let Some(release) = releases
            .iter()
            .find(|release| *release.service == *service && *release.version == *version)
        else {
            return Ok(None);
        };
        let end = ends.get(&(service, version)).copied().unwrap_or(u64::MAX);
        let introduced = self.introduced_in(org, project, release, end).await?;
        let (before, after) = self.around(org, project, release).await?;
        let mut release = release.clone();
        release.new_issues = introduced.len() as u64;
        Ok(Some(ReleaseDetail {
            release,
            before,
            after,
            introduced,
        }))
    }

    async fn deploys(
        &self,
        org: TenantId,
        filter: &ReleaseFilter,
    ) -> Result<Vec<Deploy>, StoreError> {
        let service = filter.service.as_deref().unwrap_or_default();
        let rows: Vec<DeployRow> = self
            .client()
            .query(&limited(DEPLOYS, DEPLOY_FEED_LIMIT))
            .bind(org.as_uuid())
            .bind(u8::from(filter.project.is_some()))
            .bind(uuid_of(filter.project))
            .bind(service)
            .bind(service)
            .bind(nanos(filter.window.from_unix_nanos()))
            .bind(nanos(filter.window.to_unix_nanos()))
            .fetch_all()
            .await
            .map_err(|error| refused(&error))?;
        Ok(rows.into_iter().map(read_deploy).collect())
    }
}

impl DeployFeed for ClickhouseIssues {
    async fn deployed(&self, window: Window) -> Result<Vec<Deployed>, StoreError> {
        let rows: Vec<FedRow> = self
            .client()
            .query(&limited(FEED, DEPLOY_FEED_LIMIT))
            .bind(nanos(window.from_unix_nanos()))
            .bind(nanos(window.to_unix_nanos()))
            .fetch_all()
            .await
            .map_err(|error| refused(&error))?;
        rows.into_iter()
            .map(|row| {
                Ok(Deployed {
                    org: TenantId::new(Uuid::parse_str(&row.tenant).map_err(|error| held(&error))?),
                    deploy: read_deploy(DeployRow {
                        service: row.service,
                        version: row.version,
                        environment: row.environment,
                        at_nanos: row.at_nanos,
                    }),
                })
            })
            .collect()
    }
}

fn read_deploy(row: DeployRow) -> Deploy {
    Deploy {
        service: row.service.into(),
        version: row.version.into(),
        environment: row.environment.into(),
        at_unix_nanos: unsigned(row.at_nanos),
    }
}

fn counted_per_release(releases: &[Release], firsts: &[FirstSeenRow]) -> Vec<u64> {
    let ends = rollout_ends(releases);
    releases
        .iter()
        .map(|release| {
            let end = ends
                .get(&(&*release.service, &*release.version))
                .copied()
                .unwrap_or(u64::MAX);
            firsts
                .iter()
                .filter(|first| {
                    *first.service == *release.service
                        && unsigned(first.first_nanos) >= release.first_seen_unix_nanos
                        && unsigned(first.first_nanos) < end
                })
                .count() as u64
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use ferristrace_domain::Release;

    use super::{AROUND, DEPLOYS, FEED, FIRST_SEEN_ISSUES, INTRODUCED, RELEASES, rollout_ends};

    fn release(service: &str, version: &str, first: u64) -> Release {
        Release {
            service: service.into(),
            version: version.into(),
            environments: Vec::new(),
            first_seen_unix_nanos: first,
            last_seen_unix_nanos: first,
            requests: 0,
            errors: 0,
            new_issues: 0,
        }
    }

    #[test]
    fn every_organisation_read_filters_on_the_tenant_and_narrows_the_project() {
        for query in [RELEASES, DEPLOYS, FIRST_SEEN_ISSUES, INTRODUCED, AROUND] {
            assert!(query.contains("WHERE tenant_id = ?"), "{query}");
            assert!(query.contains("(? = 0 OR project_id = ?)"), "{query}");
        }
    }

    #[test]
    fn only_the_system_feed_reads_across_tenants() {
        assert!(!FEED.contains("WHERE tenant_id"));
        assert!(FEED.contains("GROUP BY tenant_id, service_name, version, environment"));
    }

    #[test]
    fn a_deploy_is_one_row_per_service_version_and_environment() {
        assert!(DEPLOYS.contains("GROUP BY service_name, version, environment"));
        assert!(RELEASES.contains("GROUP BY service_name, version"));
    }

    #[test]
    fn a_release_lasts_until_the_next_version_of_its_service_first_appears() {
        let releases = vec![
            release("checkout", "2", 200),
            release("checkout", "1", 100),
            release("checkout", "3", 300),
            release("mailer", "7", 150),
        ];

        let ends = rollout_ends(&releases);

        assert_eq!(ends[&("checkout", "1")], 200);
        assert_eq!(ends[&("checkout", "2")], 300);
        assert_eq!(ends[&("checkout", "3")], u64::MAX);
        assert_eq!(ends[&("mailer", "7")], u64::MAX);
    }

    #[test]
    fn two_versions_first_seen_together_end_together() {
        let releases = vec![
            release("checkout", "1", 100),
            release("checkout", "2", 100),
            release("checkout", "3", 300),
        ];

        let ends = rollout_ends(&releases);

        assert_eq!(ends[&("checkout", "1")], 300);
        assert_eq!(ends[&("checkout", "2")], 300);
    }
}
