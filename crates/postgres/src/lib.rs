extern crate self as ferristrace_postgres;

mod unit_of_work;

pub use unit_of_work::{Postgres, SharedTx};
