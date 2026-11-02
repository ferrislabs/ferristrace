#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadPlacement {
    #[error("a placement name cannot be blank")]
    Blank,
    #[error("a placement name holds at most {max} bytes")]
    TooLong { max: usize },
    #[error(
        "a placement name holds letters, digits, '-', '.' and '_', and starts and ends with a letter or digit"
    )]
    Characters,
    #[error("a workload kind is one of deployment, statefulset, daemonset, cronjob or job")]
    Kind,
}
