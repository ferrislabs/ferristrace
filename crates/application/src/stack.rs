use crate::registry::UnitOfWork;

pub trait Stack: Send + Sync + 'static {
    type Repos: UnitOfWork;
}

#[derive(Debug)]
pub struct Ferristrace<S: Stack> {
    pub(crate) repos: S::Repos,
}

impl<S: Stack> Ferristrace<S> {
    pub fn new(repos: S::Repos) -> Self {
        Self { repos }
    }
}
