macro_rules! dispatch_next {
    ($tail_code:expr, $consumed:expr, $ctx:expr) => {{
        call_next($tail_code, $consumed, $ctx)
    }};
}
