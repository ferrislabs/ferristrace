use super::entities::{
    Component, ComponentDraft, ComponentPatch, Link, Listed, PageDraft, PagePatch, Saved, Sheet,
    StatusPage,
};
use super::errors::StatusError;
use super::ports::{StatusPages, StatusSubscribers};
use super::value_objects::{ComponentId, MAX_COMPONENTS, StatusPageId};
use crate::access::{ApiKeys, Reader, Readers};
use crate::alerts::AlertRules;
use crate::audit::{Action, Actor, AuditEntry, AuditTrail, Detail, Target, TargetKind};
use crate::catalogue::Projects;
use crate::clock::UnixSeconds;
use crate::identity::{AccountId, SessionStore};
use crate::organisation::{Memberships, OrgRole};
use crate::telemetry::TenantId;

const MILLIS_PER_SECOND: u64 = 1_000;

#[derive(Debug)]
pub struct StatusDesk<S, G, K, P, R, Pg, Sb, A> {
    readers: Readers<S, G, K>,
    projects: P,
    rules: R,
    pages: Pg,
    subscribers: Sb,
    trail: A,
}

impl<S, G, K, P, R, Pg, Sb, A> StatusDesk<S, G, K, P, R, Pg, Sb, A>
where
    S: SessionStore,
    G: Memberships,
    K: ApiKeys,
    P: Projects,
    R: AlertRules,
    Pg: StatusPages,
    Sb: StatusSubscribers,
    A: AuditTrail,
{
    pub fn new(
        readers: Readers<S, G, K>,
        projects: P,
        rules: R,
        pages: Pg,
        subscribers: Sb,
        trail: A,
    ) -> Self {
        Self {
            readers,
            projects,
            rules,
            pages,
            subscribers,
            trail,
        }
    }

    pub async fn list(&self, who: &Reader, org: TenantId) -> Result<Vec<Listed>, StatusError> {
        self.readers.grant(who, org).await?;
        let mut listed = Vec::new();
        for page in self.pages.list(org).await? {
            let subscribers = self.subscribers.confirmed(page.id).await?;
            listed.push(Listed { page, subscribers });
        }
        Ok(listed)
    }

    pub async fn get(
        &self,
        who: &Reader,
        org: TenantId,
        id: StatusPageId,
    ) -> Result<Sheet, StatusError> {
        self.readers.grant(who, org).await?;
        let page = self.page(org, id).await?;
        self.sheet(page).await
    }

    pub async fn create(
        &self,
        who: &Reader,
        org: TenantId,
        draft: &PageDraft,
        now: UnixSeconds,
    ) -> Result<Sheet, StatusError> {
        let by = self.admin(who, org).await?;
        let now_ms = millis(now);
        let page = StatusPage {
            id: StatusPageId::fresh(),
            org,
            name: draft.name.clone(),
            slug: draft.slug.clone(),
            public: draft.public,
            hide_from_search: draft.hide_from_search,
            theme: draft.theme,
            intro: draft.intro.clone(),
            created_ms: now_ms,
            updated_ms: now_ms,
        };
        saved(self.pages.create(&page).await?)?;
        self.note(
            org,
            by,
            Action::StatusPageCreated,
            Target::of(TargetKind::StatusPage, Some(page.id.to_string().into())),
            Detail::default()
                .with("name", &page.name)
                .with("slug", &page.slug)
                .with("public", page.public),
        )
        .await?;
        self.sheet(page).await
    }

    pub async fn update(
        &self,
        who: &Reader,
        org: TenantId,
        id: StatusPageId,
        patch: PagePatch,
        now: UnixSeconds,
    ) -> Result<Sheet, StatusError> {
        let by = self.admin(who, org).await?;
        let held = self.page(org, id).await?;
        let mut changed = held.clone();
        let mut fields = Vec::new();
        if let Some(name) = patch.name
            && name != held.name
        {
            changed.name = name;
            fields.push("name");
        }
        if let Some(slug) = patch.slug
            && slug != held.slug
        {
            changed.slug = slug;
            fields.push("slug");
        }
        if let Some(public) = patch.public
            && public != held.public
        {
            changed.public = public;
            fields.push("public");
        }
        if let Some(hide) = patch.hide_from_search
            && hide != held.hide_from_search
        {
            changed.hide_from_search = hide;
            fields.push("hide_from_search");
        }
        if let Some(theme) = patch.theme
            && theme != held.theme
        {
            changed.theme = theme;
            fields.push("theme");
        }
        if let Some(intro) = patch.intro
            && intro != held.intro
        {
            changed.intro = intro;
            fields.push("intro");
        }
        if fields.is_empty() {
            return self.sheet(held).await;
        }
        changed.updated_ms = millis(now);
        saved(self.pages.keep(&changed).await?)?;
        self.note(
            org,
            by,
            Action::StatusPageUpdated,
            Target::of(TargetKind::StatusPage, Some(id.to_string().into())),
            Detail::default()
                .with_all("fields", &fields)
                .with("public", changed.public),
        )
        .await?;
        self.sheet(changed).await
    }

    pub async fn delete(
        &self,
        who: &Reader,
        org: TenantId,
        id: StatusPageId,
    ) -> Result<(), StatusError> {
        let by = self.admin(who, org).await?;
        let held = self.page(org, id).await?;
        if !self.pages.delete(org, id).await? {
            return Err(StatusError::NoSuchPage);
        }
        self.note(
            org,
            by,
            Action::StatusPageDeleted,
            Target::of(TargetKind::StatusPage, Some(id.to_string().into())),
            Detail::default().with("slug", &held.slug),
        )
        .await
    }

    pub async fn add_component(
        &self,
        who: &Reader,
        org: TenantId,
        page: StatusPageId,
        draft: &ComponentDraft,
    ) -> Result<Component, StatusError> {
        let by = self.admin(who, org).await?;
        self.page(org, page).await?;
        self.check_link(org, &draft.link).await?;
        let held = self.pages.components(org, page).await?;
        if held.len() >= MAX_COMPONENTS {
            return Err(StatusError::TooManyComponents {
                max: MAX_COMPONENTS,
            });
        }
        let position = held
            .iter()
            .map(|component| component.position + 1)
            .max()
            .unwrap_or_default();
        let component = Component {
            id: ComponentId::fresh(),
            page,
            name: draft.name.clone(),
            link: draft.link.clone(),
            manual_status: draft.manual_status,
            position,
            public: draft.public,
        };
        if !self.pages.add_component(org, &component).await? {
            return Err(StatusError::NoSuchPage);
        }
        self.note_component(&component, org, by, Action::StatusComponentCreated)
            .await?;
        Ok(component)
    }

    pub async fn change_component(
        &self,
        who: &Reader,
        org: TenantId,
        page: StatusPageId,
        id: ComponentId,
        patch: ComponentPatch,
    ) -> Result<Component, StatusError> {
        let by = self.admin(who, org).await?;
        let held = self.component(org, page, id).await?;
        if let Some(link) = &patch.link {
            self.check_link(org, link).await?;
        }
        let changed = Component {
            name: patch.name.unwrap_or_else(|| held.name.clone()),
            link: patch.link.unwrap_or_else(|| held.link.clone()),
            manual_status: patch.manual_status.unwrap_or(held.manual_status),
            public: patch.public.unwrap_or(held.public),
            ..held.clone()
        };
        if changed == held {
            return Ok(held);
        }
        if !self.pages.keep_component(org, &changed).await? {
            return Err(StatusError::NoSuchComponent);
        }
        self.note_component(&changed, org, by, Action::StatusComponentUpdated)
            .await?;
        Ok(changed)
    }

    pub async fn remove_component(
        &self,
        who: &Reader,
        org: TenantId,
        page: StatusPageId,
        id: ComponentId,
    ) -> Result<(), StatusError> {
        let by = self.admin(who, org).await?;
        let held = self.component(org, page, id).await?;
        if !self.pages.remove_component(org, page, id).await? {
            return Err(StatusError::NoSuchComponent);
        }
        self.note_component(&held, org, by, Action::StatusComponentDeleted)
            .await
    }

    pub async fn order(
        &self,
        who: &Reader,
        org: TenantId,
        page: StatusPageId,
        ids: &[ComponentId],
    ) -> Result<Vec<Component>, StatusError> {
        let by = self.admin(who, org).await?;
        self.page(org, page).await?;
        let held = self.pages.components(org, page).await?;
        let mut asked = ids.to_vec();
        asked.sort_unstable();
        asked.dedup();
        let mut present: Vec<ComponentId> = held.iter().map(|component| component.id).collect();
        present.sort_unstable();
        if asked.len() != ids.len() || asked != present {
            return Err(StatusError::Order);
        }
        let mut ordered = Vec::with_capacity(held.len());
        for (position, id) in (0_u32..).zip(ids) {
            let Some(component) = held.iter().find(|component| component.id == *id) else {
                return Err(StatusError::Order);
            };
            let moved = Component {
                position,
                ..component.clone()
            };
            if moved.position != component.position
                && !self.pages.keep_component(org, &moved).await?
            {
                return Err(StatusError::NoSuchComponent);
            }
            ordered.push(moved);
        }
        self.note(
            org,
            by,
            Action::StatusPageUpdated,
            Target::of(TargetKind::StatusPage, Some(page.to_string().into())),
            Detail::default().with("fields", "order"),
        )
        .await?;
        Ok(ordered)
    }

    async fn admin(&self, who: &Reader, org: TenantId) -> Result<AccountId, StatusError> {
        match who {
            Reader::Key(_) | Reader::Agent(_) => {
                self.readers.grant(who, org).await?;
                Err(StatusError::ReadOnly)
            }
            Reader::Session(token) => {
                let who = self.readers.whoami().of(token).await?;
                match who.role_in(org) {
                    None => Err(StatusError::NotYours),
                    Some(role) if role.at_least(OrgRole::Admin) => Ok(who.account),
                    Some(_) => Err(StatusError::NotAdmin),
                }
            }
        }
    }

    async fn page(&self, org: TenantId, id: StatusPageId) -> Result<StatusPage, StatusError> {
        self.pages
            .get(org, id)
            .await?
            .ok_or(StatusError::NoSuchPage)
    }

    async fn component(
        &self,
        org: TenantId,
        page: StatusPageId,
        id: ComponentId,
    ) -> Result<Component, StatusError> {
        self.page(org, page).await?;
        self.pages
            .components(org, page)
            .await?
            .into_iter()
            .find(|component| component.id == id)
            .ok_or(StatusError::NoSuchComponent)
    }

    async fn sheet(&self, page: StatusPage) -> Result<Sheet, StatusError> {
        let components = self.pages.components(page.org, page.id).await?;
        let subscribers = self.subscribers.confirmed(page.id).await?;
        Ok(Sheet {
            page,
            components,
            subscribers,
        })
    }

    async fn check_link(&self, org: TenantId, link: &Link) -> Result<(), StatusError> {
        match link {
            Link::Manual | Link::Service { project: None, .. } => Ok(()),
            Link::Alert(rule) => self
                .rules
                .get(org, *rule)
                .await?
                .map(|_| ())
                .ok_or(StatusError::NoSuchAlert),
            Link::Service {
                project: Some(project),
                ..
            } => {
                if self
                    .projects
                    .of(org)
                    .await?
                    .iter()
                    .any(|held| held.id == *project)
                {
                    Ok(())
                } else {
                    Err(StatusError::NoSuchProject)
                }
            }
        }
    }

    async fn note_component(
        &self,
        component: &Component,
        org: TenantId,
        by: AccountId,
        action: Action,
    ) -> Result<(), StatusError> {
        self.note(
            org,
            by,
            action,
            Target::of(
                TargetKind::StatusComponent,
                Some(component.id.to_string().into()),
            ),
            Detail::default()
                .with("page", component.page)
                .with("name", &component.name)
                .with("link", component.link.kind())
                .with("public", component.public),
        )
        .await
    }

    async fn note(
        &self,
        org: TenantId,
        by: AccountId,
        action: Action,
        target: Target,
        detail: Detail,
    ) -> Result<(), StatusError> {
        Ok(self
            .trail
            .record(&AuditEntry {
                org,
                actor: Actor::Account(by),
                action,
                target,
                detail,
            })
            .await?)
    }
}

fn saved(saved: Saved) -> Result<(), StatusError> {
    match saved {
        Saved::Done => Ok(()),
        Saved::SlugTaken => Err(StatusError::SlugTaken),
        Saved::Gone => Err(StatusError::NoSuchPage),
    }
}

fn millis(now: UnixSeconds) -> u64 {
    now.get().saturating_mul(MILLIS_PER_SECOND)
}
