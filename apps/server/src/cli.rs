use std::time::Duration;

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use ferristrace_domain::{
    Bootstrapped, ControlPlane, Email, OrgName, OrgRole, ProjectId, ProjectName, Provisioning,
    ResetToken, TenantId,
};
use ferristrace_storage::{INVITE_WINDOW, PostgresControlPlane};
use uuid::Uuid;

use crate::config::Config;
use crate::{SECONDS_PER_HOUR, control_plane_pool};

#[derive(Debug, Parser)]
#[command(name = "comettrace", about = "OTLP ingestion and its control plane")]
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

fn print_the_way_in(token: &ResetToken) {
    eprintln!(
        "The token printed below lasts {} hours and can be spent once:",
        INVITE_WINDOW.as_secs() / SECONDS_PER_HOUR
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

pub(crate) async fn run_command(command: Command, config: &Config) -> anyhow::Result<()> {
    let pool = control_plane_pool(config)
        .await?
        .context("this command needs a control plane, so set database_url")?;
    let provisioning = Provisioning::new(
        PostgresControlPlane::new(pool).context("preparing the password hasher")?,
    );

    match command {
        Command::Health => health(config).await,
        Command::IssueKey { org, project } => issue_key(&provisioning, org, project).await,
        Command::RevokeKey { prefix } => revoke_key(&provisioning, &prefix).await,
        Command::Bootstrap { email, org } => {
            bootstrap(&provisioning, &email, &org, config.local_password).await
        }
        Command::CreateAccount { email } => create_account(&provisioning, &email).await,
        Command::ResetPassword { email } => reset_password(&provisioning, &email).await,
        Command::CreateProject { org, name } => create_project(&provisioning, org, &name).await,
        Command::CreateOrg { name, owner } => create_org(&provisioning, &name, &owner).await,
        Command::GrantRole { org, member, role } => {
            grant_role(&provisioning, org, &member, role.into()).await
        }
    }
}

async fn issue_key<C: ControlPlane>(
    provisioning: &Provisioning<C>,
    org: Uuid,
    project: Option<Uuid>,
) -> anyhow::Result<()> {
    let issued = provisioning
        .issue_key(TenantId::new(org), project.map(ProjectId::new))
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

async fn revoke_key<C: ControlPlane>(
    provisioning: &Provisioning<C>,
    prefix: &str,
) -> anyhow::Result<()> {
    let stopped = provisioning
        .revoke_key(prefix)
        .await
        .context("revoking the key")?;

    if stopped {
        eprintln!("{prefix} will not authenticate the next payload.");
    } else {
        eprintln!("{prefix} matched no live key.");
    }
    Ok(())
}

async fn bootstrap<C: ControlPlane>(
    provisioning: &Provisioning<C>,
    email: &str,
    org: &str,
    local_password: bool,
) -> anyhow::Result<()> {
    let email = Email::try_from(email).context("reading the address")?;
    let name = OrgName::try_from(org).context("reading the name")?;

    let outcome = provisioning
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
                "  comettrace issue-key --org {} --project {}",
                founded.org, founded.project
            );
            match &founded.invitation {
                Some(token) => print_the_way_in(token),
                None => {
                    eprintln!("Local passwords are off, so sign in through the identity provider.");
                }
            }
        }
    }
    Ok(())
}

async fn create_account<C: ControlPlane>(
    provisioning: &Provisioning<C>,
    email: &str,
) -> anyhow::Result<()> {
    let email = Email::try_from(email).context("reading the address")?;
    let token = provisioning
        .create_account(&email)
        .await
        .with_context(|| format!("creating an account for {}", email.as_str()))?;

    eprintln!(
        "{} cannot sign in until it carries a password.",
        email.as_str()
    );
    print_the_way_in(&token);
    Ok(())
}

async fn reset_password<C: ControlPlane>(
    provisioning: &Provisioning<C>,
    email: &str,
) -> anyhow::Result<()> {
    let email = Email::try_from(email).context("reading the address")?;
    let token = provisioning
        .reset_password(&email)
        .await
        .with_context(|| format!("resetting the password of {}", email.as_str()))?;

    eprintln!(
        "Every session of {} ends once the password changes.",
        email.as_str()
    );
    print_the_way_in(&token);
    Ok(())
}

async fn create_project<C: ControlPlane>(
    provisioning: &Provisioning<C>,
    org: Uuid,
    name: &str,
) -> anyhow::Result<()> {
    let name = ProjectName::try_from(name).context("reading the name")?;
    let project = provisioning
        .create_project(TenantId::new(org), &name)
        .await
        .context("creating the project")?;

    eprintln!("{org} now holds {}. Draw a key for it with:", name.as_str());
    eprintln!("  comettrace issue-key --org {org} --project {project}");
    println!("{project}");
    Ok(())
}

async fn create_org<C: ControlPlane>(
    provisioning: &Provisioning<C>,
    name: &str,
    owner: &str,
) -> anyhow::Result<()> {
    let name = OrgName::try_from(name).context("reading the name")?;
    let owner = Email::try_from(owner).context("reading the address")?;
    let org = provisioning
        .create_org(&name, &owner)
        .await
        .context("creating the organisation")?;

    eprintln!(
        "{} now owns {}. Issue its first key with:",
        owner.as_str(),
        name.as_str()
    );
    eprintln!("  comettrace issue-key --org {org}");
    println!("{org}");
    Ok(())
}

async fn grant_role<C: ControlPlane>(
    provisioning: &Provisioning<C>,
    org: Uuid,
    member: &str,
    role: OrgRole,
) -> anyhow::Result<()> {
    let member = Email::try_from(member).context("reading the address")?;
    provisioning
        .grant_role(TenantId::new(org), &member, role)
        .await
        .context("granting the role")?;

    eprintln!("{} is now {role} of {org}.", member.as_str());
    Ok(())
}
