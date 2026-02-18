#[macro_export]
macro_rules! versioned_read_args {
    // Dispatch a BinRead::read between two versioned raw types and run
    // the provided block with the parsed `raw` bound.

    ($ver:expr, $reader:expr,
     import: $imp:expr,
     v1: $V1:ty,
     v2: $V2:ty,
     as $raw:ident => $block:block
    ) => {{
        if $ver.major() == 1 {
            let $raw: $V1 = <$V1>::read_args($reader, $imp)?;
            $block
        } else {
            let $raw: $V2 = <$V2>::read_args($reader, $imp)?;
            $block
        }
    }};
}

#[macro_export]
macro_rules! versioned_read {
    ($ver:expr, $reader:expr,
     v1: $V1:ty,
     v2: $V2:ty,
     as $raw:ident => $block:block
    ) => {{
        versioned_read_args!($ver, $reader,
            import: (),
            v1: $V1,
            v2: $V2,
            as $raw => $block
        )
    }};
}