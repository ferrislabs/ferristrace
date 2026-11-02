use std::time::Duration;

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use ferristrace_application::registry::ProvisioningRepos;
use ferristrace_application::{Ferristrace, Stack};
use ferristrace_domain::{
    Bootstrapped, Email, OrgName, OrgRole, ProjectId, ProjectName, ResetToken, TenantId,
};
use uuid::Uuid;

use crate::SECONDS_PER_HOUR;
use crate::config::Config;

#[derive(Debug, Parser)]
#[command(name = "ferristrace", about = "OTLP ingestion and its control plane")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    IssueKey {
        #[arg(long)]
        org: Uuid,
        #[arg(long)]
        project: Option<Uuid>,
    },
    RevokeKey {
        #[arg(long)]
        prefix: String,
    },
    Health,
    Bootstrap {
        #[arg(long)]
        email: String,
        #[arg(long, default_value = "Default")]
        org: String,
    },
    CreateAccount {
        #[arg(long)]
        email: String,
    },
    ResetPassword {
        #[arg(long)]
        email: String,
    },
    CreateProject {
        #[arg(long)]
        org: Uuid,
        #[arg(long)]
        name: String,
    },
    CreateOrg {
        #[arg(long)]
        name: String,
        #[arg(long)]
        owner: String,
    },
    GrantRole {
        #[arg(long)]
        org: Uuid,
        #[arg(long)]
        member: String,
        #[arg(long)]
        role: Role,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum Role {
    Owner,
    Admin,
    Member,
}

impl From<Role> for OrgRole {
    fn from(role: Role) -> Self {
        match role {
            Role::Owner => Self::Owner,
            Role::Admin => Self::Admin,
            Role::Member => Self::Member,
        }
    }
}

fn print_the_way_in(token: &ResetToken, window: Duration) {
    eprintln!(
        "The token printed below lasts {} hours and can be spent once:",
        window.as_secs() / SECONDS_PER_HOUR
    );
    eprintln!(
        "  curl -X POST <base>/auth/reset/confirm -H 'x-requested-with: cli' \\\n    \
         -H 'content-type: application/json' \\\n    \
         -d '{{\"token\":\"<token>\",\"password\":\"<chosen>\"}}'"
    );
    println!("{}", token.exposed());
}

const HEALTH_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) async fn health(config: &Config) -> anyhow::Result<()> {
    let url = format!("http://127.0.0.1:{}/health", config.http_address.port());
    let answer = reqwest::Client::builder()
        .timeout(HEALTH_TIMEOUT)
        .build()
        .context("preparing the request")?
        .get(&url)
        .send()
        .await
        .with_context(|| format!("asking {url}"))?;
    anyhow::ensure!(
        answer.status().is_success(),
        "{url} answered {}",
        answer.status()
    );
    Ok(())
}

pub(crate) async fn run_command<S: Stack>(
    command: Command,
    config: &Config,
    app: &Ferristrace<S>,
    invite_window: Duration,
) -> anyhow::Result<()>
where
    S::Repos: ProvisioningRepos,
{
    match command {
        Command::Health => health(config).await,
        Command::IssueKey { org, project } => issue_key(app, org, project).await,
        Command::RevokeKey { prefix } => revoke_key(app, &prefix).await,
        Command::Bootstrap { email, org } => {
            bootstrap(app, &email, &org, config.local_password, invite_window).await
        }
        Command::CreateAccount { email } => create_account(app, &email, invite_window).await,
        Command::ResetPassword { email } => reset_password(app, &email, invite_window).await,
        Command::CreateProject { org, name } => create_project(app, org, &name).await,
        Command::CreateOrg { name, owner } => create_org(app, &name, &owner).await,
        Command::GrantRole { org, member, role } => {
            grant_role(app, org, &member, role.into()).await
        }
    }
}

async fn issue_key<S: Stack>(
    app: &Ferristrace<S>,
    org: Uuid,
    project: Option<Uuid>,
) -> anyhow::Result<()>
where
    S::Repos: ProvisioningRepos,
{
    let issued = app
        .issue_ingest_key(TenantId::new(org), project.map(ProjectId::new))
        .await
        .context("issuing the key")?;

    if issued.project.is_none() {
        eprintln!(
            "This organisation holds no single project, so this key names none \
             and its spans will match no project. Name one with --project."
        );
    }
    eprintln!("This key is shown once. Store it now; it cannot be read back.");
    println!("{}", issued.key.exposed());
    Ok(())
}

