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
