use std::fmt::{Formatter, Debug};
use core::fmt;
#[macro_export]
macro_rules! if_opt {
    ($child_expr: expr, $some_expr: expr) => {
        if $child_expr {
            Some($some_expr)
        } else {
            None
        }
    };
}

pub struct DebugFn<'a, F: Fn(&mut Formatter) -> fmt::Result>(pub &'a F);

impl<'a, F: Fn(&mut Formatter) -> fmt::Result> Debug for DebugFn<'a, F> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let func = self.0;
        (func)(f)
    }
}
#[macro_export]
macro_rules! debug_fn {
    ($content: expr) => {
        &crate::macros::DebugFn(&$content)
    };
}