async fn revoke_key<S: Stack>(app: &Ferristrace<S>, prefix: &str) -> anyhow::Result<()>
where
    S::Repos: ProvisioningRepos,
{
    let stopped = app
        .revoke_ingest_key(prefix)
        .await
        .context("revoking the key")?;

    if stopped {
        eprintln!("{prefix} will not authenticate the next payload.");
    } else {
        eprintln!("{prefix} matched no live key.");
    }
    Ok(())
}

async fn bootstrap<S: Stack>(
    app: &Ferristrace<S>,
    email: &str,
    org: &str,
    local_password: bool,
    window: Duration,
) -> anyhow::Result<()>
where
    S::Repos: ProvisioningRepos,
{
    let email = Email::try_from(email).context("reading the address")?;
    let name = OrgName::try_from(org).context("reading the name")?;

    let outcome = app
        .bootstrap(&name, &email, local_password)
        .await
        .context("founding the organisation")?;

    match outcome {
        Bootstrapped::AlreadyFounded { existing } => {
            eprintln!(
                "This deployment already holds an organisation ({existing}), \
                 so nothing was changed."
            );
        }
        Bootstrapped::Founded(founded) => {
            eprintln!(
                "{} owns {} ({}).",
                email.as_str(),
                name.as_str(),
                founded.org
            );
            eprintln!(
                "It holds one project, {} ({}).",
                founded.project_name.as_str(),
                founded.project
            );
            eprintln!("Draw its first ingest key with:");
            eprintln!(
                "  ferristrace issue-key --org {} --project {}",
                founded.org, founded.project
            );
            match &founded.invitation {
                Some(token) => print_the_way_in(token, window),
                None => {
                    eprintln!("Local passwords are off, so sign in through the identity provider.");
                }
            }
        }
    }
    Ok(())
}

async fn create_account<S: Stack>(
    app: &Ferristrace<S>,
    email: &str,
    window: Duration,
) -> anyhow::Result<()>
where
    S::Repos: ProvisioningRepos,
{
    let email = Email::try_from(email).context("reading the address")?;
    let token = app
        .create_account(&email)
        .await
        .with_context(|| format!("creating an account for {}", email.as_str()))?;

    eprintln!(
        "{} cannot sign in until it carries a password.",
        email.as_str()
    );
    print_the_way_in(&token, window);
    Ok(())
}

async fn reset_password<S: Stack>(
    app: &Ferristrace<S>,
    email: &str,
    window: Duration,
) -> anyhow::Result<()>
where
    S::Repos: ProvisioningRepos,
{
    let email = Email::try_from(email).context("reading the address")?;
    let token = app
        .issue_reset_link(&email)
        .await
        .with_context(|| format!("resetting the password of {}", email.as_str()))?;

    eprintln!(
        "Every session of {} ends once the password changes.",
        email.as_str()
    );
    print_the_way_in(&token, window);
    Ok(())
}

async fn create_project<S: Stack>(app: &Ferristrace<S>, org: Uuid, name: &str) -> anyhow::Result<()>
where
    S::Repos: ProvisioningRepos,
{
    let name = ProjectName::try_from(name).context("reading the name")?;
    let project = app
        .claim_project(TenantId::new(org), &name)
        .await
        .context("creating the project")?;

    eprintln!("{org} now holds {}. Draw a key for it with:", name.as_str());
    eprintln!("  ferristrace issue-key --org {org} --project {project}");
    println!("{project}");
    Ok(())
}

async fn create_org<S: Stack>(app: &Ferristrace<S>, name: &str, owner: &str) -> anyhow::Result<()>
where
    S::Repos: ProvisioningRepos,
{
    let name = OrgName::try_from(name).context("reading the name")?;
    let owner = Email::try_from(owner).context("reading the address")?;
    let org = app
        .create_org(&name, &owner)
        .await
        .context("creating the organisation")?;

    eprintln!(
        "{} now owns {}. Issue its first key with:",
        owner.as_str(),
        name.as_str()
    );
    eprintln!("  ferristrace issue-key --org {org}");
    println!("{org}");
    Ok(())
}

async fn grant_role<S: Stack>(
    app: &Ferristrace<S>,
    org: Uuid,
    member: &str,
    role: OrgRole,
) -> anyhow::Result<()>
where
    S::Repos: ProvisioningRepos,
{
    let member = Email::try_from(member).context("reading the address")?;
    app.grant_org_role(TenantId::new(org), &member, role)
        .await
        .context("granting the role")?;

    eprintln!("{} is now {role} of {org}.", member.as_str());
    Ok(())
}
