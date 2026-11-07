use super::value_objects::ServiceFilter;

pub(super) trait Slot: Copy {
    fn idle(start_unix_nanos: u64) -> Self;
    fn start_unix_nanos(self) -> u64;
}

pub(super) fn filled<P: Slot>(served: Vec<P>, filter: &ServiceFilter) -> Vec<P> {
    let mut served = served.into_iter().peekable();
    filter
        .slot_starts()
        .map(|start| {
            while served
                .peek()
                .is_some_and(|point| point.start_unix_nanos() < start)
            {
                served.next();
            }
            served
                .next_if(|point| point.start_unix_nanos() == start)
                .unwrap_or_else(|| P::idle(start))
        })
        .collect()
}
