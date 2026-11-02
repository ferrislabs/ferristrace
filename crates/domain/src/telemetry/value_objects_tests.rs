use super::errors::InvalidId;
use super::value_objects::{SpanId, TraceId};

#[test]
fn a_trace_id_needs_sixteen_bytes() {
    assert_eq!(
        TraceId::try_from([1u8; 8].as_slice()),
        Err(InvalidId::Length {
            expected: 16,
            actual: 8
        })
    );
}

#[test]
fn an_all_zero_identifier_is_invalid() {
    assert_eq!(
        TraceId::try_from([0u8; 16].as_slice()),
        Err(InvalidId::Zero)
    );
    assert_eq!(SpanId::try_from([0u8; 8].as_slice()), Err(InvalidId::Zero));
}

#[test]
fn a_well_formed_identifier_keeps_its_bytes() {
    let id = SpanId::try_from([1, 2, 3, 4, 5, 6, 7, 8].as_slice()).expect("eight bytes");
    assert_eq!(id.as_bytes(), &[1, 2, 3, 4, 5, 6, 7, 8]);
}

mod proptests {
    use proptest::prelude::*;

    use super::{InvalidId, SpanId, TraceId};

    proptest! {
        #[test]
        fn a_trace_id_of_the_correct_width_round_trips_unless_it_is_all_zero(
            bytes in prop::collection::vec(any::<u8>(), TraceId::WIDTH)
        ) {
            let outcome = TraceId::try_from(bytes.as_slice());
            if bytes.iter().all(|byte| *byte == 0) {
                prop_assert_eq!(outcome, Err(InvalidId::Zero));
            } else {
                let id = outcome.expect("a nonzero array of the trace id width");
                prop_assert_eq!(id.as_bytes().as_slice(), bytes.as_slice());
            }
        }

        #[test]
        fn a_trace_id_of_the_wrong_width_is_refused_rather_than_coerced(
            bytes in prop::collection::vec(any::<u8>(), 0..40)
                .prop_filter("not the trace id width", |bytes| bytes.len() != TraceId::WIDTH)
        ) {
            prop_assert_eq!(
                TraceId::try_from(bytes.as_slice()),
                Err(InvalidId::Length { expected: TraceId::WIDTH, actual: bytes.len() })
            );
        }

        #[test]
        fn a_span_id_of_the_correct_width_round_trips_unless_it_is_all_zero(
            bytes in prop::collection::vec(any::<u8>(), SpanId::WIDTH)
        ) {
            let outcome = SpanId::try_from(bytes.as_slice());
            if bytes.iter().all(|byte| *byte == 0) {
                prop_assert_eq!(outcome, Err(InvalidId::Zero));
            } else {
                let id = outcome.expect("a nonzero array of the span id width");
                prop_assert_eq!(id.as_bytes().as_slice(), bytes.as_slice());
            }
        }

        #[test]
        fn a_span_id_of_the_wrong_width_is_refused_rather_than_coerced(
            bytes in prop::collection::vec(any::<u8>(), 0..40)
                .prop_filter("not the span id width", |bytes| bytes.len() != SpanId::WIDTH)
        ) {
            prop_assert_eq!(
                SpanId::try_from(bytes.as_slice()),
                Err(InvalidId::Length { expected: SpanId::WIDTH, actual: bytes.len() })
            );
        }
    }
}
